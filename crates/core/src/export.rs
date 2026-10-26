//! Offline export: drives the very same `Engine::process` faster than real time.

use crate::compile::{RtSlots, compile};
use crate::samples::SampleCache;
use daw_engine::{Command, EngineConfig, create};
use daw_project::{ClipContent, InstrumentDef, Project};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub sample_rate: u32,
    /// Render one session scene instead of the arrangement.
    pub scene: Option<usize>,
    /// Explicit length in bars (default: arrangement end / longest scene clip).
    pub bars: Option<f64>,
    /// Extra seconds after the end for release tails.
    pub tail_seconds: f64,
    pub metronome: bool,
    pub block: usize,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { sample_rate: 48_000, scene: None, bars: None, tail_seconds: 2.0, metronome: false, block: 512 }
    }
}

pub struct Rendered {
    /// Interleaved stereo.
    pub data: Vec<f32>,
    pub sample_rate: u32,
    /// Samples that could not be loaded (rendered silent).
    pub warnings: Vec<String>,
    pub beats: f64,
}

impl Rendered {
    pub fn peak(&self) -> f32 {
        self.data.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }
}

/// Every sample reference used by the project.
pub fn sample_refs(p: &Project) -> Vec<String> {
    let mut v = Vec::new();
    for t in &p.tracks {
        match &t.instrument {
            InstrumentDef::Sampler { sample: Some(s), .. } => v.push(s.clone()),
            InstrumentDef::Drums { pads } => v.extend(pads.iter().filter_map(|d| d.sample.clone())),
            _ => {}
        }
        for c in t.slots.iter().flatten().chain(t.arrangement.iter().map(|a| &a.clip)) {
            if let ClipContent::Audio { sample, .. } = &c.content {
                v.push(sample.clone());
            }
        }
    }
    v.sort();
    v.dedup();
    v
}

/// Render with a private cache (loads everything synchronously first).
pub fn render_project(p: &Project, opts: &ExportOptions) -> Result<Rendered, String> {
    let mut cache = SampleCache::new(opts.sample_rate);
    render_with_cache(p, &mut cache, opts)
}

/// Render using an existing cache (the app passes its own; recordings live there).
pub fn render_with_cache(p: &Project, cache: &mut SampleCache, opts: &ExportOptions) -> Result<Rendered, String> {
    let mut warnings = Vec::new();
    for key in sample_refs(p) {
        if cache.peek(&key).is_none()
            && let Err(e) = cache.load_blocking(&key)
        {
            warnings.push(e);
        }
    }
    let bpb = p.beats_per_bar.max(1) as f64;
    let beats = match (opts.bars, opts.scene) {
        (Some(b), _) => b * bpb,
        (None, Some(s)) => {
            if s >= p.scenes.len() {
                return Err(format!("scene {} does not exist (project has {})", s + 1, p.scenes.len()));
            }
            let longest = p.tracks.iter().filter_map(|t| t.slots.get(s)?.as_ref()).map(|c| c.length).fold(0.0, f64::max);
            (longest / bpb).ceil().max(1.0) * bpb
        }
        (None, None) => {
            let end = p.arrangement_end();
            if end <= 0.0 {
                return Err("the arrangement is empty (use --scene N or --bars N)".into());
            }
            end
        }
    };

    let sr = opts.sample_rate as f64;
    let block = opts.block.clamp(16, 8192);
    let (mut engine, mut h) = create(EngineConfig { sample_rate: sr, max_block_frames: block });
    let song = compile(p, cache, &mut RtSlots::default());
    h.set_song(song).map_err(|_| "song queue full")?;
    let mut cmds = vec![
        Command::SetTempo(p.tempo_bpm),
        Command::SetBeatsPerBar(p.beats_per_bar),
        Command::SetMetronome(opts.metronome),
        Command::SetMasterGain(p.master_gain_linear()),
        Command::SetMasterPan(p.master_pan),
        Command::SetLoop { enabled: false, start: 0.0, end: 0.0 },
        Command::SetLaunchQuantize(0.0),
    ];
    cmds.push(match opts.scene {
        Some(s) => Command::LaunchScene { scene: s as u32 },
        None => Command::Play,
    });
    for c in cmds {
        h.send(c).map_err(|_| "command queue full")?;
    }

    let play_frames = (beats * 60.0 / p.tempo_bpm * sr).round() as usize;
    let tail_frames = (opts.tail_seconds.max(0.0) * sr) as usize;
    let mut data = vec![0.0f32; (play_frames + tail_frames) * 2];
    let (head, tail) = data.split_at_mut(play_frames * 2);
    for chunk in head.chunks_mut(block * 2) {
        engine.process(chunk, 2);
    }
    h.send(Command::Stop).map_err(|_| "command queue full")?;
    for chunk in tail.chunks_mut(block * 2) {
        engine.process(chunk, 2);
    }
    Ok(Rendered { data, sample_rate: opts.sample_rate, warnings, beats })
}

