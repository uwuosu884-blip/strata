//! `daw-render` — render a project to a WAV file without a sound card.
//!
//! Uses exactly the same `Engine::process` as the live app, so what you hear
//! in a render is what the app plays.

use assert_no_alloc::assert_no_alloc;
use daw_engine::{Command, EngineConfig, create};
use daw_project::Project;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

#[cfg(debug_assertions)]
#[global_allocator]
static A: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

const USAGE: &str = "\
Render a project to WAV, faster than real time.

USAGE:
    daw-render [OPTIONS] --out <FILE.wav>

OPTIONS:
    -o, --out <FILE>        Output WAV file (required)
    -p, --project <FILE>    Project file to render (default: a new project)
    -b, --bars <N>          Number of bars to render [default: 4]
    -t, --tempo <BPM>       Override the project tempo
        --sample-rate <HZ>  [default: 48000]
        --bits <16|32>      16-bit integer or 32-bit float [default: 32]
    -h, --help              Show this help";

struct Args {
    out: PathBuf,
    project: Option<PathBuf>,
    bars: u32,
    tempo: Option<f64>,
    sample_rate: u32,
    bits: u16,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let (mut out, mut project, mut tempo) = (None, None, None);
    let (mut bars, mut sample_rate, mut bits) = (4u32, 48_000u32, 32u16);

    while let Some(flag) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match flag.as_str() {
            "-o" | "--out" => out = Some(PathBuf::from(value(&flag)?)),
            "-p" | "--project" => project = Some(PathBuf::from(value(&flag)?)),
            "-b" | "--bars" => bars = value(&flag)?.parse().map_err(|_| "invalid --bars")?,
            "-t" | "--tempo" => tempo = Some(value(&flag)?.parse().map_err(|_| "invalid --tempo")?),
            "--sample-rate" => sample_rate = value(&flag)?.parse().map_err(|_| "invalid --sample-rate")?,
            "--bits" => bits = value(&flag)?.parse().map_err(|_| "invalid --bits")?,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if !(8_000..=384_000).contains(&sample_rate) {
        return Err("--sample-rate must be between 8000 and 384000".into());
    }
    if bits != 16 && bits != 32 {
        return Err("--bits must be 16 or 32".into());
    }
    if bars == 0 || bars > 10_000 {
        return Err("--bars must be between 1 and 10000".into());
    }
    let out = out.ok_or("--out is required")?;
    Ok(Args { out, project, bars, tempo, sample_rate, bits })
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let project = match &args.project {
        Some(path) => Project::load(path)?,
        None => Project::default(),
    };
    let tempo = args.tempo.unwrap_or(project.tempo_bpm);

    let block = 512;
    let (mut engine, mut handle) =
        create(EngineConfig { sample_rate: args.sample_rate as f64, max_block_frames: block });
    for cmd in [
        Command::SetTempo(tempo),
        Command::SetBeatsPerBar(project.beats_per_bar),
        Command::SetMetronome(project.metronome),
        Command::SetMasterGain(project.master_gain_linear()),
        Command::Play,
    ] {
        handle.send(cmd).map_err(|c| format!("command queue full ({c:?})"))?;
    }
    let tempo = apply_commands_and_read_tempo(&mut engine, tempo);

    let seconds = args.bars as f64 * project.beats_per_bar as f64 * 60.0 / tempo;
    let total_frames = (seconds * args.sample_rate as f64).round() as usize;

    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: args.sample_rate,
        bits_per_sample: args.bits,
        sample_format: if args.bits == 32 { hound::SampleFormat::Float } else { hound::SampleFormat::Int },
    };
    if let Some(dir) = args.out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut writer = hound::WavWriter::create(&args.out, spec)?;

    let started = Instant::now();
    let mut buffer = vec![0.0f32; block * 2];
    let mut written = 0;
    while written < total_frames {
        let frames = block.min(total_frames - written);
        let buf = &mut buffer[..frames * 2];
        assert_no_alloc(|| engine.process(buf, 2));
        for &s in buf.iter() {
            if args.bits == 32 {
                writer.write_sample(s)?;
            } else {
                writer.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16)?;
            }
        }
        written += frames;
    }
    writer.finalize()?;

    let elapsed = started.elapsed().as_secs_f64();
    println!(
        "Rendered {} bars at {:.1} BPM → {} ({:.2} s audio, {}-bit, {} Hz) in {:.3} s ({:.0}× real time)",
        args.bars,
        tempo,
        args.out.display(),
        seconds,
        args.bits,
        args.sample_rate,
        elapsed,
        seconds / elapsed.max(1e-9)
    );
    Ok(())
}

/// Process zero frames so queued commands are applied, then read back the
/// tempo the engine actually uses (it clamps out-of-range values).
fn apply_commands_and_read_tempo(engine: &mut daw_engine::Engine, requested: f64) -> f64 {
    let mut empty: [f32; 0] = [];
    engine.process(&mut empty, 2);
    let actual = engine.transport().tempo();
    if (actual - requested).abs() > f64::EPSILON {
        eprintln!("note: tempo {requested} clamped to {actual}");
    }
    actual
}

fn main() -> ExitCode {
    match parse_args() {
        Ok(args) => match run(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        Err(msg) if msg.is_empty() => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Err(msg) => {
            eprintln!("error: {msg}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
