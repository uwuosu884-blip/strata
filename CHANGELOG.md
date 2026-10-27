# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Planned
- Effects (EQ, compressor, reverb, delay) and automation lanes (phase 6).
- Plugin hosting (CLAP/VST3), multi-core track processing.

## [0.7.0] - 2026-10-27

### Changed
- **Ableton Live 12 look**, measured from Live screenshots at 1920×1080 and
  matched pixel for pixel at 100 % zoom. The UI font is now Inter (bundled,
  OFL).
  - **Top bar**: Live's layout with tap/tempo, signature, metronome, quantize,
    transport, position/loop fields, Follow switch, CPU and output level.
    Live controls the DAW doesn't have yet (Link, Key/MIDI map, scale chooser,
    H/W, overview) are shown greyed out.
  - **Browser**: search box, Collections, places sidebar and a Live item list.
    Collections and search are greyed out.
  - **Arrangement**: ruler rows (scale/loop/locators, bars, time), alternating
    bar shading, Live 290 px track headers (name · I/O · mixer), dot meters,
    the master row and the time ruler.
  - **Clips**: coloured title strip over a body of almost the same colour with
    dark notes or waveform, and grid lines visible through it. Selected clips
    get a dark title and a light-blue body.
  - **Session**: 94 px columns with coloured headers, 17 px slots with stop
    squares, scene launchers on the right ("Main"), a "Stop Clips" row, I/O
    (From / Monitor / To), and mixer strips with a volume pill, pan knob,
    activator/solo/arm and a fader beside a meter with a 0–60 dB scale.
  - **Clip View / piano roll / sample editor**: Live's row, bar-band and grid
    colours, plus the dark waveform on a light editor.
  - **Devices**: Live device frames with a title strip and activator.
  - **Status bar**: Live's layout.
- New tracks and clips follow the colour policy below.

### Added
- **Settings window** in Live style (Ctrl+,) with a tab list.
- **Theme & Colors** tab:
  - **Theme**: Default / Dark / Mid Dark / Mid Light / Light, Appearance
    (Follow System / Light / Dark), Tone (Neutral / Cool / Warm), High Contrast.
  - **Customization**: Grid Line Intensity, Brightness, Color Intensity, Color
    Hue.
  - **Track and Clip Colors**: Auto-Assign Track Colors, Default Track Color,
    Reduced Automatic Colors, Clip Color (Track Color / Random).
  - All settings apply live and are saved in `settings.ron`.
- **Zoom Display** (Settings > Display & Input): 50–200 %.

## [0.6.1] - 2026-10-27

### Added
- **Master device view**: clicking the master header (Arrangement) or the
  master column header / strip (Session) selects the master and shows its
  (empty) audio effect chain, ready for effects.
- **Cue volume** (Live's Preview/Cue): a blue knob right of the master volume
  controls the metronome and browser preview. The Cue bus is mixed after the
  master gain, so it never reaches Resampling. The master pan sits under the
  two knobs once the master row is expanded; the Session master strip has pan,
  fader and Cue.
- **Browser preview**: with "Preview" on, clicking a factory sound or a file
  plays it on the Cue bus (loads in the background, resampled on the fly).
- **Output level** in the top bar: a monitoring gain on top of the master that
  only changes what you hear and never what gets rendered (master -0.1 dB plus
  output -6 dB = -6.1 dB heard, -0.1 dB exported). Endless drag like the
  tempo box (hidden cursor), wheel, double-click = 0 dB. Saved in Settings.
- **Sample rate** in Settings > Audio (Default / 44.1–192 kHz) and
  `--sample-rate HZ`. On a change, recordings still in memory are resampled,
  every sample position (clip offsets, warp markers, transients, undo history)
  moves to the new rate, and projects store their `frame_rate` so they open
  correctly at any rate (also in `daw-render`).
- **Alt+Plus / Alt+Minus** zoom the selected tracks' height (all tracks when
  none are selected, the master row when the master is selected).
- **Master colour**: right-click the master header to pick a colour.
- **Shift+drag of a clip edge** on a warped audio clip stretches the sample
  together with the clip (warp markers, loop, start and envelopes scale too).

### Changed
- Soloing dims the orange activators of the tracks the solo silences (their
  group buses and sources stay lit, matching what the engine plays).
- New tracks (Ctrl+T / Ctrl+Shift+T, menus) are inserted below the selected
  track, inside its group; a selected group gets the new track after its
  last child.
- The metronome now follows the Cue volume instead of the master volume.

## [0.6.0] - 2026-10-26

