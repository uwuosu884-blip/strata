//! `daw` — the desktop application.

mod app;
mod arr_ops;
mod audio;
mod clip_ops;
mod midi;
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

fn main() -> ExitCode {
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
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };

    let result = eframe::run_native(
        "daw",
        native_options,
        Box::new(move |cc| {
            cc.egui_ctx.set_theme(eframe::egui::Theme::Dark);
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
