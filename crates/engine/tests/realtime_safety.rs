//! Proves `Engine::process` never allocates or frees memory, across every
//! feature: song swaps, launches, all instruments, audio clips, recording,
//! monitoring, MIDI input, loop jumps. The custom allocator aborts the test
//! process on any allocation inside `assert_no_alloc` (debug builds).

#[cfg(debug_assertions)]
use assert_no_alloc::AllocDisabler;
use assert_no_alloc::assert_no_alloc;
use daw_engine::*;
use std::sync::Arc;

#[cfg(debug_assertions)]
#[global_allocator]
static A: AllocDisabler = AllocDisabler;

fn big_song(variant: usize) -> Song {
    let sample = Arc::new(SampleBuffer::mono("s", 44_100, (0..44_100).map(|i| (i as f32 * 0.01).sin()).collect()));
    // Every warp mode (incl. the phase vocoder), envelopes, loop markers.
    let mode =
        [StretchMode::RePitch, StretchMode::Beats, StretchMode::Tones, StretchMode::Texture, StretchMode::Complex, StretchMode::ComplexPro]
            [variant % 6];
    let warp = WarpSetup {
        map: WarpMap::from_markers(&[(0.0, 0.0), (1.0, 20_000.0), (2.0, 44_100.0)], 0.0, 26_460.0),
        mode,
        params: StretchParams {
            flux: 0.5,
            fade: 0.3,
            loop_mode: [LoopMode::Off, LoopMode::Forward, LoopMode::BackAndForth][variant % 3],
            ..Default::default()
        },
        segs: vec![0.0, 0.5, 1.0, 1.5],
        transpose: 3.0,
        gain_env: vec![(0.0, 1.0), (2.0, 0.2)],
        transpose_env: vec![(0.0, 0.0), (1.0, -5.0)],
        fade_in: 0.25,
        fade_out: 0.5,
        fade_in_curve: 0.4,
        ..Default::default()
    };
    let mut audio = Clip::new(
        2.0,
        true,
        ClipContent::Audio(AudioClip { sample: sample.clone(), offset_frames: 0.0, warp_bpm: Some(100.0), gain: 0.5, warp }),
    );
    audio.start = 0.25;
    audio.loop_start = 0.5;
    audio.loop_end = 1.75;
    let many_notes: Vec<Note> = (0..600)
        .map(|i| Note {
            start: i as f64 / 150.0,
            length: 0.1,
            pitch: 30 + (i % 60) as u8,
            velocity: 100,
            muted: false,
            mpe: (i % 3) as u32,
            vel_dev: (i % 5) as i8 * 4,
        })
        .collect();
    let mut midi = Clip::new(4.0, true, ClipContent::Midi(many_notes));
    midi.start = 0.5;
    midi.loop_start = 1.0;
    midi.loop_end = 3.0;
    let curve: Vec<(f64, f32)> = (0..80).map(|k| (k as f64 * 0.01, (k as f32 * 0.3).sin())).collect();
    midi.extra.mpe = vec![NoteMpe { pitch: curve.clone(), slide: curve.clone(), pressure: curve }; 2];
    midi.extra.vol_env = vec![(0.0, 0.5), (4.0, 1.0)];
    midi.extra.bend_env = vec![(0.0, -1.0), (4.0, 1.0)];
    midi.extra.launch = Launch {
        mode: [LaunchMode::Trigger, LaunchMode::Gate, LaunchMode::Toggle, LaunchMode::Repeat][variant % 4],
        quantize: Some(0.5),
        legato: variant % 2 == 1,
        velocity: 0.5,
        follow: Some((Follow::Next, Follow::Any, 0.5, 1.0)),
    };
    midi.extra.program = [Some(1), Some(2), Some(3)];
    let pads = (0..16)
        .map(|i| DrumPad { name: "p".into(), sample: Some(sample.clone()), gain: 0.5, tune_semitones: 0.0, choke: (i % 3) as u8 })
        .collect();
    let instruments = [
        Instrument::Synth(SynthParams::preset("Pad")),
        Instrument::Sampler { sample: Some(sample.clone()), params: SamplerParams::default() },
        Instrument::Drums(pads),
        Instrument::None,
    ];
    let tracks = (0..12)
        .map(|i| SongTrack {
            rt: ((i + variant) % MAX_TRACKS) as u16,
            pan: (i as f32 / 6.0) - 1.0,
            monitor: [Monitor::In, Monitor::Auto, Monitor::Off][i % 3],
            arm: i % 2 == 0,
            input: match i % 6 {
                0 => TrackInput::Ext { ch: 1, stereo: i % 4 == 0 },
                1 => TrackInput::Midi { port: MIDI_ALL_INS, channel: 0 },
                2 => TrackInput::Resampling,
                3 => TrackInput::Track {
                    rt: ((i + 1 + variant) % MAX_TRACKS) as u16,
                    tap: [Tap::PreFx, Tap::PostMixer, Tap::Pad(2)][variant % 3],
                },
                4 => TrackInput::MidiTrack { rt: ((i + 2 + variant) % MAX_TRACKS) as u16 },
                _ => TrackInput::None,
            },
            instrument: instruments[i % 4].clone(),
            slots: vec![Some(if i % 4 == 3 { audio.clone() } else { midi.clone() }), None, Some(midi.clone())],
            arrangement: vec![ArrangementClip { start: 1.0, clip: midi.clone() }],
            // Exercise group routing on the RT path too.
            output: match i % 5 {
                0 => TrackOutput::Track(((i + 1 + variant) % MAX_TRACKS) as u16),
                1 => TrackOutput::Ext { ch: 2, stereo: i % 2 == 0 },
                2 => TrackOutput::MidiPort { port: 0, channel: 3 },
                3 => TrackOutput::MidiTrack { rt: ((i + 3 + variant) % MAX_TRACKS) as u16, instrument: variant.is_multiple_of(2) },
                _ => TrackOutput::Master,
            },
            ..Default::default()
        })
        .collect();
    let mut song = Song::new(tracks);
    song.tempo_map = vec![(0.5, 140.0), (1.5, 90.0), (3.0, 128.0)];
    song.base_tempo = 120.0;
    song
}

