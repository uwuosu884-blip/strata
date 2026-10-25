//! Sample decoding (symphonia), resampling (rubato) and an asynchronous,
//! de-duplicating cache keyed by reference string (path / `factory:` / `recording:`).

use crate::factory;
use daw_engine::SampleBuffer;
use daw_project::{FACTORY_PREFIX, RECORDING_PREFIX};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

/// Decode any supported file (wav, flac, mp3, ogg/vorbis, aac/m4a, aiff…) to stereo f32.
pub fn decode_file(path: &Path) -> Result<SampleBuffer, String> {
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::errors::Error;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;

    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| format!("{}: unsupported format ({e})", path.display()))?;
    let track = format.default_track(TrackType::Audio).ok_or("no audio track")?;
    let track_id = track.id;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or("not an audio track")?.clone();
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| format!("no decoder: {e}"))?;

    let (mut left, mut right) = (Vec::new(), Vec::new());
    let mut rate = params.sample_rate.unwrap_or(48_000);
    let mut scratch: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(Error::ResetRequired) => break,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("read error: {e}")),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(b) => b,
            Err(Error::DecodeError(_)) => continue, // skip corrupt frame
            Err(e) => return Err(format!("decode error: {e}")),
        };
        rate = buf.spec().rate();
        let ch = buf.spec().channels().count().max(1);
        scratch.resize(buf.samples_interleaved(), 0.0);
        buf.copy_to_slice_interleaved(&mut scratch);
        left.reserve(scratch.len() / ch);
        right.reserve(scratch.len() / ch);
        for f in scratch.chunks_exact(ch) {
            left.push(f[0]);
            right.push(if ch > 1 { f[1] } else { f[0] });
        }
    }
    if left.is_empty() {
        return Err(format!("{}: no audio decoded", path.display()));
    }
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("sample");
    Ok(SampleBuffer::new(name, rate, left, right))
}

/// High-quality offline resample to `target` Hz (no-op when rates match).
pub fn resample(buf: SampleBuffer, target: u32) -> Result<SampleBuffer, String> {
    use rubato::audioadapter_buffers::direct::InterleavedSlice;
    use rubato::{Fft, FixedSync, Resampler};
    if buf.sample_rate == target || buf.is_empty() {
        return Ok(buf);
    }
    let frames = buf.len();
    let data: Vec<f32> = buf.left.iter().zip(&buf.right).flat_map(|(l, r)| [*l, *r]).collect();
    let mut rs =
        Fft::<f32>::new(buf.sample_rate as usize, target as usize, 1024, 2, FixedSync::Input).map_err(|e| format!("resampler: {e}"))?;
    let input = InterleavedSlice::new(&data, 2, frames).map_err(|e| format!("resampler input: {e}"))?;
    let out = rs.process_all(&input, frames, None).map_err(|e| format!("resample: {e}"))?.take_data();
    let (mut l, mut r) = (Vec::with_capacity(out.len() / 2), Vec::with_capacity(out.len() / 2));
    for f in out.as_chunks::<2>().0 {
        l.push(f[0]);
        r.push(f[1]);
    }
    Ok(SampleBuffer::new(buf.name, target, l, r))
}

/// Load (decode + resample) any reference synchronously.
fn load_reference(key: &str, sr: u32) -> Result<SampleBuffer, String> {
    if let Some(name) = key.strip_prefix(FACTORY_PREFIX) {
        return factory::generate(name, sr).ok_or_else(|| format!("unknown factory sound {name}"));
    }
    if key.starts_with(RECORDING_PREFIX) {
        return Err("recording is no longer in memory".into());
    }
    resample(decode_file(Path::new(key))?, sr)
}

type LoadResult = (String, Result<SampleBuffer, String>);

/// Decoded-sample cache. Loads happen on a background thread; call
/// [`SampleCache::poll`] each frame and recompile the song when it returns > 0.
pub struct SampleCache {
    map: HashMap<String, Arc<SampleBuffer>>,
    pending: HashSet<String>,
    failed: HashMap<String, String>,
    tx: Sender<String>,
    rx: Receiver<LoadResult>,
    sample_rate: u32,
}

