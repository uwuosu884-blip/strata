//! Proves `Engine::process` never allocates or frees memory.
//!
//! The custom global allocator aborts the test process if any allocation
//! happens inside `assert_no_alloc(..)`. Only active in debug builds
//! (`assert_no_alloc` is a no-op in release by design).

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use daw_engine::{Command, EngineConfig, create};

#[cfg(debug_assertions)]
#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn process_is_allocation_free() {
    let (mut engine, mut handle) = create(EngineConfig { sample_rate: 44_100.0, max_block_frames: 256 });
    let mut buffer = vec![0.0f32; 4096 * 2];
    let commands = [
        Command::Play,
        Command::SetTempo(174.0),
        Command::SetMasterGain(0.5),
        Command::SetMetronome(false),
        Command::SetMetronome(true),
        Command::SetBeatsPerBar(7),
        Command::Stop,
        Command::Rewind,
        Command::Play,
    ];

    for (i, cmd) in commands.iter().cycle().take(200).enumerate() {
        handle.send(*cmd).unwrap();
        // Vary the callback size, including sizes larger than max_block_frames.
        let frames = [64, 256, 300, 1024, 4096][i % 5];
        let buf = &mut buffer[..frames * 2];
        assert_no_alloc(|| {
            engine.process(buf, 2);
            engine.report_cpu_load(0.1);
        });
        let _ = handle.snapshot();
    }
}
