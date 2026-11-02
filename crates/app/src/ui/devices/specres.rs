//! Spectral Resonator (Live 12): Mode / MIDI column, Freq or Transp. with
//! Hz / ♩, Stretch / Shift, Decay / damping, the modulation column, the
//! spectrogram (Harmonics, Quantize) and Send / Unison / Dry/Wet. Geometry
//! measured from Live 12 at 100 % (l/t = device left/top; text = baselines).

use super::dynamics::{KN, KN_BIG, knob_lv, sig3, tbutton};
use super::fx4::*;
use super::modfx::drag_text;
use super::*;
use daw_project::devices::dev_tap;
use daw_project::devices::params4::srp;
use daw_project::devices::resonators::note_name;
use daw_project::devices::specres::*;

pub const WIDTH: f32 = 515.0;

fn set(cx: &mut Ctx, f: impl FnOnce(&mut SpecResParams)) {
    if let FxParams::SpectralResonator(d) = &mut cx.params {
        f(d);
        cx.undo_step();
    }
}

fn pct(x: f32) -> String {
    format!("{:.0} %", x * 100.0)
}

fn pct1(x: f32) -> String {
    format!("{:.1} %", x * 100.0)
}

fn ms2(x: f32) -> String {
    if x >= 1000.0 { format!("{} s", sig3(x / 1000.0)) } else { format!("{} ms", sig3(x)) }
}

