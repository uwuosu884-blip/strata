//! Channel strip, audio clips and built-in instruments.

mod common;
use common::*;
use daw_engine::*;
use std::sync::Arc;

fn dc_clip(level: f32) -> Clip {
    let s = Arc::new(SampleBuffer::mono("dc", SR as u32, vec![level; SR as usize * 4]));
    Clip { length: 4.0, looping: true, content: ClipContent::Audio(AudioClip { sample: s, offset_frames: 0.0, warp_bpm: None, gain: 1.0 }) }
}

fn audio_track(rt: u16, clip: Clip) -> SongTrack {
    SongTrack { rt, slots: vec![Some(clip)], ..Default::default() }
}

#[test]
fn pan_law_and_unity_centre() {
    for (pan, expect_l, expect_r) in [(0.0f32, 0.5f32, 0.5f32), (-1.0, 0.5 * std::f32::consts::SQRT_2, 0.0), (1.0, 0.0, 0.707)] {
        let (mut e, mut h) = engine();
        let mut t = audio_track(0, dc_clip(0.5));
        t.pan = pan;
        h.set_song(Song { tracks: vec![t] }).unwrap();
        h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
        let out = render(&mut e, 4800, 480);
        let (l, r) = (left(&out)[4000], right(&out)[4000]);
        assert!((l - expect_l).abs() < 1e-3 && (r - expect_r).abs() < 1e-3, "pan {pan}: {l} {r}");
    }
}

#[test]
fn mute_solo_and_gain() {
    let (mut e, mut h) = engine();
    let a = audio_track(0, dc_clip(0.25));
    let b = audio_track(1, dc_clip(0.125));
    h.set_song(Song { tracks: vec![a, b] }).unwrap();
    h.send(Command::LaunchScene { scene: 0 }).unwrap();
    let level = |e: &mut Engine| *left(&render(e, 4800, 480)).last().unwrap();
    assert!((level(&mut e) - 0.375).abs() < 1e-4);
    h.send(Command::SetTrackSolo { track: 1, solo: true }).unwrap();
    assert!((level(&mut e) - 0.125).abs() < 1e-4, "solo b");
    h.send(Command::SetTrackMute { track: 1, mute: true }).unwrap();
    assert!(level(&mut e).abs() < 1e-4, "solo b muted → silence");
    send_all(
        &mut h,
        &[
            Command::SetTrackSolo { track: 1, solo: false },
            Command::SetTrackMute { track: 1, mute: false },
            Command::SetTrackGain { track: 0, gain: 2.0 },
        ],
    );
    assert!((level(&mut e) - 0.625).abs() < 1e-4, "gain 2× on a");
    let s = h.track(0);
    assert!(s.peak[0] > 0.49, "track meter {:?}", s.peak);
}

#[test]
fn audio_clip_follows_source_and_warp_repitches() {
    // Ramp sample: value = frame index / 1e6 (exactly representable steps).
    let data: Vec<f32> = (0..SR as usize * 4).map(|i| i as f32 / 1.0e6).collect();
    let s = Arc::new(SampleBuffer::mono("ramp", SR as u32, data));
    for (warp, speed) in [(None, 1.0), (Some(60.0), 2.0)] {
        let (mut e, mut h) = engine();
        let clip = Clip {
            length: 4.0,
            looping: true,
            content: ClipContent::Audio(AudioClip { sample: s.clone(), offset_frames: 100.0, warp_bpm: warp, gain: 1.0 }),
        };
        h.set_song(Song { tracks: vec![audio_track(0, clip)] }).unwrap();
        h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
        let out = left(&render(&mut e, 2000, 512));
        let expected = (100.0 + 1000.0 * speed) / 1.0e6;
        assert!((out[1000] - expected as f32).abs() < 1e-6, "warp {warp:?}: {} vs {expected}", out[1000]);
        assert_eq!(out[0], 0.0, "declick fade starts at zero");
    }
}

