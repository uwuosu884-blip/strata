//! `daw` — the desktop application.

mod audio;
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
    daw [OPTIONS]

OPTIONS:
        --host <NAME>     Audio backend: ALSA, JACK, PipeWire, PulseAudio (Linux),
                          WASAPI, ASIO (Windows), CoreAudio (macOS)
        --buffer <FRAMES> Request an audio buffer size, e.g. 128 or 256
        --null-audio      Run without a sound card (silent, for testing)
        --list-devices    Show available audio hosts and output devices
    -h, --help            Show this help";

fn parse_args() -> Result<Option<audio::AudioOptions>, String> {
    let mut opts = audio::AudioOptions::default();
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
            "--list-devices" => {
                audio::list_devices();
                return Ok(None);
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Some(opts))
}

fn main() -> ExitCode {
    let opts = match parse_args() {
        Ok(Some(opts)) => opts,
        Ok(None) => return ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("error: {msg}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let (audio, engine) = audio::start(&opts);
    println!("audio: {}", audio.info.summary());

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("daw")
            .with_app_id("daw")
            .with_inner_size([1100.0, 680.0])
            .with_min_inner_size([720.0, 400.0]),
        ..Default::default()
    };

    let result = eframe::run_native(
        "daw",
        native_options,
        Box::new(move |cc| Ok(Box::new(ui::DawApp::new(cc, engine, audio)))),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: could not start the GUI: {e}");
            ExitCode::FAILURE
        }
    }
}