/// Write interleaved stereo as 16/24-bit PCM (TPDF-dithered) or 32-bit float WAV.
pub fn write_wav(path: &Path, data: &[f32], sample_rate: u32, bits: u16) -> Result<(), String> {
    let float = bits == 32;
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: if float {
            32
        } else if bits == 24 {
            24
        } else {
            16
        },
        sample_format: if float { hound::SampleFormat::Float } else { hound::SampleFormat::Int },
    };
    let tmp = path.with_extension("wav.tmp");
    let mut w = hound::WavWriter::create(&tmp, spec).map_err(|e| format!("{}: {e}", path.display()))?;
    let res: Result<(), hound::Error> = if float {
        data.iter().try_for_each(|&s| w.write_sample(s))
    } else {
        // TPDF dither when reducing word length.
        let mut seed = 0x1234_5678u32;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32
        };
        if bits == 24 {
            const FS: f32 = 8_388_607.0;
            data.iter().try_for_each(|&s| {
                let d = (rnd() - rnd()) / FS;
                w.write_sample(((s + d).clamp(-1.0, 1.0) * FS).round() as i32)
            })
        } else {
            let mut w16 = w.get_i16_writer(data.len() as u32);
            for &s in data {
                let d = (rnd() - rnd()) / 32768.0;
                w16.write_sample(((s + d).clamp(-1.0, 1.0) * 32767.0).round() as i16);
            }
            w16.flush()
        }
    };
    res.and_then(|_| w.finalize()).map_err(|e| format!("write failed: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_project::{ArrangementClip, Clip, Note};

    fn drum_project() -> Project {
        let mut p = Project::new_default();
        let notes: Vec<Note> = (0..4).map(|i| Note { start: i as f64, length: 0.25, pitch: 36, velocity: 110, muted: false }).collect();
        p.tracks[0].slots[0] = Some(Clip { length: 4.0, content: ClipContent::Midi { notes: notes.clone() }, ..Default::default() });
        p.tracks[0].arrangement.push(ArrangementClip {
            start: 4.0,
            clip: Clip { length: 4.0, looping: false, content: ClipContent::Midi { notes }, ..Default::default() },
        });
        p
    }

    #[test]
    fn renders_arrangement_and_scene() {
        let p = drum_project();
        let opts = ExportOptions { tail_seconds: 0.5, ..Default::default() };
        let r = render_project(&p, &opts).unwrap();
        assert_eq!(r.beats, 8.0);
        assert_eq!(r.data.len(), (8 * 24_000 + 24_000) * 2);
        assert!(r.warnings.is_empty());
        // Silent first bar, kicks in bar 2 starting at beat 4.
        assert!(r.data[..4 * 24_000 * 2].iter().all(|v| *v == 0.0));
        assert!(r.data[4 * 48_000..4 * 48_000 + 2000].iter().any(|v| v.abs() > 0.3));

        let s = render_project(&p, &ExportOptions { scene: Some(0), tail_seconds: 0.0, ..Default::default() }).unwrap();
        assert_eq!(s.beats, 4.0);
        assert!(s.data[..2000].iter().any(|v| v.abs() > 0.3), "scene starts immediately");
        assert!(render_project(&p, &ExportOptions { scene: Some(99), ..Default::default() }).is_err());
        assert!(render_project(&Project::new_default(), &ExportOptions::default()).is_err());
    }

    #[test]
    fn render_is_deterministic_and_block_size_independent() {
        let p = drum_project();
        let a = render_project(&p, &ExportOptions { block: 512, ..Default::default() }).unwrap();
        let b = render_project(&p, &ExportOptions { block: 37, ..Default::default() }).unwrap();
        assert_eq!(a.data.len(), b.data.len());
        let max_diff = a.data.iter().zip(&b.data).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
        assert!(max_diff < 1e-4, "max diff {max_diff}");
    }

    #[test]
    fn writes_16_and_32_bit_wavs() {
        let dir = std::env::temp_dir().join(format!("daw-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let data: Vec<f32> = (0..2000).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
        for bits in [16, 24, 32] {
            let path = dir.join(format!("t{bits}.wav"));
            write_wav(&path, &data, 44_100, bits).unwrap();
            let r = hound::WavReader::open(&path).unwrap();
            assert_eq!((r.spec().channels, r.spec().sample_rate, r.spec().bits_per_sample), (2, 44_100, bits));
            assert_eq!(r.len(), 2000);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
