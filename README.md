# daw

An Ableton Live–style digital audio workstation written in Rust.
Targets **Linux (priority), Windows and macOS**.

> Status: **Phase 0**: workspace skeleton, real-time engine with transport and
> metronome, desktop app with transport bar, offline WAV renderer, CI for all
> three operating systems.

## Layout

```
crates/
  engine/   real-time audio engine (no GUI code). Engine::process is RT-safe:
            no locks, no allocation, no I/O. Talks to the GUI through a
            lock-free command queue (rtrb) and atomics.
  project/  the document model, saved as human-readable RON (.dawproj)
  render/   `daw-render`: renders a project to WAV faster than real time
  app/      `daw`: the desktop app (egui/eframe GUI + cpal audio device)
```

## Building

Needs Rust **1.95+** (`rustup update`).

### Linux (Debian/Ubuntu)
```bash
sudo bash scripts/linux-deps.sh          # ALSA + GUI libraries
cargo run --release -p daw-app           # start the DAW
```
Optional backends:
```bash
sudo bash scripts/linux-deps.sh --all
cargo run --release -p daw-app --features jack,pipewire -- --host PipeWire
```
On modern distros PipeWire also provides an ALSA device, so the default build
works with PipeWire out of the box. The native PipeWire/JACK backends give
lower latency and routing between apps.

**Real-time priority:** if the status bar says *no real-time priority*, add
your user to the `audio` group (or configure `rtprio` in
`/etc/security/limits.d/`) and log in again.

### Windows
Install Rust from <https://rustup.rs> (MSVC toolchain), then:
```powershell
cargo run --release -p daw-app
```
Low-latency **ASIO** (optional): install LLVM, set `LIBCLANG_PATH`, then
`cargo run --release -p daw-app --features asio -- --host ASIO`.

### macOS
Install Xcode command-line tools (`xcode-select --install`) and Rust, then:
```bash
cargo run --release -p daw-app
```

### Shipping builds
```bash
cargo build --profile dist -p daw-app -p daw-render   # fat LTO, output in target/dist/
```
GitHub Actions (`.github/workflows/ci.yml`) builds and tests on all three
operating systems and uploads the binaries as artifacts.

## Running

```
daw [--host NAME] [--buffer FRAMES] [--null-audio] [--list-devices]
```

| Key / control | Action |
|---|---|
| Space | Play / stop |
| ▶ while playing | Restart from bar 1 |
| ■ while stopped | Return to bar 1 |
| Double-click master slider | Reset to 0 dB |
| Click the meter | Reset the clip indicator |

Offline render:
```bash
cargo run --release -p daw-render -- --bars 8 --tempo 128 --out dist/click.wav
```

## Testing

```bash
cargo test --workspace            # debug: overflow checks + no-allocation check
cargo test --workspace --release  # release
```
`crates/engine/tests/realtime_safety.rs` installs an allocator that **aborts
if `Engine::process` ever allocates memory**, so real-time safety is enforced
by the test suite.

## Releases

Build artifacts are **not** kept in the repo or workspace. Release builds are
produced with `cargo build --profile dist -p daw-app -p daw-render`, packaged
in `/var/tmp/release/`, and uploaded to Google Drive under `daw/<version>/`
(binary tarball + `.sha256` + source archive of the git tag).