/// Eighth-note glyph for the ♩ button.
fn note_glyph(p: &Painter, c: Pos2, col: Color32) {
    quad(p, Rect::from_min_max(c + vec2(-0.5, -4.5), c + vec2(0.5, 3.5)), col);
    quad(p, Rect::from_min_max(c + vec2(0.5, -4.5), c + vec2(2.5, -3.5)), col);
    quad(p, Rect::from_min_max(c + vec2(-2.5, 2.5), c + vec2(0.5, 4.5)), col);
}

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::SpectralResonator(d) = cx.params else { return };
    let (l, t) = (r.left(), r.top());
    panels(p, r, &[(2.0, 193.0), (195.0, 252.0), (452.0, 514.0)], &[(254.0, 450.0)]);
    if scale_btn(cx, ui, p, l + 452.0, t, srp::SCALE, d.scale_aware) {
        set(cx, |q| q.scale_aware = !q.scale_aware);
    }
    let lt = |p: &Painter, x: f32, y: f32, s: &str, col: Color32| {
        text_at(p, l + x, t + y, Align2::LEFT_BOTTOM, s, font(FS), col);
    };
    let ct = |p: &Painter, x: f32, y: f32, s: &str| {
        text_at(p, l + x, t + y, Align2::CENTER_BOTTOM, s, font(FS), text());
    };

    // ---- Mode / MIDI ----
    let midi = d.midi_mode;
    lt(p, 7.0, 30.0, "Mode", text());
    if tbutton(cx, ui, p, rr(l, t, 6.0, 34.0, 50.0, 47.0), srp::MODE, "Internal", !midi, true) {
        set(cx, |q| q.midi_mode = false);
    }
    cx.salt = 1;
    if tbutton(cx, ui, p, rr(l, t, 53.0, 34.0, 81.0, 47.0), srp::MODE, "MIDI", midi, true) {
        set(cx, |q| q.midi_mode = true);
    }
    cx.salt = 0;
    lt(p, 7.0, 64.0, "MIDI From", text());
    let own = match crate::fx_ops::top_of(&cx.app.project, cx.chain) {
        ChainRef::Track(id) => Some(id),
        _ => None,
    };
    let prj = &cx.app.project;
    let tracks: Vec<(u64, String)> = prj
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, x)| Some(x.id) != own && x.kind == daw_project::TrackKind::Midi)
        .map(|(i, x)| (x.id, prj.track_label(i)))
        .collect();
    let cur = tracks.iter().find(|x| x.0 == d.midi.source).map_or("No Input".to_string(), |x| x.1.clone());
    let mut items: Vec<&str> = vec!["No Input"];
    items.extend(tracks.iter().map(|x| x.1.as_str()));
    let cr = rr(l, t, 6.0, 67.0, 81.0, 80.0);
    let shown = fit(p, &cur, &font(FS), cr.width() - 14.0);
    if let Some(i) = chooser(cx, ui, p, cr, srp::SOURCE, &shown, &items, true) {
        let src = if i == 0 { 0 } else { tracks[i - 1].0 };
        set(cx, |q| q.midi.source = src);
    }
    let ext = midi && d.midi.source != 0;
    let taps: Vec<&str> = ScTap::ALL.iter().map(|x| x.label()).collect();
    let tap_s = if ext { d.midi.tap.label() } else { "" };
    let tr = rr(l, t, 6.0, 83.0, 81.0, 96.0);
    if !ext {
        boxed(p, tr, box_fill(cx.on));
    } else if let Some(i) = chooser(cx, ui, p, tr, srp::TAP, tap_s, &taps, ext) {
        set(cx, |q| q.midi.tap = ScTap::ALL[i]);
    }
    if sel_btn(cx, ui, p, rr(l, t, 6.0, 111.0, 54.0, 124.0), srp::POLY, if d.poly { "Poly" } else { "Mono" }, true, midi) {
        set(cx, |q| q.poly = !q.poly);
    }
    let vi = SR_VOICES[(d.voices as usize).min(SR_VOICES.len() - 1)];
    value_box_ex(cx, ui, p, rr(l, t, 57.0, 111.0, 81.0, 124.0), srp::VOICES, &vi.to_string(), true, false, box_fill(cx.on));
    lt(p, 7.0, 137.0, "MIDI Gate", text());
    if tbutton(cx, ui, p, rr(l, t, 57.0, 127.0, 81.0, 140.0), srp::GATE, if d.midi_gate { "On" } else { "Off" }, d.midi_gate, midi) {
        set(cx, |q| q.midi_gate = !q.midi_gate);
    }
    lt(p, 7.0, 164.0, "Glide", text());
    value_box_ex(cx, ui, p, rr(l, t, 36.0, 153.0, 81.0, 166.0), srp::GLIDE, &ms2(d.glide), true, false, box_fill(cx.on));
    lt(p, 7.0, 180.0, "PB", text());
    value_box_ex(
        cx,
        ui,
        p,
        rr(l, t, 36.0, 169.0, 81.0, 182.0),
        srp::PB,
        &format!("{} st", d.pb.round() as i32),
        true,
        midi,
        box_fill(cx.on),
    );
    px(p, l, t, 86.0, 21.0, 87.0, 183.0, gray(0x24));

    // ---- Freq / Transp. ----
    if midi {
        knob_lv(cx, ui, p, l + 113.5, t + 30.0, srp::TRANSP, "Transp.", &st0(d.transp), 0.0, KN_BIG, 21.5, 51.0, true);
    } else {
        let v = if d.note_mode { note_name(hz_midi(d.freq).round() as i32) } else { hz(d.freq) };
        knob_lv(cx, ui, p, l + 113.5, t + 30.0, srp::FREQ, "Freq", &v, 0.0, KN_BIG, 21.5, 51.0, true);
        if tbutton(cx, ui, p, rr(l, t, 91.0, 89.0, 112.0, 102.0), srp::NOTE_MODE, "Hz", !d.note_mode, true) {
            set(cx, |q| q.note_mode = false);
        }
        let nr = rr(l, t, 115.0, 89.0, 136.0, 102.0);
        boxed(p, nr, if d.note_mode { if cx.on { ORANGE } else { gray(0x75) } } else { gray(0x1e) });
        note_glyph(p, nr.center(), if d.note_mode { gray(0x20) } else { text() });
        cx.salt = 1;
        if cx.toggle(ui, nr, srp::NOTE_MODE, true) {
            set(cx, |q| {
                q.note_mode = true;
                q.freq = midi_hz(hz_midi(q.freq).round());
            });
        }
        cx.salt = 0;
    }
    ct(p, 113.5, 135.0, "Stretch");
    value_box(cx, ui, p, rr(l, t, 91.0, 139.0, 136.0, 152.0), srp::STRETCH, &pct1(d.stretch), true);
    ct(p, 113.5, 165.0, "Shift");
    value_box(cx, ui, p, rr(l, t, 91.0, 169.0, 136.0, 182.0), srp::SHIFT, &format!("{:.1} st", d.shift), true);
    knob_lv(cx, ui, p, l + 165.5, t + 30.0, srp::DECAY, "Decay", &ms2(d.decay), 0.0, KN, 14.5, 36.0, true);
    knob_lv(cx, ui, p, l + 165.5, t + 87.0, srp::HI_DAMP, "Hi Damp", &pct1(d.hi_damp), 0.0, KN, 14.5, 36.0, true);
    knob_lv(cx, ui, p, l + 165.5, t + 145.0, srp::LO_DAMP, "Lo Damp", &pct1(d.lo_damp), 0.0, KN, 14.5, 36.0, true);

    // ---- Modulation ----
    for (i, y) in [21.0, 37.0, 53.0, 69.0].into_iter().enumerate() {
        cx.salt = i as u32;
        if tbutton(cx, ui, p, rr(l, t, 199.0, y, 248.0, y + 13.0), srp::MOD, SR_MODS[i], d.mod_mode == i as u8, true) {
            set(cx, |q| q.mod_mode = i as u8);
        }
    }
    cx.salt = 0;
    let modded = d.mod_mode != 0;
    knob_lv(cx, ui, p, l + 223.5, t + 107.0, srp::MOD_RATE, "Mod Rate", &format!("{:.0} %", d.mod_rate), 0.0, KN, 14.5, 36.0, modded);
    ct(p, 223.5, 166.0, "Pch.Mod");
    value_box_ex(
        cx,
        ui,
        p,
        rr(l, t, 199.0, 170.0, 248.0, 183.0),
        srp::PCH_MOD,
        &format!("{:.2} st", d.pch_mod),
        true,
        false,
        box_fill(cx.on),
    );

    // ---- Spectrogram ----
    if play_btn(cx, ui, p, pos2(l + 264.0, t + 26.3), srp::DISPLAY, d.display) {
        set(cx, |q| q.display = !q.display);
    }
    lt(p, 302.0, 30.0, "Harmonics", gray(0x86));
    drag_text(cx, ui, p, l + 360.0, t + 30.0, srp::HARMONICS, &format!("{:.0}", d.harmonics), true, ORANGE);
    if check(cx, ui, p, l + 390.0, t + 30.0, srp::QUANTIZE, "Quantize", d.quantize) {
        set(cx, |q| q.quantize = !q.quantize);
    }
    spectrogram(cx, ui, p, rr(l, t, 255.0, 36.0, 449.0, 185.0), d.display);

    // ---- Send / Unison / Dry/Wet ----
    knob_lv(cx, ui, p, l + 483.5, t + 30.0, srp::SEND, "Send", &db2(d.send), 0.0, KN, 14.5, 36.0, true);
    let tap = dev_tap(cx.dev.id);
    let lid = egui::Id::unique(("sr_led", cx.dev.id));
    let now = ui.input(|i| i.time);
    let mut last: f64 = ui.data(|dd| dd.get_temp(lid)).unwrap_or(-10.0);
    if tap.take(TAP_LIMIT, 0.0) > 0.5 {
        last = now;
        ui.data_mut(|dd| dd.insert_temp(lid, last));
    }
    let lit = cx.on && now - last < 0.15;
    if lit {
        cx.app.animating = true;
    }
    led(p, pos2(l + 507.5, t + 44.5), lit);
    ct(p, 483.5, 83.0, "Unison");
    value_box_ex(cx, ui, p, rr(l, t, 456.0, 87.0, 510.0, 100.0), srp::UNISON, &format!("{:.0}", d.unison), true, false, box_fill(cx.on));
    ct(p, 483.5, 112.0, "Uni. Amt");
    value_box_ex(cx, ui, p, rr(l, t, 456.0, 116.0, 510.0, 129.0), srp::UNI_AMT, &pct(d.uni_amt), true, false, box_fill(cx.on));
    knob_lv(cx, ui, p, l + 483.5, t + 144.0, srp::DRY_WET, "Dry/Wet", &pct(d.dry_wet), 0.0, KN, 15.5, 37.0, true);
}

