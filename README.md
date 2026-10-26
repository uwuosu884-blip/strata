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
- **Clip View** (Live style): clip / launch / notes or audio panels, start,
  end and loop markers, launch modes and Follow Actions, clip envelopes, MPE
  editor, Groove Pool, MIDI Tools (Pitch & Time, 12 Transform and 5 Generate
  tools with Auto Apply).
- **Warping**: Beats / Tones / Texture / Re-Pitch / Complex / Complex Pro,
  warp markers and transients, Warp As, quantize markers, reverse, crop,
  Tap Tempo, Nudge and leader clips with tempo Follow.
- **Piano roll**: add, move and resize notes, edit velocity, quantize and
  transpose.
- **Recording**: audio and MIDI, from a MIDI controller or the computer
  keyboard, with latency compensation. Several armed audio tracks record
  their own inputs at once (mono inputs make mono WAVs, stored in
  `<project> Samples/Recorded/`).
- **Routing & I/O** (Live style, Ctrl+Alt+I): per-track Audio/MIDI From
  (interface channels with meters, Resampling, other tracks with Pre FX /
  Post FX / Post Mixer / drum pad taps, MIDI ports or All Ins with channel
  filter, computer keyboard), Monitor In/Auto/Off, Audio/MIDI To (master,
  group, other tracks, interface outputs, external MIDI ports/channels or
  another MIDI track's input or instrument). Settings (Ctrl+,) choose the
  driver, devices and buffer, configure/rename mono and stereo channels and
  switch MIDI ports on; they are saved to `settings.ron` in the user config
  folder (`$DAW_CONFIG_DIR` overrides it).
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
    [--output-device NAME] [--input-device NAME]
    [--null-audio] [--no-input] [--no-midi]
```
`examples/demo.dawproj` is a small demo song.

| Key | Action |
|---|---|
| Space / Shift+Space | Play from the start point / continue; stop |
| Tab | Switch Session ↔ Arrangement |
| Shift+Tab | Detail panel: clip view ↔ device view |
| Enter | Launch the selected scene |
| F9 | Record |
| A … ' | Play notes (computer keyboard MIDI); Z/X octave, C/V velocity |
| M | Toggle computer keyboard MIDI |
| Ctrl+Alt+I | Show / hide the In/Out section |
| Ctrl+, | Settings (audio / MIDI) |
| Del | Delete selection (notes, time selection, clips or tracks) |
| 0 | Deactivate / activate the selected clips or notes |
| Esc | Clear the selection (and the last-touched track) |
| Ctrl+Z / Ctrl+Shift+Z (Ctrl+Y) | Undo / redo |
| Ctrl+S / Ctrl+Shift+S / Ctrl+O / Ctrl+N | Save / save as / open / new |
| Ctrl+Shift+R | Export audio |
| Ctrl+T / Ctrl+Shift+T | Add audio / MIDI track |
| Ctrl+G / Ctrl+Shift+G | Group selected tracks / ungroup |
| Ctrl+L | Loop the selected region (or clips); again: loop off; nothing selected: previous loop back on |
| Ctrl+J | Consolidate clips (audio is rendered to a new sample) |
| Ctrl+Shift+J | Crop the clip in Clip View |
| Ctrl+U / Ctrl+Shift+U | Quantize (notes or warp markers) / Quantize tool |
| Ctrl+Enter | Apply the MIDI tool |
| Ctrl+R | Rename the selected clip, else the selected track (or double-click a header / clip title) |
| Ctrl+A | Select all notes (piano roll) or all clips |
| ↑ / ↓, Shift+↑ / ↓ | Transpose the selected notes by a semitone / an octave |
| Ctrl+E | Split clips at the selection edges or at the playhead |
| Ctrl+Shift+M | Insert a MIDI clip over the selected region |
| Ctrl+D | Duplicate (notes, selection, clips or session clip) |
| Ctrl+B | Show / hide browser |

Arrangement mouse: drag a clip's title bar to move it (and all selected
clips); drag its left/right edge to trim (up to the clip's content). Drag on a lane or clip body to make a
time selection; drag inside a selection to cut that part out and move it.
Click empty space to move the playhead; click a clip to select it and put the
start point at its start. Shift+click selects a range (headers: the tracks in
between; clips: every clip in the tracks × time rectangle), Ctrl+click toggles.
Wheel scrolls, Shift+wheel scrolls horizontally, Ctrl+wheel zooms, Alt+wheel
changes the height of the track touched last (all after Esc); zoom fully out,
or use the ⊳ header button, to collapse a
track to clip title bars. Drag headers to reorder tracks or move them into
or out of groups. Drag in the ruler to set the loop range, drag the loop
brace or its edges to adjust it, and click it to switch looping on or off. Hold
Alt to ignore the grid. Right-click → Grid sets the grid size.

Piano roll: double-click adds or removes notes (new notes use the last length
and velocity you set), drag on empty space to select with a rubber band
(Ctrl+D then duplicates that time range), Ctrl+drag copies notes, Ctrl+wheel /
Alt+wheel zoom, Shift+wheel scrolls, and clicking the ruler moves the playhead.
Notes dragged past the clip end are kept but silent. The headphones button
toggles note preview; drag the line above the velocity lane to resize it.

Tempo box: drag or wheel in whole BPM, Shift for 0.01 steps, double-click to
type. Time signature: drag or wheel, double-click for 4/4.

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
