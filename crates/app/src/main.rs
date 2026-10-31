//! `daw` — the desktop application.

mod app;
mod arr_ops;
mod audio;
mod clip_ops;
mod edit_ops;
mod midi;
mod pinch;
mod settings;
mod ui;

use std::process::ExitCode;

// Debug builds: abort if anything allocates inside `assert_no_alloc` (the
// audio callback). Release builds use the normal system allocator.
#[cfg(debug_assertions)]
#[global_allocator]
static A: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

const USAGE: &str = "\
daw — a Rust DAW

USAGE:
    daw [OPTIONS] [PROJECT.dawproj]

OPTIONS:
        --host <NAME>     Audio backend: ALSA, JACK, PipeWire, PulseAudio (Linux),
                          WASAPI, ASIO (Windows), CoreAudio (macOS)
        --buffer <FRAMES> Request an audio buffer size, e.g. 128 or 256
        --sample-rate <HZ> Request a sample rate, e.g. 44100 or 48000
        --output-device <NAME>  Audio output device (see --list-devices)
        --input-device <NAME>   Audio input device
        --null-audio      Run without a sound card (silent, for testing)
        --no-input        Don't open an audio input (no audio recording)
        --no-midi         Don't open MIDI ports
Device / buffer choices made in Settings (Ctrl+,) are saved; options here override them.
        --list-devices    Show available audio hosts and output devices
    -h, --help            Show this help";

struct Args {
    audio: audio::AudioOptions,
    midi: bool,
    project: Option<std::path::PathBuf>,
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut opts = audio::AudioOptions::default();
    let (mut midi, mut project) = (true, None);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--host" => opts.host = Some(args.next().ok_or("--host needs a name")?),
            "--sample-rate" => {
                let v = args.next().ok_or("--sample-rate needs a number")?;
                let hz: u32 = v.parse().map_err(|_| format!("invalid sample rate: {v}"))?;
                if !(8_000..=384_000).contains(&hz) {
                    return Err("--sample-rate must be between 8000 and 384000".into());
                }
                opts.sample_rate = Some(hz);
            }
            "--buffer" => {
                let v = args.next().ok_or("--buffer needs a number")?;
                let frames: u32 = v.parse().map_err(|_| format!("invalid buffer size: {v}"))?;
                if !(16..=8192).contains(&frames) {
                    return Err("--buffer must be between 16 and 8192".into());
                }
                opts.buffer_frames = Some(frames);
            }
            "--null-audio" => opts.force_null = true,
            "--no-input" => opts.no_input = true,
            "--output-device" => opts.output_device = Some(args.next().ok_or("--output-device needs a name")?),
            "--input-device" => opts.input_device = Some(args.next().ok_or("--input-device needs a name")?),
            "--no-midi" => midi = false,
            "--list-devices" => {
                audio::list_devices();
                return Ok(None);
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            other if !other.starts_with('-') && project.is_none() => project = Some(std::path::PathBuf::from(other)),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Some(Args { audio: opts, midi, project }))
}

/// Panics (any thread) are appended to `crash.log` next to the settings
/// with a backtrace, so random crashes can be reported and found.
fn install_crash_log() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(path) = settings::Settings::path().map(|p| p.with_file_name("crash.log")) {
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
            let thread = std::thread::current().name().unwrap_or("?").to_string();
            let when = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let bt = std::backtrace::Backtrace::force_capture();
            let msg = format!("--- daw {} crash (unix {when}) in thread '{thread}'\n{info}\n{bt}\n", env!("CARGO_PKG_VERSION"));
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                let _ = f.write_all(msg.as_bytes());
            }
            eprintln!("crash details written to {}", path.display());
        }
        default(info);
    }));
}

fn main() -> ExitCode {
    install_crash_log();
    let args = match parse_args() {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("error: {msg}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    // Load before opening audio so a bad path fails fast.
    let (project, path) = match &args.project {
        Some(p) => match daw_core::load_project(p) {
            Ok(proj) => (proj, Some(p.clone())),
            Err(e) => {
                eprintln!("error: {}: {e}", p.display());
                return ExitCode::FAILURE;
            }
        },
        None => (daw_project::Project::new_default(), None),
    };
    // Saved preferences fill in what the command line leaves open.
    let settings = settings::Settings::load();
    let base = args.audio.clone();
    let opts = audio::AudioOptions {
        host: args.audio.host.clone().or(settings.host.clone()),
        buffer_frames: args.audio.buffer_frames.or(settings.buffer_frames),
        sample_rate: args.audio.sample_rate.or(settings.sample_rate),
        no_input: args.audio.no_input || settings.no_input,
        output_device: args.audio.output_device.clone().or(settings.output_device.clone()),
        input_device: args.audio.input_device.clone().or(settings.input_device.clone()),
        ..args.audio.clone()
    };
    let (audio, engine) = audio::start(&opts);
    println!("audio: {}", audio.info.summary());

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("daw")
            .with_app_id("daw")
            .with_inner_size([1920.0, 1080.0])
            .with_maximized(true)
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };

    let result = eframe::run_native(
        "daw",
        native_options,
        Box::new(move |cc| {
            cc.egui_ctx.set_theme(eframe::egui::Theme::Dark);
            ui::theme::install_fonts(&cc.egui_ctx);
            pinch::start(cc);
            Ok(Box::new(app::DawApp::new(engine, audio, project, path, args.midi, settings, base)))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: could not start the GUI: {e}");
            ExitCode::FAILURE
        }
    }
}