// ---- parameters --------------------------------------------------------------

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::SpectralResonator(SpecResParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    let FxParams::SpectralResonator(d) = p else { return };
    match k {
        srp::FREQ => d.freq = midi_hz((hz_midi(d.freq).round() + dir).clamp(SR_MIDI.0, SR_MIDI.1)),
        srp::TRANSP => d.transp = (d.transp + dir).clamp(-SR_TRANSP, SR_TRANSP),
        srp::SHIFT => d.shift = ((d.shift * 10.0).round() + dir).clamp(-SR_SHIFT * 10.0, SR_SHIFT * 10.0) / 10.0,
        srp::HARMONICS => d.harmonics = (d.harmonics + dir).clamp(SR_HARM.0, SR_HARM.1),
        srp::UNISON => d.unison = (d.unison + dir).clamp(SR_UNISON.0, SR_UNISON.1),
        srp::PB => d.pb = (d.pb + dir).clamp(0.0, SR_PB),
        srp::VOICES => d.voices = (d.voices as f32 + dir).clamp(0.0, (SR_VOICES.len() - 1) as f32) as u8,
        _ => {
            if let Some(v) = norm(p, k) {
                set_norm(p, k, ((v * 100.0).round() + dir) / 100.0);
            }
        }
    }
}

pub(super) fn name(k: u8) -> &'static str {
    match k {
        srp::FREQ => "Frequency",
        srp::TRANSP => "Transpose",
        srp::STRETCH => "Stretch",
        srp::SHIFT => "Shift",
        srp::DECAY => "Decay",
        srp::HI_DAMP => "Hi Damp",
        srp::LO_DAMP => "Lo Damp",
        srp::MOD_RATE => "Mod Rate",
        srp::PCH_MOD => "Pitch Mod",
        srp::HARMONICS => "Harmonics",
        srp::SEND => "Input Send",
        srp::UNISON => "Unison",
        srp::UNI_AMT => "Unison Amount",
        srp::DRY_WET => "Dry/Wet",
        srp::GLIDE => "Glide",
        srp::PB => "Pitch Bend Range",
        srp::VOICES => "Voices",
        srp::MODE => "Mode",
        srp::NOTE_MODE => "Frequency Unit",
        srp::POLY => "Mono/Poly",
        srp::GATE => "MIDI Gate",
        srp::MOD => "Modulation Mode",
        srp::QUANTIZE => "Quantize",
        srp::SCALE => "Scale Awareness",
        srp::DISPLAY => "Spectrogram",
        srp::SOURCE => "MIDI From",
        srp::TAP => "MIDI From Tap",
        _ => "",
    }
}
