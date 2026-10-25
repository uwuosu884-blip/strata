//! Audio device handling: opens the sound card with cpal and drives the
//! engine from its real-time callback. Falls back to a "null" output (a timer
//! thread, no sound) when no device is available, so the app always runs.

use assert_no_alloc::assert_no_alloc;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use daw_engine::{Engine, EngineConfig, EngineHandle, create};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Largest internal engine block.
const ENGINE_BLOCK: usize = 512;
/// Frames converted per step when the device wants a non-f32 sample format.
const SCRATCH_FRAMES: usize = 1024;
const NULL_SAMPLE_RATE: u32 = 48_000;
const NULL_BLOCK: usize = 256;

#[derive(Debug, Clone, Default)]
pub struct AudioOptions {
    /// Host/backend name, e.g. "ALSA", "JACK", "PipeWire", "WASAPI", "ASIO".
    pub host: Option<String>,
    /// Requested buffer size in frames (None = device default).
    pub buffer_frames: Option<u32>,
    /// Skip the sound card entirely.
    pub force_null: bool,
}

#[derive(Debug, Clone)]
pub struct AudioInfo {
    pub host: String,
    pub device: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub buffer_frames: Option<u32>,
    pub sample_format: String,
    /// Some(reason) when running without a sound card.
    pub null_reason: Option<String>,
}

impl AudioInfo {
    pub fn summary(&self) -> String {
        let buffer = self.buffer_frames.map_or("default".into(), |b| {
            format!("{b} ({:.1} ms)", b as f64 * 1000.0 / self.sample_rate as f64)
        });
        match &self.null_reason {
            Some(reason) => format!("No audio output ({reason}) · engine running silently at {} Hz", self.sample_rate),
            None => format!(
                "{} · {} · {} Hz · {} ch · {} · buffer {}",
                self.host, self.device, self.sample_rate, self.channels, self.sample_format, buffer
            ),
        }
    }
}

/// Problems reported by the audio thread. Counters are atomics; the message
/// slot uses `try_lock` so the audio side can never block on it.
#[derive(Default)]
pub struct AudioHealth {
    pub stream_errors: AtomicU32,
    pub xruns: AtomicU32,
    pub realtime_denied: AtomicBool,
    pub last_error: Mutex<Option<String>>,
}

enum Backend {
    Device(#[allow(dead_code)] cpal::Stream),
    Null(NullOutput),
}

/// Owns the running audio stream. Dropping it stops audio.
pub struct AudioSystem {
    _backend: Backend,
    pub info: AudioInfo,
    pub health: Arc<AudioHealth>,
}

/// Open the audio device (or the null fallback) and start the engine.
pub fn start(opts: &AudioOptions) -> (AudioSystem, EngineHandle) {
    let health = Arc::new(AudioHealth::default());
    if !opts.force_null {
        match start_device(opts, &health) {
            Ok((backend, info, handle)) => {
                return (AudioSystem { _backend: backend, info, health }, handle);
            }
            Err(reason) => {
                eprintln!("audio: {reason}; falling back to silent null output");
                return start_null(reason, health);
            }
        }
    }
    start_null("disabled with --null-audio".into(), health)
}

fn start_null(reason: String, health: Arc<AudioHealth>) -> (AudioSystem, EngineHandle) {
    let (engine, handle) = create(EngineConfig {
        sample_rate: NULL_SAMPLE_RATE as f64,
        max_block_frames: ENGINE_BLOCK,
    });
    let info = AudioInfo {
        host: "Null".into(),
        device: "none".into(),
        sample_rate: NULL_SAMPLE_RATE,
        channels: 2,
        buffer_frames: Some(NULL_BLOCK as u32),
        sample_format: "f32".into(),
        null_reason: Some(reason),
    };
    let backend = Backend::Null(NullOutput::spawn(engine, NULL_SAMPLE_RATE, NULL_BLOCK));
    (AudioSystem { _backend: backend, info, health }, handle)
}

fn pick_host(name: Option<&str>) -> Result<cpal::Host, String> {
    let Some(name) = name else { return Ok(cpal::default_host()) };
    let available = cpal::available_hosts();
    let id = available
        .iter()
        .find(|id| id.name().eq_ignore_ascii_case(name))
        .ok_or_else(|| {
            let names: Vec<_> = available.iter().map(|h| h.name()).collect();
            format!("audio host '{name}' not available (compiled-in hosts: {})", names.join(", "))
        })?;
    cpal::host_from_id(*id).map_err(|e| format!("could not open host {}: {e}", id.name()))
}

fn device_name(device: &cpal::Device) -> String {
    device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "unknown device".into())
}