### Added
- **Clip View (Live 12 ch. 8)** in the detail area, panels foldable (`<`):
  - Clip box: Clip Activator, name (double-click renames), colour menu with
    Assign Track Color, Info Text, selected-clip count; Signature, Groove +
    Commit, Scale (root / name, highlighted in the editor), Start / End and
    Loop Position / Length with Set buttons (min:sec for unwarped audio),
    Duplicate Loop, Crop (Ctrl+Shift+J).
  - Launch box (Session clips, `L`): Trigger / Gate / Toggle / Repeat, Legato,
    Quantization, Velocity, Follow Actions (A / B, Chance, Time, Linked,
    multiplier, Jump), Bank / Sub-bank / Program sent on launch.
  - Marker strip: draggable start / end / loop markers, loop brace keys
    (Up/Down shift by loop length, Ctrl+Left/Right shorten/lengthen,
    Ctrl+Up/Down double/halve), Follow auto-scroll.
- **Audio clips, tempo and warping (ch. 9)**: Warp toggle, Seg. BPM ×2/÷2,
  warp modes Beats (Preserve, Transient Loop, Envelope), Tones, Texture
  (Grain / Flux), Re-Pitch, Complex, Complex Pro (Formants, Envelope); warp
  markers (double-click / Ctrl+I add, drag, Delete), transients, Set 1.1.1
  Here, Warp From Here, Warp As N bars, Quantize Markers (Ctrl+U); Gain,
  Transpose, Detune, Fade, RAM, HiQ, Reverse (new sample), Edit (external
  editor). Phase vocoder with identity phase locking for the stretch modes.
  Tap Tempo, Phase Nudge −/+, Leader clips + Follow (tempo automation).
- **MIDI Tools (ch. 11)**: Pitch & Time (Transpose in scale degrees, Fit to
  Scale, Invert, Add Interval, Stretch ×2/÷2, Set Length, Humanize, Reverse,
  Legato); Transform tools Arpeggiate, Chop, Connect, Glissando, LFO,
  Ornament, Quantize (Ctrl+Shift+U), Recombine, Span, Strum, Time Warp,
  Velocity Shaper; Generate tools Rhythm, Seed, Shape, Stacks, Euclidean.
  Auto Apply re-runs from the original notes; Apply / Ctrl+Enter, Reset.
- **Clip envelopes**: Volume, Pan, Clip Gain, Transpose, Pitch Bend, Mod
  Wheel; linked or unlinked with their own loop length.
- **MPE**: per-note pitch / slide / pressure curves, MPE editor tab, synth
  per-note expression.
- **Groove Pool**: factory grooves, extract from a clip, Base / Quantize /
  Timing / Random / Velocity, Global Amount, hot-swap, Commit.
- Clip launch buttons fire on press; release sends Gate / Repeat stops.

### Changed
- Arrangement clips draw from their start marker, loop and warp markers.
- Track header: Routing & I/O sits between the name and the controls and
  collapses when zoomed out; pan knob matches the volume knob; Solo + Arm
  together match one knob; no rounded corners on the colour part.
- The device view has a fixed height; clip view / piano roll stay resizable.

### Fixed
- Tracks with Resampling input were silent.
- Clicking the empty area below the tracks deselects everything without
  moving the playhead.
- Piano roll: resizing several notes keeps their relative lengths; clicking
  the keys plays them.
- BPM field released focus after editing.
- Clicking a clip moves the start point only from its title bar.

## [0.5.0] - 2026-10-26

