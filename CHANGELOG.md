# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Planned
- Effects (EQ, compressor, reverb, delay) and automation lanes (phase 6).
- Plugin hosting (CLAP/VST3), audio warping, multi-core track processing.

## [0.1.0] - 2026-10-25

First public version: a working "mini-Live" with session and arrangement
views, built-in instruments, mixer, recording, undo/redo and WAV export.

### Added

**Real-time audio engine (`daw-engine`)**
- Lock-free design: the audio callback never locks, allocates or does I/O.
  The test suite installs an allocator that aborts if `Engine::process` ever
  allocates, covering every feature.
- Commands go to the engine through a wait-free queue. Edited songs are
  swapped in atomically, and the old song is freed off the audio thread.
- Transport: play/stop/rewind, tempo, time signature, metronome, loop region
  and sample-accurate positioning.
- Session launching: per-clip and per-scene launch with launch quantization
  (none to 8 bars), stopping tracks or all clips, and "Back to Arrangement".
- Arrangement playback with loop jumps that keep MIDI notes sample-accurate.
- Mixer: per-track gain, pan, mute, solo and input monitoring, plus master
  gain, with smoothed parameter changes (no zipper noise). Peak meters and
  CPU load are reported to the GUI.
- Instruments:
  - **Synth**: 2 oscillators, filter with envelope, amp envelope and 5
    presets (Init Saw, Bass, Pluck, Pad, Square Lead).
  - **Sampler**: pitched playback of any sample, with ADSR.
  - **Drum Rack**: 16 pads with gain, tune and choke groups.
- Audio and MIDI recording, including latency compensation and a background
  writer.

**Project and core (`daw-project`, `daw-core`)**
- Project files (`.dawproj`) in human-readable RON, with format versioning.
  Samples inside the project folder are stored as relative `/` paths, so
  project folders move between machines and operating systems. Clip and
  track ids are repaired on load.
- Undo/redo with 200 steps of history.
- Sample loading for WAV, FLAC, MP3, AAC and MP4 (symphonia) with
  high-quality resampling (rubato). A background sample cache keeps disk
  access off the GUI thread.
- A synthesized factory drum kit (Kick, Snare, Clap, hats, toms, Rim,
  Cowbell, Crash), so no sample files are needed.
- WAV export at 16-bit (TPDF dither), 24-bit (TPDF dither) or 32-bit float.

**Desktop app (`daw`)**
- **Session view**: clip grid with launch buttons, scene launchers, track
  mixer strips (fader, pan, mute, solo, arm, monitor) and master meter.
- **Arrangement view**: timeline with ruler, loop brace and a draggable
  playhead. Clips can be moved, resized, duplicated and deleted; MIDI notes
  and audio waveforms are drawn in the lanes.
- Recording into the arrangement replaces whatever was in that region (like
  Live). Session recording records into empty slots.
- **Piano roll**: add, move and resize notes, edit velocities, quantize,
  transpose by octaves, and double or halve clip length. The view scrolls to
  the selected clip's notes.
- Device editors for the synth, sampler and drum rack.
- **Browser**: instruments, factory sounds and the file system. Drag items
  onto tracks, slots or devices.
- **Computer-keyboard MIDI** (A–' keys, Z/X octave, C/V velocity) and
  external MIDI input (midir).
- File menu: new/open/save/save as, export audio, add tracks and scenes.
- Keyboard shortcuts: Space play/stop, Tab switches view, Enter launches the
  scene, F9 records, Ctrl+Z / Ctrl+Shift+Z undo/redo, Ctrl+S save, Ctrl+O
  open, Ctrl+Shift+R export, Ctrl+T add track, Ctrl+D duplicate, Ctrl+B
  toggles the browser.
- Audio backends: ALSA (default), JACK, PipeWire and PulseAudio on Linux;
  WASAPI and ASIO on Windows; CoreAudio on macOS.
- `--null-audio`, `--no-input` and `--no-midi` flags for headless use and
  testing.

**Offline renderer (`daw-render`)**
- Renders a project, the arrangement or a single scene to WAV, far faster
  than real time (~150× on 2 cores).
- `--demo` and `--write-demo` produce a built-in demo song.

**Tooling**
- CI on Linux, Windows and macOS: format check, clippy, and tests in debug
  and release.
- Tagged releases are built and published automatically.

### Known limitations
- No effects or automation yet (phase 6).
- No plugin hosting (CLAP/VST3) or time-stretching yet.
- No Windows download yet. Windows builds and passes the tests in CI, so
  build it from source for now.
- The macOS build is only tested in CI.

[Unreleased]: https://github.com/uwuosu884-blip/strata/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/uwuosu884-blip/strata/releases/tag/v0.1.0