#[test]
fn synth_plays_correct_pitch_and_releases() {
    for wave in [Wave::Sine, Wave::Saw, Wave::Square, Wave::Triangle] {
        let (mut e, mut h) = engine();
        let p = SynthParams {
            osc1: wave,
            osc2: wave,
            osc2_detune_cents: 0.0,
            cutoff_hz: 18_000.0,
            filter_env_octaves: 0.0,
            ..Default::default()
        };
        h.set_song(Song { tracks: vec![SongTrack { rt: 0, instrument: Instrument::Synth(p), ..Default::default() }] }).unwrap();
        h.send(Command::NoteOn { track: 0, pitch: 69, velocity: 100 }).unwrap();
        let out = left(&render(&mut e, 24_000, 512));
        let f = freq(&out[4800..], SR);
        assert!((f - 440.0).abs() < 2.0, "{wave:?}: {f} Hz");
        assert!(out.iter().all(|s| s.is_finite() && s.abs() < 1.0));
        h.send(Command::NoteOff { track: 0, pitch: 69 }).unwrap();
        let tail = left(&render(&mut e, 48_000, 512));
        assert!(tail[40_000..].iter().all(|&s| s == 0.0), "{wave:?} released");
        assert_eq!(h.track(0).voices, 0);
    }
}

#[test]
fn synth_presets_are_stable_with_full_polyphony() {
    for name in SynthParams::PRESETS {
        let (mut e, mut h) = engine();
        let instrument = Instrument::Synth(SynthParams::preset(name));
        h.set_song(Song { tracks: vec![SongTrack { rt: 0, instrument, ..Default::default() }] }).unwrap();
        for p in 30..70 {
            h.send(Command::NoteOn { track: 0, pitch: p, velocity: 127 }).unwrap(); // 40 notes > 16 voices
        }
        let out = render(&mut e, 48_000, 512);
        assert!(out.iter().all(|s| s.is_finite()), "{name}");
        assert!(out.iter().any(|s| s.abs() > 0.01), "{name} audible");
        assert_eq!(h.track(0).voices, 16, "{name}: voice stealing caps polyphony");
    }
}

#[test]
fn clip_notes_end_after_their_length() {
    let (mut e, mut h) = engine();
    let p = SynthParams { amp: AdsrParams { attack: 0.001, decay: 0.1, sustain: 1.0, release: 0.01 }, ..Default::default() };
    let clip = Clip {
        length: 4.0,
        looping: true,
        content: ClipContent::Midi(vec![Note { start: 0.0, length: 1.0, pitch: 60, velocity: 100, muted: false }]),
    };
    h.set_song(Song { tracks: vec![SongTrack { rt: 0, instrument: Instrument::Synth(p), slots: vec![Some(clip)], ..Default::default() }] })
        .unwrap();
    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
    let out = left(&render(&mut e, SPB * 2, 512));
    assert!(out[SPB / 2..SPB / 2 + 500].iter().any(|s| s.abs() > 0.01), "sounding during the note");
    assert!(out[SPB + 2400..].iter().all(|&s| s == 0.0), "silent after note + release");
}

#[test]
fn sampler_transposes_relative_to_root() {
    let sine: Vec<f32> = (0..SR as usize).map(|i| (i as f32 * 100.0 * std::f32::consts::TAU / SR as f32).sin()).collect();
    let s = std::sync::Arc::new(SampleBuffer::mono("sine", SR as u32, sine));
    let (mut e, mut h) = engine();
    let instrument = Instrument::Sampler { sample: Some(s), params: SamplerParams { root_note: 60, ..Default::default() } };
    h.set_song(Song { tracks: vec![SongTrack { rt: 0, instrument, ..Default::default() }] }).unwrap();
    h.send(Command::NoteOn { track: 0, pitch: 72, velocity: 127 }).unwrap();
    let out = left(&render(&mut e, 12_000, 512));
    let f = freq(&out[1000..], SR);
    assert!((f - 200.0).abs() < 1.0, "octave up: {f}");
}