impl SampleCache {
    /// `sample_rate`: files are resampled to this (the engine's rate).
    pub fn new(sample_rate: u32) -> Self {
        let (tx, job_rx) = channel::<String>();
        let (done_tx, rx) = channel::<LoadResult>();
        std::thread::Builder::new()
            .name("daw-sample-loader".into())
            .spawn(move || {
                while let Ok(key) = job_rx.recv() {
                    let res = load_reference(&key, sample_rate);
                    if done_tx.send((key, res)).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn sample loader");
        Self { map: HashMap::new(), pending: HashSet::new(), failed: HashMap::new(), tx, rx, sample_rate }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Get a sample, starting a background load if needed. Factory sounds
    /// are generated synchronously (they take < 1 ms).
    pub fn get(&mut self, key: &str) -> Option<Arc<SampleBuffer>> {
        if let Some(b) = self.map.get(key) {
            return Some(b.clone());
        }
        if key.starts_with(FACTORY_PREFIX) {
            return self.load_blocking(key).ok();
        }
        if !self.pending.contains(key) && !self.failed.contains_key(key) && !key.starts_with(RECORDING_PREFIX) {
            self.pending.insert(key.to_string());
            let _ = self.tx.send(key.to_string());
        }
        None
    }

    /// Load now on the calling thread (export, CLI, tests).
    pub fn load_blocking(&mut self, key: &str) -> Result<Arc<SampleBuffer>, String> {
        if let Some(b) = self.map.get(key) {
            return Ok(b.clone());
        }
        match load_reference(key, self.sample_rate) {
            Ok(b) => Ok(self.insert(key, b)),
            Err(e) => {
                self.failed.insert(key.to_string(), e.clone());
                Err(e)
            }
        }
    }

    pub fn insert(&mut self, key: &str, buf: SampleBuffer) -> Arc<SampleBuffer> {
        let arc = Arc::new(buf);
        self.map.insert(key.to_string(), arc.clone());
        self.pending.remove(key);
        self.failed.remove(key);
        arc
    }

    /// Re-key an entry (recording saved to disk → path).
    pub fn rename(&mut self, from: &str, to: &str) {
        if let Some(b) = self.map.remove(from) {
            self.map.insert(to.to_string(), b);
        }
    }

    /// Collect finished background loads. Returns how many arrived.
    pub fn poll(&mut self) -> usize {
        let mut n = 0;
        while let Ok((key, res)) = self.rx.try_recv() {
            self.pending.remove(&key);
            match res {
                Ok(b) => {
                    self.map.insert(key, Arc::new(b));
                    n += 1;
                }
                Err(e) => {
                    self.failed.insert(key, e);
                    n += 1;
                }
            }
        }
        n
    }

    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn error(&self, key: &str) -> Option<&str> {
        self.failed.get(key).map(String::as_str)
    }

    pub fn peek(&self, key: &str) -> Option<&Arc<SampleBuffer>> {
        self.map.get(key)
    }

    /// Forget a failure so the next `get` retries.
    pub fn retry(&mut self, key: &str) {
        self.failed.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_test_wav(path: &Path, sr: u32, channels: u16, frames: usize) {
        let spec = hound::WavSpec { channels, sample_rate: sr, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..frames {
            let v = ((i as f32 * 440.0 * std::f32::consts::TAU / sr as f32).sin() * 16000.0) as i16;
            for c in 0..channels {
                w.write_sample(if c == 0 { v } else { -v }).unwrap();
            }
        }
        w.finalize().unwrap();
    }

    #[test]
    fn decodes_wav_and_resamples() {
        let dir = std::env::temp_dir().join(format!("daw-core-dec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("tone.wav");
        write_test_wav(&p, 44_100, 2, 44_100);
        let b = decode_file(&p).unwrap();
        assert_eq!((b.sample_rate, b.len()), (44_100, 44_100));
        assert!((b.left[100] + b.right[100]).abs() < 1e-3, "stereo preserved");
        let r = resample(b, 48_000).unwrap();
        assert_eq!(r.sample_rate, 48_000);
        assert!((r.len() as i64 - 48_000).abs() < 64, "len {}", r.len());
        let peak = r.left[1000..47_000].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.488).abs() < 0.02, "peak {peak}");

        let mono = dir.join("mono.wav");
        write_test_wav(&mono, 48_000, 1, 1000);
        let m = decode_file(&mono).unwrap();
        assert_eq!(m.left, m.right);
        assert!(decode_file(&dir.join("missing.wav")).is_err());
        std::fs::write(dir.join("junk.wav"), b"not audio at all").unwrap();
        assert!(decode_file(&dir.join("junk.wav")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cache_loads_in_background_and_dedups() {
        let dir = std::env::temp_dir().join(format!("daw-core-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.wav");
        write_test_wav(&p, 48_000, 2, 4800);
        let key = p.to_str().unwrap();
        let mut c = SampleCache::new(48_000);
        assert!(c.get(key).is_none());
        assert!(c.get(key).is_none());
        assert!(c.is_pending());
        let t = std::time::Instant::now();
        while c.poll() == 0 {
            assert!(t.elapsed().as_secs() < 10);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(c.get(key).unwrap().len(), 4800);
        assert!(!c.is_pending());
        assert!(c.get("factory:Kick").is_some());
        let bad = dir.join("nope.wav");
        assert!(c.load_blocking(bad.to_str().unwrap()).is_err());
        assert!(c.error(bad.to_str().unwrap()).is_some());
        assert!(c.get(bad.to_str().unwrap()).is_none() && !c.is_pending(), "failed keys are not retried");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