fn start_device(
    opts: &AudioOptions,
    health: &Arc<AudioHealth>,
) -> Result<(Backend, AudioInfo, EngineHandle), String> {
    let host = pick_host(opts.host.as_deref())?;
    let device = host.default_output_device().ok_or("no default output device")?;
    let supported = device.default_output_config().map_err(|e| format!("no usable output config: {e}"))?;

    let sample_format = supported.sample_format();
    let mut config: StreamConfig = supported.config();
    if let Some(frames) = opts.buffer_frames {
        config.buffer_size = cpal::BufferSize::Fixed(frames);
    }

    let (engine, handle) = create(EngineConfig {
        sample_rate: config.sample_rate as f64,
        max_block_frames: ENGINE_BLOCK,
    });

    let stream = match sample_format {
        SampleFormat::F32 => build_stream::<f32>(&device, &config, engine, health),
        SampleFormat::F64 => build_stream::<f64>(&device, &config, engine, health),
        SampleFormat::I16 => build_stream::<i16>(&device, &config, engine, health),
        SampleFormat::I32 => build_stream::<i32>(&device, &config, engine, health),
        SampleFormat::U16 => build_stream::<u16>(&device, &config, engine, health),
        SampleFormat::U8 => build_stream::<u8>(&device, &config, engine, health),
        SampleFormat::I8 => build_stream::<i8>(&device, &config, engine, health),
        other => return Err(format!("unsupported sample format {other}")),
    }?;
    stream.play().map_err(|e| format!("could not start stream: {e}"))?;

    let info = AudioInfo {
        host: host.id().name().to_string(),
        device: device_name(&device),
        sample_rate: config.sample_rate,
        channels: config.channels,
        buffer_frames: stream.buffer_size().ok().or(opts.buffer_frames),
        sample_format: sample_format.to_string(),
        null_reason: None,
    };
    Ok((Backend::Device(stream), info, handle))
}

/// Build the output stream. The callback is the real-time hot path.
fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut engine: Engine,
    health: &Arc<AudioHealth>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate as f64;
    // Allocated here, once, never inside the callback.
    let mut scratch = vec![0.0f32; SCRATCH_FRAMES * channels];

    let data_callback = move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
        assert_no_alloc(|| {
            let started = Instant::now();
            // SAFETY: flush-to-zero/denormals-are-zero is enabled only for the
            // duration of this closure and restored afterwards; our DSP does
            // not depend on denormal precision.
            unsafe {
                no_denormals::no_denormals(|| {
                    for out in data.chunks_mut(SCRATCH_FRAMES * channels) {
                        let tmp = &mut scratch[..out.len()];
                        engine.process(tmp, channels);
                        for (o, s) in out.iter_mut().zip(tmp.iter()) {
                            *o = T::from_sample(*s);
                        }
                    }
                });
            }
            let frames = data.len() / channels.max(1);
            if frames > 0 {
                let budget = frames as f64 / sample_rate;
                engine.report_cpu_load((started.elapsed().as_secs_f64() / budget) as f32);
            }
        });
    };

    let health = Arc::clone(health);
    let error_callback = move |err: cpal::Error| {
        match err.kind() {
            cpal::ErrorKind::Xrun => {
                health.xruns.fetch_add(1, Relaxed);
                return;
            }
            cpal::ErrorKind::RealtimeDenied => health.realtime_denied.store(true, Relaxed),
            _ => {
                health.stream_errors.fetch_add(1, Relaxed);
            }
        }
        // Rare path: formatting allocates, which is acceptable for errors.
        if let Ok(mut slot) = health.last_error.try_lock() {
            *slot = Some(err.to_string());
        }
    };

    device
        .build_output_stream::<T, _, _>(config.clone(), data_callback, error_callback, None)
        .map_err(|e| format!("could not open output stream: {e}"))
}

/// Silent fallback: drives the engine from a timer thread in real time, so
/// the transport, playhead and meters behave exactly as with a sound card.
struct NullOutput {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl NullOutput {
    fn spawn(mut engine: Engine, sample_rate: u32, block: usize) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("null-audio".into())
            .spawn(move || {
                let mut buffer = vec![0.0f32; block * 2];
                let period = Duration::from_secs_f64(block as f64 / sample_rate as f64);
                let mut deadline = Instant::now();
                while !stop_flag.load(Relaxed) {
                    let started = Instant::now();
                    assert_no_alloc(|| engine.process(&mut buffer, 2));
                    engine.report_cpu_load((started.elapsed().as_secs_f64() / period.as_secs_f64()) as f32);
                    deadline += period;
                    let now = Instant::now();
                    if deadline > now {
                        std::thread::sleep(deadline - now);
                    } else {
                        deadline = now; // fell behind (e.g. machine suspended): resync
                    }
                }
            })
            .expect("failed to spawn null audio thread");
        Self { stop, thread: Some(thread) }
    }
}

impl Drop for NullOutput {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Print every compiled-in host and its output devices (`--list-devices`).
pub fn list_devices() {
    for id in cpal::available_hosts() {
        println!("Host: {}", id.name());
        let Ok(host) = cpal::host_from_id(id) else {
            println!("  (unavailable)");
            continue;
        };
        let default = host.default_output_device().map(|d| device_name(&d));
        match host.output_devices() {
            Ok(devices) => {
                for d in devices {
                    let name = device_name(&d);
                    let mark = if Some(&name) == default.as_ref() { " (default)" } else { "" };
                    let cfg = d
                        .default_output_config()
                        .map(|c| format!("{} Hz, {} ch, {}", c.sample_rate(), c.channels(), c.sample_format()))
                        .unwrap_or_else(|e| format!("unusable: {e}"));
                    println!("  {name}{mark} — {cfg}");
                }
            }
            Err(e) => println!("  could not list devices: {e}"),
        }
    }
}
