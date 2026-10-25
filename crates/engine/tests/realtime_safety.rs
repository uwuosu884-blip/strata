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
    let audio = Clip {
        length: 2.0,
        looping: true,
        content: ClipContent::Audio(AudioClip { sample: sample.clone(), offset_frames: 0.0, warp_bpm: Some(100.0), gain: 0.5 }),
    };
    let many_notes: Vec<Note> =
        (0..600).map(|i| Note { start: i as f64 / 150.0, length: 0.1, pitch: 30 + (i % 60) as u8, velocity: 100 }).collect();
    let midi = Clip { length: 4.0, looping: true, content: ClipContent::Midi(many_notes) };
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
            monitor: i == 3,
            instrument: instruments[i % 4].clone(),
            slots: vec![Some(if i % 4 == 3 { audio.clone() } else { midi.clone() }), None, Some(midi.clone())],
            arrangement: vec![ArrangementClip { start: 1.0, clip: midi.clone() }],
            // Exercise group routing on the RT path too.
            output: (i % 3 == 2).then(|| ((i - 1 + variant) % MAX_TRACKS) as u16),
            ..Default::default()
        })
        .collect();
    Song { tracks }
}

#[test]
fn process_is_allocation_free() {
    let (mut engine, mut handle) = create(EngineConfig { sample_rate: 44_100.0, max_block_frames: 256 });
    let mut input = handle.take_input().unwrap();
    let mut midi = handle.take_midi_input().unwrap();
    let mut rec = handle.take_recorder().unwrap();
    let mut buffer = vec![0.0f32; 4096 * 2];
    let commands = [
        Command::Play,
        Command::SetTempo(174.0),
        Command::LaunchScene { scene: 0 },
        Command::SetLoop { enabled: true, start: 0.0, end: 3.0 },
        Command::SetRecording(true),
        Command::NoteOn { track: 1, pitch: 60, velocity: 100 },
        Command::SetMidiTarget(Some(2)),
        Command::SetTrackSolo { track: 2, solo: true },
        Command::SetTrackPan { track: 1, pan: 0.3 },
        Command::LaunchClip { track: 4, scene: 2 },
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
        let _ = midi.push(MidiEvent::NoteOn { pitch: (i % 80) as u8 + 20, velocity: 90 });
        for _ in 0..512 {
            let _ = input.push(0.1);
        }
        let frames = [64, 256, 300, 1024, 4096][i % 5];
        let buf = &mut buffer[..frames * 2];
        assert_no_alloc(|| {
            engine.process(buf, 2);
            engine.report_cpu_load(0.1);
        });
        assert!(buf.iter().all(|s| s.is_finite()));
        while rec.audio.pop().is_ok() {}
        while rec.events.pop().is_ok() {}
        handle.collect_garbage();
        let _ = handle.snapshot();
    }
}