#[test]
fn process_is_allocation_free() {
    let (mut engine, mut handle) = create(EngineConfig { sample_rate: 44_100.0, max_block_frames: 256, input_channels: 4 });
    let mut input = handle.take_input().unwrap();
    let mut midi = handle.take_midi_input().unwrap();
    let mut rec = handle.take_recorder().unwrap();
    let mut midi_out = handle.take_midi_output().unwrap();
    let mut buffer = vec![0.0f32; 4096 * 4];
    let commands = [
        Command::Play,
        Command::SetTempo(174.0),
        Command::LaunchScene { scene: 0 },
        Command::SetLoop { enabled: true, start: 0.0, end: 3.0 },
        Command::SetRecording(true),
        Command::NoteOn { track: 1, pitch: 60, velocity: 100 },
        Command::SetAutoArm(2),
        Command::KeyboardNote { pitch: 64, velocity: 80 },
        Command::KeyboardNote { pitch: 64, velocity: 0 },
        Command::SetMasterPan(0.3),
        Command::SetTrackSolo { track: 2, solo: true },
        Command::SetTrackPan { track: 1, pan: 0.3 },
        Command::LaunchClip { track: 4, scene: 2 },
        Command::ReleaseClip { track: 4, scene: 2 },
        Command::Nudge(1.08),
        Command::SetTempoFollow(false),
        Command::LaunchClip { track: 7, scene: 0 },
        Command::Nudge(1.0),
        Command::SetTempoFollow(true),
        Command::NoteOff { track: 1, pitch: 60 },
        Command::SetLaunchQuantize(0.25),
        Command::SetTrackSolo { track: 2, solo: false },
        Command::StopTrack { track: 5 },
        Command::SetRecording(false),
        Command::BackToArrangement,
        Command::AllNotesOff { track: 1 },
        Command::SetPosition(1.5),
        Command::StopAllClips,
        Command::Stop,
        Command::Rewind,
    ];

    for (i, cmd) in commands.iter().cycle().take(400).enumerate() {
        handle.send(*cmd).unwrap();
        if i % 37 == 0 {
            handle.set_song(big_song(i)).unwrap(); // allocation here is fine (control thread)
        }
        let _ = midi.push(MidiInput { port: 0, channel: 0, event: MidiEvent::NoteOn { pitch: (i % 80) as u8 + 20, velocity: 90 } });
        for _ in 0..512 {
            let _ = input.push(0.1);
        }
        let frames = [64, 256, 300, 1024, 4096][i % 5];
        let channels = 2 + (i % 3);
        let buf = &mut buffer[..frames * channels];
        assert_no_alloc(|| {
            engine.process(buf, channels);
            engine.report_cpu_load(0.1);
        });
        assert!(buf.iter().all(|s| s.is_finite()));
        while rec.audio.pop().is_ok() {}
        while rec.events.pop().is_ok() {}
        while midi_out.pop().is_ok() {}
        handle.collect_garbage();
        let _ = handle.snapshot();
    }
}