### Added
- **Audio and MIDI inputs / routing (Live's Routing & I/O)**:
  - In/Out section in the session mixer and the arrangement headers
    (View menu or Ctrl+Alt+I). Changes apply to every selected track.
  - Audio From: Ext. In channels (mono or stereo pairs, with signal and
    overload meters), Resampling (the main output; the recording track is left
    out), another track at Pre FX / Post FX / Post Mixer, or a drum rack pad
    (tapped pads leave the internal mix), or No Input (hides Arm).
  - MIDI From: All Ins, Computer Keyboard, a single port, or another MIDI
    track; channel filter All / 1–16 with activity meters.
  - Monitor In / Auto / Off (audio defaults to Off, MIDI to Auto). Right-click:
    Keep Monitoring Latency in Recorded Audio and Return to Default; Delete
    over the radio resets it too.
  - Audio To: Master, the parent group, another track (many-to-one submixes;
    solo follows Live's rules), or Ext. Out channels (mono = L+R −6 dB).
    The master output channel can be chosen too.
  - MIDI To (MIDI tracks without an instrument): an external port + channel,
    or another MIDI track's Track In or its instrument directly.
  - The selected MIDI track is auto-armed while no MIDI track is armed.
  - Transport MIDI Track in/out indicators.
- **Settings dialog** (Ctrl+, / View menu): driver type, output and input
  device, buffer size, latency readout; input/output channel configuration
  (enable mono/stereo channels, rename them — names are stored per device);
  MIDI input/output ports with Track switches. Saved to `settings.ron`.
  `--output-device` / `--input-device` command-line options.
- **Multi-track audio recording**: each armed audio track records its own
  input. Mono inputs record mono WAVs; recordings are saved to
  `<project> Samples/Recorded/`.

### Changed
- `M` toggles the computer MIDI keyboard (was Shift+M).
- Project format version 4 (old files load; the old monitor switch becomes
  Monitor In).

### Fixed
- Ctrl+L with nothing selected re-applies the previous loop after it was
  turned off.
- Piano roll zoom (Ctrl+wheel, and Alt+wheel row height) is anchored at the
  point under the cursor when the zoom gesture started, not the moving cursor.
- Clicking Arm on one of several selected tracks arms all of them;
  Ctrl+click arms only that track.

## [0.4.0] - 2026-10-26

### Added
- **Master track in the arrangement**, pinned below the tracks (Live style).
  It is collapsed by default. It has no activator, solo or arm; volume and
  pan are visible even when collapsed. Master pan is new (engine, export
  and project file).
- **Live-style track headers**: the left area is in the track colour and holds
  the collapse button and the name. On the right, row 1 has the activator
  (track number), solo and arm; row 2 has volume under the activator and pan
  under solo. Collapsed tracks show only the triangle, name, activator and
  solo.
- **Clip rename**: double-click a clip's title bar, its name bar in the clip
  view, or press Ctrl+R with a clip selected.
- **Save-changes dialog keys**: ←/→ (or Tab) move the highlight, Enter/Space
  confirms, `S` saves, `D` doesn't save, `C` cancels.
- **Piano roll**:
  - Ctrl+A selects all notes.
  - ↑/↓ transpose the selected notes by a semitone; Shift+↑/↓ by an octave.
  - Ctrl+drag a note to copy it.
  - Headphones button above the keys toggles note preview.
  - The velocity lane can be resized with a splitter. It has a 1/64/127 scale,
    shows the value while dragging, and draws each note's length as a thin
    line.
  - Double-clicked notes use the length and velocity you last set.
  - Notes can be dragged past the clip end. There they are kept but silent,
    drawn in the darker area, and hidden in the arrangement. Shortening a
    clip no longer loses the notes beyond its end, and the view zooms out to
    the furthest note.

### Changed
- **Selection**: Shift+click selects a range of track headers; Ctrl+click
  toggles. For clips, Shift+click selects every clip in the rectangle spanned
  by the anchor clip and the clicked clip (tracks × time); Ctrl+click toggles.
- Clicking a clip moves the start point to the clip's start. Nothing starts
  playing, and playback doesn't jump while playing.
- **Alt+wheel** resizes the track you touched last (header, lane or clip), or
  all tracks after Esc or a click on empty space.
- **Arming** is no longer exclusive. Every armed MIDI track receives the
  hardware and computer-keyboard MIDI input and records.
- **Ctrl+D in the piano roll** duplicates the whole rubber-band time range
  with its notes, placed right after the range, which moves along.
- The ±12 and ×2/÷2 buttons act on the selected notes only.
- **Tempo**: drag or wheel steps whole BPM and shows no decimals. Hold Shift
  for 0.01 steps (e.g. 120.23).
- **Time signature box**: drag or wheel to change; double-click resets to 4/4.
- Vertical piano-roll zoom (Alt+wheel) keeps the pitch under the pointer in
  place.

### Fixed
- The detail panel keeps the height you set when switching between the piano
  roll and the device view.

## [0.3.0] - 2026-10-25

### Added
- **Track numbers**: every track shows its position number before its name,
  and the number follows the track when it moves. Renaming a track drops its
  number without changing the other tracks. A name starting with `#` gets the
  number substituted (`# Bass` shows as `3 Bass`).
- **Rename** with Ctrl+R, by double-clicking the header, or from the track
  menu. Enter (or clicking elsewhere) commits, Esc cancels.
- **Volume and pan in arrangement headers** (Live style): a dB box and a pan
  box (50L … C … 50R). Drag to change, Shift for fine steps, double-click to
  reset.
- **Per-track height**: Alt+wheel resizes the selected tracks (all tracks
  when none are selected).
- **Collapse**: zooming a track all the way out, or using the ⊳/⊽ button at
  the left of its header, collapses it so its clips show only their title
  bars.
- **Reorder tracks** by dragging headers, including into and out of groups.
- **Ctrl+J consolidate**: merges the selected clips (or the time selection)
  into one clip per track. MIDI notes are merged. Audio is rendered offline
  into a new sample.
- **0 deactivates** the selected clips, or the selected notes when the piano
  roll has focus. Deactivated items are drawn grey and are silent. Clips also
  get an "Active" checkbox.
- **Grid menu** in the arrangement and piano-roll context menus: adaptive
  (Widest … Narrowest), fixed (4 bars … 1/32), or off.
- **Alt bypasses the grid** for playhead clicks, clip trimming and note
  moving/resizing.
- **Shift+Tab** switches the detail panel between the clip view and the
  device view.
- **Clip names** follow the track: `<track> <n>` for created and recorded
  clips.
- **Loop brace edges** can be dragged in the ruler.
- **Piano roll**:
  - Rubber-band selection (drag on empty grid; Shift/Ctrl adds to the
    selection).
  - Ctrl+wheel zooms horizontally, Alt+wheel changes row height, Shift+wheel
    scrolls horizontally, and dragging past an edge auto-scrolls.
  - A ruler: click it to move the song playhead.
  - Right-click menu with Delete, Duplicate, Deactivate, Select All and Grid.

### Changed
- **Ctrl+L** a second time on the same region turns the loop off.
- **Play** while playing (or after Stop) restarts from the last start point.
  Stop leaves the playhead where it is, and Shift+Space continues from there.
- Clips can't be trimmed beyond their content (audio sample or MIDI length).
- New MIDI tracks are empty (no instrument) until you drop one on them.
- BPM box: drag up or down with the cursor hidden, so it never stops at the
  screen edge. Double-click to type a value.
- Project format version 3. Older projects load fine, and old auto names
  (`1 MIDI`) are migrated to numbered names.

### Fixed
- Delete now deletes the selected tracks.
- Ctrl+D with notes selected duplicates the notes, not the clip.
- Piano roll velocity lane: overlapping (chord) notes all show their own
  stem, and with nothing selected a drag edits the whole chord instead of
  only the last note.
- The device view no longer sticks to an audio clip after you click another
  track's header.

## [0.2.0] - 2026-10-25

### Added
- **Group tracks** (Ctrl+G, Ctrl+Shift+G to ungroup). A group is a real audio
  bus: its children are summed into it, then the group's volume, pan, mute
  and solo apply. Groups can be nested. Soloing a child keeps its group
  audible, and soloing a group keeps its children audible. Groups fold in
  both views. The group lane shows an overview of its children's clips. In
  the session view, a group's slot launches the children's clips of that
  scene, and its stop button stops them. Deleting or duplicating a group
  includes its tracks.
- **Arrangement selection**:
  - Drag on a lane or clip body to make a time selection across tracks.
  - Click or Shift+click to select clips.
  - Shift+click track headers to select several tracks.
- **Ctrl+L** loops the selected region (or the selected clips) and turns
  looping on.
- **Ctrl+E** splits clips at the edges of the time selection, or splits the
  selected clips (or the clips on the selected track) at the playhead.
- Dragging inside a time selection cuts that part out of the clips and moves
  it, across tracks too.
- **Ctrl+Shift+M** inserts an empty MIDI clip over the selected region.
- **Del / Ctrl+D** work on the time selection: delete or duplicate the
  material in it.
- Moving or dropping clips overwrites what lies underneath (like Live).
- Arrangement zoom like Live: Ctrl+wheel zooms around the mouse and
  Alt+wheel changes track height. Wheel and Shift+wheel scroll.
- Clicking anywhere in the arrangement moves the playhead.
- Files dragged from the OS file manager onto the arrangement are imported.
- **Opus** audio (`.opus`, Opus in `.ogg`) via a pure-Rust decoder.
- Small level meters in the arrangement track headers.

### Changed
- Arrangement track headers are on the right, like Live.
- Ctrl+T now adds an audio track and Ctrl+Shift+T a MIDI track (swapped).
- Dragging in the ruler sets the loop range without turning looping on.
  Dragging the loop brace moves it. Clicking the brace still turns looping
  on or off.
- Clip moves snap the clip under the mouse to the grid.

### Fixed
- OGG files that failed to load: Opus-encoded `.ogg` files (common from
  yt-dlp, voice notes and browsers) had no decoder. Vorbis and FLAC in OGG
  are covered by tests.
- Possible crash when a track was deleted from a header menu while the
  views were drawing, or when undo removed the selected track.

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

[Unreleased]: https://github.com/uwuosu884-blip/strata/compare/v0.7.0...HEAD
[0.7.0]: https://github.com/uwuosu884-blip/strata/compare/v0.6.1...v0.7.0
[0.6.1]: https://github.com/uwuosu884-blip/strata/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/uwuosu884-blip/strata/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/uwuosu884-blip/strata/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/uwuosu884-blip/strata/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/uwuosu884-blip/strata/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/uwuosu884-blip/strata/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/uwuosu884-blip/strata/releases/tag/v0.1.0