#[test]
fn drum_rack_pads_and_choke_groups() {
    let long = std::sync::Arc::new(SampleBuffer::mono("long", SR as u32, vec![0.5; SR as usize]));
    let pad = |choke| DrumPad { name: "p".into(), sample: Some(long.clone()), gain: 1.0, tune_semitones: 0.0, choke };
    let (mut e, mut h) = engine();
    let instrument = Instrument::Drums(vec![pad(0), pad(1), pad(1)]);
    h.set_song(Song { tracks: vec![SongTrack { rt: 0, instrument, ..Default::default() }] }).unwrap();
    send_all(
        &mut h,
        &[
            Command::NoteOn { track: 0, pitch: DRUM_BASE_NOTE, velocity: 127 },
            Command::NoteOn { track: 0, pitch: DRUM_BASE_NOTE + 1, velocity: 127 },
        ],
    );
    render(&mut e, 4800, 480);
    assert_eq!(h.track(0).voices, 2);
    h.send(Command::NoteOn { track: 0, pitch: DRUM_BASE_NOTE + 2, velocity: 127 }).unwrap(); // chokes pad 1
    render(&mut e, 4800, 480);
    assert_eq!(h.track(0).voices, 2, "pad 1 choked by pad 2");
    h.send(Command::NoteOff { track: 0, pitch: DRUM_BASE_NOTE }).unwrap();
    render(&mut e, 480, 480);
    assert_eq!(h.track(0).voices, 2, "drum hits are one-shot (ignore note-off)");
    h.send(Command::NoteOn { track: 0, pitch: 20, velocity: 127 }).unwrap(); // no pad there
    render(&mut e, 480, 480);
    assert_eq!(h.track(0).voices, 2);
}

#[test]
fn hardware_midi_goes_to_midi_target() {
    let (mut e, mut h) = engine();
    let mut midi = h.take_midi_input().unwrap();
    h.set_song(Song { tracks: vec![track(3, vec![])] }).unwrap();
    midi.push(MidiEvent::NoteOn { pitch: 60, velocity: 127 }).unwrap();
    assert!(nonzero(&render(&mut e, 512, 512)).is_empty(), "no target yet");
    h.send(Command::SetMidiTarget(Some(3))).unwrap();
    midi.push(MidiEvent::NoteOn { pitch: 60, velocity: 127 }).unwrap();
    assert_eq!(nonzero(&left(&render(&mut e, 512, 512))), vec![0]);
}

#[test]
fn group_bus_sums_children_and_solo_propagates() {
    let (mut e, mut h) = engine();
    let g = SongTrack { rt: 5, ..Default::default() };
    let a = SongTrack { output: Some(5), ..audio_track(0, dc_clip(0.25)) };
    let b = SongTrack { output: Some(5), ..audio_track(1, dc_clip(0.125)) };
    let c = audio_track(2, dc_clip(0.0625)); // outside the group
    h.set_song(Song { tracks: vec![g, a, b, c] }).unwrap();
    h.send(Command::LaunchScene { scene: 0 }).unwrap();
    let level = |e: &mut Engine| *left(&render(e, 4800, 480)).last().unwrap();
    assert!((level(&mut e) - 0.4375).abs() < 1e-4, "all to master");
    h.send(Command::SetTrackGain { track: 5, gain: 2.0 }).unwrap();
    assert!((level(&mut e) - (0.75 + 0.0625)).abs() < 1e-4, "group gain scales children");
    assert!(h.track(5).peak[0] > 0.74, "group meter");
    h.send(Command::SetTrackMute { track: 5, mute: true }).unwrap();
    assert!((level(&mut e) - 0.0625).abs() < 1e-4, "group mute");
    send_all(&mut h, &[Command::SetTrackMute { track: 5, mute: false }, Command::SetTrackSolo { track: 1, solo: true }]);
    assert!((level(&mut e) - 0.25).abs() < 1e-4, "solo child keeps its group audible, siblings/others silent");
    send_all(&mut h, &[Command::SetTrackSolo { track: 1, solo: false }, Command::SetTrackSolo { track: 5, solo: true }]);
    assert!((level(&mut e) - 0.75).abs() < 1e-4, "solo group keeps children");
}

#[test]
fn group_output_must_precede_child() {
    // Child listed before its "group" → routed to master instead (no cycle / lost audio).
    let mut s = Song {
        tracks: vec![
            SongTrack { output: Some(1), ..audio_track(0, dc_clip(0.1)) },
            SongTrack { rt: 1, output: Some(0), ..Default::default() },
        ],
    };
    s.prepare();
    assert_eq!(s.tracks[0].output, None);
    assert_eq!(s.tracks[1].output, Some(0));
}
