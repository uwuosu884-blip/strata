//! End-to-end: run the real binary.
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_daw-render"))
}

#[test]
fn demo_renders_valid_wavs_and_bad_args_fail() {
    let dir = std::env::temp_dir().join(format!("daw-render-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("demo.wav");
    let st = bin().args(["--demo", "--bits", "16", "-o"]).arg(&out).status().unwrap();
    assert!(st.success());
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    // 16 beats @120 + 2 s tail = 10 s, 16-bit stereo 48 kHz.
    assert_eq!(bytes.len() - 44, 10 * 48_000 * 4);

    let scene = dir.join("scene.wav");
    assert!(bin().args(["--demo", "--scene", "1", "--bars", "1", "--tail", "0", "-o"]).arg(&scene).status().unwrap().success());
    let len = std::fs::metadata(&scene).unwrap().len() as usize;
    assert!((2 * 48_000 * 8..2 * 48_000 * 8 + 100).contains(&len), "float WAV size {len}");

    assert_eq!(bin().args(["--demo"]).status().unwrap().code(), Some(2), "missing --out");
    assert_eq!(bin().args(["--demo", "--scene", "0", "-o", "x.wav"]).status().unwrap().code(), Some(2));
    assert_eq!(bin().arg("-o").arg(dir.join("y.wav")).arg(dir.join("missing.dawproj")).status().unwrap().code(), Some(1));
    std::fs::remove_dir_all(&dir).unwrap();
}
