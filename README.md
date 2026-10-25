# daw

An Ableton Live–style digital audio workstation written in Rust: fast,
lightweight and real-time safe. It targets **Linux (priority), Windows and
macOS**.

> **Status: v0.1.0, the first version.** Session and arrangement views, audio
> and MIDI clips, a built-in synth, sampler and drum rack, a mixer, recording,
> undo/redo and WAV export. Effects and automation come next. See
> [CHANGELOG.md](CHANGELOG.md).

**Download:** [Releases](https://github.com/uwuosu884-blip/strata/releases)
has builds for Linux and macOS. Windows builds are coming; for now, build
from source (see below).

## Features

- **Session view**: clip grid, scene launching with launch quantization, and
  mixer strips (fader, pan, mute, solo, arm, monitor).
- **Arrangement view**: timeline with loop brace. Clips can be moved,
  resized and duplicated, and recording replaces what was in that region.
- **Instruments**: 2-oscillator synth with filter and presets, sampler, and a
  16-pad drum rack with choke groups. A factory drum kit is built in, so no
  sample files are needed.
- **Piano roll**: add, move and resize notes, edit velocity, quantize and
  transpose.
- **Recording**: audio and MIDI, from a MIDI controller or the computer
  keyboard, with latency compensation.
- **Files**: WAV/FLAC/MP3/AAC import, human-readable `.dawproj` projects, and
  WAV export at 16-bit, 24-bit or 32-bit float.
- **Efficient**: the audio thread never locks or allocates (enforced by the
  tests), and it uses about 1% CPU for the demo song. Offline export runs
  ~150× faster than real time.

## Layout

```
crates/
  engine/   real-time audio engine (no GUI code). Engine::process is RT-safe:
            no locks, no allocation, no I/O. Talks to the GUI through a
            lock-free command queue (rtrb) and atomics.
  project/  the document model (.dawproj, RON) + undo/redo history
  core/     sample decoding/resampling/cache, factory kit, project → engine
            compiler, recorder, WAV export, persistence
  render/   `daw-render`: renders a project to WAV faster than real time
  app/      `daw`: the desktop app (egui/eframe GUI + cpal audio + midir MIDI)
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

## Running

```
daw [PROJECT.dawproj] [--host NAME] [--buffer FRAMES] [--list-devices]
    [--null-audio] [--no-input] [--no-midi]
```
`examples/demo.dawproj` is a small demo song.

| Key | Action |
|---|---|
| Space | Play / stop |
| Tab | Switch Session ↔ Arrangement |
| Enter | Launch the selected scene |
| F9 | Record |
| A … ' | Play notes (computer keyboard MIDI); Z/X octave, C/V velocity |
| Shift+M | Toggle computer keyboard MIDI |
| Del | Delete selection (time selection, clips or notes) |
| Esc | Clear the arrangement selection |
| Ctrl+Z / Ctrl+Shift+Z (Ctrl+Y) | Undo / redo |
| Ctrl+S / Ctrl+Shift+S / Ctrl+O / Ctrl+N | Save / save as / open / new |
| Ctrl+Shift+R | Export audio |
| Ctrl+T / Ctrl+Shift+T | Add audio / MIDI track |
| Ctrl+G / Ctrl+Shift+G | Group selected tracks / ungroup |
| Ctrl+L | Loop the selected region (or clips) |
| Ctrl+E | Split clips at the selection edges or at the playhead |
| Ctrl+Shift+M | Insert a MIDI clip over the selected region |
| Ctrl+D | Duplicate (selection, clips or session clip) |
| Ctrl+B | Show / hide browser |

Arrangement mouse: drag a clip's title bar to move it (and all selected
clips); drag its right edge to resize. Drag on a lane or clip body to make a
time selection; drag inside a selection to cut that part out and move it.
Click anywhere to move the playhead. Shift+click headers or clips to
multi-select. Wheel scrolls, Shift+wheel scrolls horizontally, Ctrl+wheel
zooms, Alt+wheel changes track height. Drag in the ruler to set the loop
range; click the loop brace to switch looping on or off.

Audio files: WAV, AIFF, FLAC, MP3, AAC/M4A, OGG (Vorbis, FLAC and Opus).

Offline render:
```bash
daw-render examples/demo.dawproj -o song.wav --bits 24   # whole arrangement
daw-render examples/demo.dawproj -o scene.wav --scene 1  # one session scene
daw-render --demo -o demo.wav                            # built-in demo
```

## Testing

```bash
cargo test --workspace            # debug: overflow checks + no-allocation check
cargo test --workspace --release  # release
```
`crates/engine/tests/realtime_safety.rs` installs an allocator that **aborts
if `Engine::process` ever allocates memory**, so the tests enforce real-time
safety.

## Releases

1. Update `version` in `Cargo.toml` and add a section to `CHANGELOG.md`.
2. Commit, then `git tag vX.Y.Z && git push origin main vX.Y.Z`.
3. `.github/workflows/release.yml` builds all platforms and publishes the
   GitHub Release, using that version's changelog section as the notes.

Build artifacts are never committed. Copies of each release are also kept on
Google Drive under `daw/<version>/`.
