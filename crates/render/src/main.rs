//! `daw-render`: render a project (arrangement or one scene) to WAV without
//! a sound card. Same engine code path as the live app.

use daw_core::{ExportOptions, load_project, render_project, write_wav};
use daw_project::Project;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

const USAGE: &str = "\
Render a project to WAV, faster than real time.

USAGE:
    daw-render [OPTIONS] --out <FILE.wav> [PROJECT.dawproj]

OPTIONS:
    -o, --out <FILE>        Output WAV file (required)
    -s, --scene <N>         Render session scene N (1-based) instead of the arrangement
    -b, --bars <N>          Length in bars (default: arrangement end / longest clip in scene)
        --tail <SECONDS>    Release tail after the end [default: 2]
        --metronome         Include the metronome click
        --demo              Render a built-in demo beat (no project needed)
        --write-demo <FILE> Save the demo as a project file and exit
        --sample-rate <HZ>  [default: 48000]
        --bits <16|24|32>   16/24-bit PCM (dithered) or 32-bit float [default: 32]
    -h, --help              Show this help";

struct Args {
    out: PathBuf,
    project: Option<PathBuf>,
    demo: bool,
    bits: u16,
    opts: ExportOptions,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let (mut out, mut project, mut demo, mut bits) = (None, None, false, 32u16);
    let mut opts = ExportOptions::default();
    while let Some(flag) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match flag.as_str() {
            "-o" | "--out" => out = Some(PathBuf::from(value(&flag)?)),
            "-s" | "--scene" => {
                let n: usize = value(&flag)?.parse().map_err(|_| "invalid --scene")?;
                opts.scene = Some(n.checked_sub(1).ok_or("--scene is 1-based")?);
            }
            "-b" | "--bars" => {
                let b: f64 = value(&flag)?.parse().map_err(|_| "invalid --bars")?;
                if !(b > 0.0 && b <= 10_000.0) {
                    return Err("--bars must be between 0 and 10000".into());
                }
                opts.bars = Some(b);
            }
            "--tail" => opts.tail_seconds = value(&flag)?.parse().map_err(|_| "invalid --tail")?,
            "--metronome" => opts.metronome = true,
            "--demo" => demo = true,
            "--write-demo" => {
                let path = PathBuf::from(value(&flag)?);
                let mut p = demo_project();
                p.normalize(); // give every track/clip a unique id in the saved file
                p.save(&path).map_err(|e| e.to_string())?;
                println!("Wrote {}", path.display());
                return Err(String::new() + "\u{0}");
            }
            "--sample-rate" => opts.sample_rate = value(&flag)?.parse().map_err(|_| "invalid --sample-rate")?,
            "--bits" => bits = value(&flag)?.parse().map_err(|_| "invalid --bits")?,
            "-h" | "--help" => return Err(String::new()),
            other if !other.starts_with('-') && project.is_none() => project = Some(PathBuf::from(other)),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if !(8_000..=384_000).contains(&opts.sample_rate) {
        return Err("--sample-rate must be between 8000 and 384000".into());
    }
    if ![16, 24, 32].contains(&bits) {
        return Err("--bits must be 16, 24 or 32".into());
    }
    if project.is_none() && !demo {
        return Err("give a PROJECT file or --demo".into());
    }
    Ok(Args { out: out.ok_or("--out is required")?, project, demo, bits, opts })
}

/// A short drum + bass + chords pattern on scene 1 (used by --demo and tests).
fn demo_project() -> Project {
    use daw_project::{ArrangementClip, Clip, ClipContent, Note};
    let mut p = Project::new_default();
    p.name = "Demo".into();
    let n = |start: f64, length: f64, pitch: u8, velocity: u8| Note { start, length, pitch, velocity, muted: false };
    let mut drums = Vec::new();
    for bar in 0..2 {
        let o = bar as f64 * 4.0;
        drums.extend([n(o, 0.25, 36, 120), n(o + 1.5, 0.25, 36, 100), n(o + 2.5, 0.25, 36, 110)]);
        drums.extend([n(o + 1.0, 0.25, 37, 110), n(o + 3.0, 0.25, 37, 115)]);
        drums.extend((0..8).map(|i| n(o + i as f64 * 0.5, 0.1, 39, if i % 2 == 0 { 90 } else { 60 })));
    }
    let bass: Vec<Note> = [36u8, 36, 39, 41, 36, 36, 43, 41].iter().enumerate().map(|(i, &p)| n(i as f64, 0.45, p, 105)).collect();
    let chords: Vec<Note> =
        [(0.0, [60u8, 63, 67]), (4.0, [58, 62, 65])].iter().flat_map(|(s, ch)| ch.iter().map(move |&p| n(*s, 3.5, p, 80))).collect();
    let clip = |len: f64, notes: Vec<Note>| Clip { length: len, content: ClipContent::Midi { notes }, ..Default::default() };
    for (t, db) in [(0, -5.0), (1, -6.0), (2, -10.0)] {
        p.tracks[t].gain_db = db;
    }
    p.tracks[0].slots[0] = Some(clip(8.0, drums.clone()));
    p.tracks[1].slots[0] = Some(clip(8.0, bass.clone()));
    p.tracks[2].slots[0] = Some(clip(8.0, chords.clone()));
    for (t, notes) in [(0, drums), (1, bass), (2, chords)] {
        for k in 0..2 {
            p.tracks[t]
                .arrangement
                .push(ArrangementClip { start: k as f64 * 8.0, clip: Clip { looping: false, ..clip(8.0, notes.clone()) } });
        }
    }
    p
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let project = match (&args.project, args.demo) {
        (Some(path), _) => load_project(path)?,
        (None, _) => demo_project(),
    };
    let t = Instant::now();
    let r = render_project(&project, &args.opts)?;
    let elapsed = t.elapsed().as_secs_f64();
    for w in &r.warnings {
        eprintln!("warning: {w} (rendered silent)");
    }
    write_wav(&args.out, &r.data, r.sample_rate, args.bits)?;
    let secs = r.data.len() as f64 / 2.0 / r.sample_rate as f64;
    let peak = r.peak();
    println!(
        "Rendered {:.2} s ({} beats @ {} BPM) to {} in {:.3} s ({:.0}x real time), peak {:.1} dBFS{}",
        secs,
        r.beats,
        project.tempo_bpm,
        args.out.display(),
        elapsed,
        secs / elapsed.max(1e-9),
        20.0 * peak.max(1e-9).log10(),
        if peak > 1.0 { " (CLIPPING)" } else { "" }
    );
    Ok(())
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
        Err(msg) if msg == "\u{0}" => ExitCode::SUCCESS,
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
