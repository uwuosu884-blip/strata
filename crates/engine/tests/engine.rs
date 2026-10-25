//! Behavioural tests for the engine, driven exactly like an audio backend would.

use daw_engine::{Command, Engine, EngineConfig, EngineHandle, create};

const SR: f64 = 48_000.0;

fn engine() -> (Engine, EngineHandle) {
    create(EngineConfig { sample_rate: SR, max_block_frames: 512 })
}

/// Render `frames` stereo frames using callback buffers of `callback_frames`.
fn render(engine: &mut Engine, frames: usize, callback_frames: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames * 2];
    for chunk in out.chunks_mut(callback_frames * 2) {
        engine.process(chunk, 2);
    }
    out
}

fn left(interleaved: &[f32]) -> Vec<f32> {
    interleaved.iter().step_by(2).copied().collect()
}

/// Sample indices where a click starts (first non-zero sample after ≥1000 silent ones).
fn onsets(signal: &[f32]) -> Vec<usize> {
    let mut found = Vec::new();
    let mut silent_run = usize::MAX / 2;
    for (i, &s) in signal.iter().enumerate() {
        if s == 0.0 {
            silent_run += 1;
        } else {
            if silent_run >= 1000 {
                found.push(i);
            }
            silent_run = 0;
        }
    }
    found
}

fn zero_crossings(signal: &[f32]) -> usize {
    signal.windows(2).filter(|w| (w[0] <= 0.0) != (w[1] <= 0.0)).count()
}

#[test]
fn silent_until_play() {
    let (mut e, _h) = engine();
    let out = render(&mut e, 48_000, 512);
    assert!(out.iter().all(|&s| s == 0.0));
}

#[test]
fn clicks_land_exactly_on_the_beat_grid() {
    let (mut e, mut h) = engine();
    h.send(Command::Play).unwrap();
    let out = left(&render(&mut e, 4 * 24_000 + 100, 480));
    let got = onsets(&out);
    // 120 BPM @ 48 kHz → a beat every 24 000 samples. The sine starts at phase
    // 0 (a zero sample), so the first audible sample is one later.
    let expected = [1, 24_001, 48_001, 72_001, 96_001];
    assert_eq!(got.len(), expected.len(), "onsets: {got:?}");
    for (g, e) in got.iter().zip(expected) {
        assert!(g.abs_diff(e) <= 1, "click at {g}, expected {e} (all: {got:?})");
    }
}

#[test]
fn downbeat_is_higher_pitched() {
    let (mut e, mut h) = engine();
    h.send(Command::Play).unwrap();
    let out = left(&render(&mut e, 2 * 24_000, 512));
    let window = 480; // 10 ms
    let downbeat = zero_crossings(&out[1..1 + window]);
    let beat_two = zero_crossings(&out[24_001..24_001 + window]);
    // 1760 Hz vs 880 Hz → about twice the zero crossings.
    assert!(downbeat > beat_two * 3 / 2, "downbeat {downbeat}, beat two {beat_two}");
}

#[test]
fn output_is_identical_for_any_callback_size() {
    let run = |cb: usize| {
        let (mut e, mut h) = engine();
        h.send(Command::SetTempo(133.0)).unwrap();
        h.send(Command::Play).unwrap();
        render(&mut e, 100_000, cb)
    };
    let reference = run(512);
    for cb in [1, 64, 333, 1000, 4096] {
        assert_eq!(run(cb), reference, "callback size {cb} changed the output");
    }
}

#[test]
fn doubling_tempo_halves_click_spacing() {
    let (mut e, mut h) = engine();
    h.send(Command::SetTempo(240.0)).unwrap();
    h.send(Command::Play).unwrap();
    let got = onsets(&left(&render(&mut e, 48_000, 512)));
    assert_eq!(got.len(), 4, "{got:?}"); // beats at 0, 12k, 24k, 36k
    assert!(got[1].abs_diff(12_001) <= 1);
}

#[test]
fn stop_keeps_position_and_rewind_resets_it() {
    let (mut e, mut h) = engine();
    h.send(Command::Play).unwrap();
    render(&mut e, 36_000, 512); // 1.5 beats
    h.send(Command::Stop).unwrap();
    render(&mut e, 10_000, 512);
    let s = h.snapshot();
    assert!(!s.playing);
    assert!((s.beat_position - 1.5).abs() < 1e-6, "{}", s.beat_position);
    assert_eq!(s.position.to_string(), "1.2.3");

    h.send(Command::Rewind).unwrap();
    render(&mut e, 64, 64);
    assert_eq!(h.snapshot().beat_position, 0.0);
}

#[test]
fn resuming_mid_beat_waits_for_next_beat() {
    let (mut e, mut h) = engine();
    h.send(Command::Play).unwrap();
    render(&mut e, 36_000, 512); // stop at beat 1.5
    h.send(Command::Stop).unwrap();
    render(&mut e, 48_000, 512); // let the click ring out
    h.send(Command::Play).unwrap();
    let got = onsets(&left(&render(&mut e, 24_000, 512)));
    // Next beat (2.0) is 0.5 beats = 12 000 samples away.
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].abs_diff(12_001) <= 1, "{got:?}");
}

#[test]
fn metronome_off_and_zero_gain_are_silent() {
    let (mut e, mut h) = engine();
    h.send(Command::SetMetronome(false)).unwrap();
    h.send(Command::Play).unwrap();
    assert!(render(&mut e, 48_000, 512).iter().all(|&s| s == 0.0));

    let (mut e, mut h) = engine();
    h.send(Command::SetMasterGain(0.0)).unwrap();
    h.send(Command::Play).unwrap();
    let out = render(&mut e, 48_000, 512);
    // The gain glides to zero (≈14 time constants of 20 ms ≈ 280 ms), so only the
    // first click may be faintly audible; beats at 24 000 and 48 000 must be silent.
    assert!(out[2 * 12_000..].iter().all(|&s| s == 0.0));
    assert!(out.iter().all(|&s| s.abs() < 0.5));
}

#[test]
fn snapshot_reports_peaks_and_resets_them() {
    let (mut e, mut h) = engine();
    h.send(Command::Play).unwrap();
    render(&mut e, 2_048, 512);
    let s = h.snapshot();
    assert!(s.playing);
    assert!(s.peak[0] > 0.3 && s.peak[0] <= 0.5, "peak {}", s.peak[0]);
    assert_eq!(h.snapshot().peak, [0.0, 0.0], "peaks must reset after reading");
}

#[test]
fn mono_and_surround_layouts() {
    for channels in [1usize, 2, 6] {
        let (mut e, mut h) = engine();
        h.send(Command::Play).unwrap();
        let mut out = vec![0.0f32; 1024 * channels];
        e.process(&mut out, channels);
        assert!(out.iter().any(|&s| s != 0.0));
        if channels == 6 {
            for frame in out.chunks_exact(6) {
                assert!(frame[2..].iter().all(|&s| s == 0.0));
            }
        }
    }
}

#[test]
fn full_command_queue_returns_command() {
    let (_e, mut h) = engine();
    let mut sent = 0;
    while h.send(Command::Play).is_ok() {
        sent += 1;
        assert!(sent <= 100_000, "queue never filled");
    }
    assert_eq!(h.send(Command::Stop), Err(Command::Stop));
}
