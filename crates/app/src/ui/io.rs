//! Live's In/Out section (input/output choosers, monitor radio) and the
//! Settings dialog (audio device, channel configuration, MIDI ports).

use super::widgets::*;
use crate::app::{DawApp, SettingsTab};
use crate::settings::{ChanEntry, ChannelConfig, Settings};
use daw_project::{AudioIn, AudioOut, InstrumentDef, MidiIn, MidiOut, Monitor, Tap, TrackKind};
use eframe::egui::{self, Color32, CornerRadius, Rect, RichText, Sense, pos2, vec2};

/// Height of the In/Out block in a mixer column.
pub const IO_H: f32 = 132.0;
const MON_BLUE: Color32 = Color32::from_rgb(70, 150, 240);

fn small(s: impl Into<String>) -> RichText {
    RichText::new(s.into()).size(super::theme::FS)
}

fn caption(ui: &mut egui::Ui, s: &str) {
    ui.label(RichText::new(s).size(super::theme::FS_SMALL).color(Color32::GRAY));
}

/// Chooser label with room for a meter at its left (Live input choosers).
fn metered(ui: &egui::Ui, s: impl Into<String>) -> egui::WidgetText {
    let mut job = egui::text::LayoutJob::default();
    let fmt = egui::TextFormat::simple(egui::FontId::proportional(super::theme::FS), ui.visuals().widgets.inactive.text_color());
    job.append(&s.into(), 9.0, fmt);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.into()
}

/// 9 px meter slot at the left of a chooser button.
fn meter_slot(r: Rect) -> Rect {
    let y = (r.center().y - 4.5).round();
    Rect::from_min_max(pos2(r.min.x + 2.0, y), pos2(r.min.x + 7.0, y + 9.0))
}

/// Signal / activity bar drawn at the right of a chooser entry.
fn level_bar(ui: &egui::Ui, row: Rect, level: f32, clip: bool) {
    let r = Rect::from_min_max(pos2(row.max.x - 30.0, row.center().y - 3.0), pos2(row.max.x - 4.0, row.center().y + 3.0));
    ui.painter().rect_filled(r, CornerRadius::ZERO, gray(25));
    let w = r.width() * level.clamp(0.0, 1.0);
    ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(w, r.height())), CornerRadius::ZERO, PLAY_GREEN);
    if clip {
        ui.painter().rect_filled(Rect::from_min_size(pos2(r.max.x - 3.0, r.min.y), vec2(3.0, r.height())), CornerRadius::ZERO, REC_RED);
    }
}

/// Selectable chooser entry with an optional meter (level 0..1, overload).
fn entry(ui: &mut egui::Ui, selected: bool, text: &str, meter: Option<(f32, bool)>) -> bool {
    let r = ui.add(egui::Button::selectable(selected, small(text)).min_size(vec2(170.0, 0.0)));
    if let Some((l, c)) = meter {
        level_bar(ui, r.rect, l, c);
    }
    r.clicked()
}

/// 0..1 display level of an input channel entry (dB scaled).
fn chan_level(app: &DawApp, e: ChanEntry) -> (f32, bool) {
    let get = |c: u16| (app.input_meters.get(c as usize).copied().unwrap_or(0.0), app.input_clip.get(c as usize).copied().unwrap_or(false));
    let (mut p, mut c) = get(e.ch);
    if e.stereo {
        let (p2, c2) = get(e.ch + 1);
        p = p.max(p2);
        c |= c2;
    }
    (((daw_project::gain_to_db(p) + 60.0) / 60.0).clamp(0.0, 1.0), c)
}

/// MIDI activity (age in seconds) → 0..1 meter level.
pub(super) fn activity_level(age: f32) -> f32 {
    (1.0 - age / 0.4).clamp(0.0, 1.0)
}

fn ext_label(cfg: &ChannelConfig, ch: u16, stereo: bool) -> String {
    cfg.label(ChanEntry { ch, stereo })
}

/// Track labels other tracks can route to/from (excluding `t`).
fn other_tracks(app: &DawApp, t: usize, f: impl Fn(&daw_project::Track) -> bool) -> Vec<(u64, String)> {
    let p = &app.project;
    p.tracks.iter().enumerate().filter(|&(i, x)| i != t && f(x)).map(|(i, x)| (x.id, p.track_label(i))).collect()
}

fn track_name(app: &DawApp, id: u64) -> String {
    app.project.tracks.iter().position(|x| x.id == id).map_or_else(|| "?".into(), |i| app.project.track_label(i))
}

/// The In/Out block of mixer column `t`: input type + channel, monitor radio,
/// output type + channel. Changes apply to every selected track of the same kind.
pub fn io_section(app: &mut DawApp, ui: &mut egui::Ui, t: usize, w: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(w, IO_H), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(3), gray(38));
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(4.0, 3.0))).layout(egui::Layout::top_down(egui::Align::Min)));
    let ui = &mut child;
    ui.spacing_mut().item_spacing = vec2(2.0, 1.0);
    ui.spacing_mut().interact_size.y = 16.0;
    let cw = w - 8.0;
    let Some(track) = app.project.tracks.get(t) else { return };
    let kind = track.kind;
    if kind != TrackKind::Group {
        caption(ui, if kind == TrackKind::Audio { "Audio From" } else { "MIDI From" });
        if kind == TrackKind::Audio {
            audio_from(app, ui, t, cw, true);
        } else {
            midi_from(app, ui, t, cw, true);
        }
        caption(ui, "Monitor");
        monitor_radio(app, ui, t, cw);
    } else {
        ui.add_space(68.0);
    }
    let sends_midi = app.project.tracks[t].sends_midi();
    caption(ui, if sends_midi { "MIDI To" } else { "Audio To" });
    if sends_midi {
        midi_to(app, ui, t, cw, true);
    } else {
        audio_to(app, ui, t, cw, true);
    }
}

/// Arrangement header variant: no captions, clipped to the row (shows as many
/// choosers as the track height allows, Live order).
pub fn io_compact(app: &mut DawApp, ui: &mut egui::Ui, t: usize, rect: Rect, clip: Rect) {
    let bg = if app.is_track_selected(t) { super::theme::sel() } else { super::theme::panel() };
    ui.painter().with_clip_rect(clip).rect_filled(rect, CornerRadius::ZERO, bg);
    let Some(track) = app.project.tracks.get(t) else { return };
    let (kind, collapsed) = (track.kind, track.collapsed);
    // Rows that fit, by priority: from-type, to-type, monitor, from-channel,
    // to-channel (shown in Live order). Narrow column: smaller text.
    let fit = (((rect.height() - 4.0) / 17.0).floor().max(0.0) as usize).min(if collapsed { 1 } else { 5 });
    if fit == 0 || rect.width() < 24.0 {
        return;
    }
    let group = kind == TrackKind::Group;
    let (in_t, out_t, mon, in_ch, out_ch) =
        if group { (false, fit >= 1, false, false, fit >= 2) } else { (true, fit >= 2, fit >= 3, fit >= 4, fit >= 5) };
    let pad = 2.0;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(rect.min + vec2(pad, 2.0), rect.max - vec2(pad, 0.0)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(clip.intersect(rect));
    let ui = &mut child;
    // Live: 13 px controls on a 17 px pitch.
    ui.spacing_mut().item_spacing = vec2(2.0, 4.0);
    ui.spacing_mut().interact_size.y = 13.0;
    ui.spacing_mut().button_padding = vec2(3.0, 0.0);
    ui.spacing_mut().combo_width = 0.0;
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
    let cw = rect.width() - 2.0 * pad;
    if in_t {
        if kind == TrackKind::Audio {
            audio_from(app, ui, t, cw, in_ch);
        } else {
            midi_from(app, ui, t, cw, in_ch);
        }
    }
    if mon {
        monitor_radio(app, ui, t, cw);
    }
    if out_t {
        if app.project.tracks[t].sends_midi() {
            midi_to(app, ui, t, cw, out_ch);
        } else {
            audio_to(app, ui, t, cw, out_ch);
        }
    }
}

/// Arrangement master row: main output channels.
/// Like a track's choosers: 13 px tall, vertically centred while collapsed.
pub fn master_io_compact(app: &mut DawApp, ui: &mut egui::Ui, rect: Rect, centred: bool) {
    let y = if centred { (rect.center().y - 6.5).round() } else { rect.min.y + 2.0 };
    let r = Rect::from_min_max(pos2(rect.min.x + 2.0, y), pos2(rect.max.x - 2.0, rect.max.y));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.spacing_mut().interact_size.y = 13.0;
    child.spacing_mut().button_padding = vec2(3.0, 0.0);
    child.spacing_mut().combo_width = 0.0;
    master_out_combo(app, &mut child, rect.width() - 4.0);
}

fn audio_from(app: &mut DawApp, ui: &mut egui::Ui, t: usize, cw: f32, ch: bool) {
    let cur = app.project.tracks[t].audio_in;
    let type_text = match cur {
        AudioIn::None => "No Input".to_string(),
        AudioIn::Ext { .. } => "Ext. In".into(),
        AudioIn::Resampling => "Resampling".into(),
        AudioIn::Track { id, .. } => track_name(app, id),
    };
    let sources = other_tracks(app, t, |x| x.kind != TrackKind::Midi || !x.sends_midi());
    let mut pick: Option<AudioIn> = None;
    egui::ComboBox::from_id_salt(("ain", t))
        .wrap_mode(egui::TextWrapMode::Truncate)
        .width(cw)
        .height(420.0)
        .selected_text(small(type_text))
        .show_ui(ui, |ui| {
            if entry(ui, matches!(cur, AudioIn::Ext { .. }), "Ext. In", None) {
                pick = Some(if matches!(cur, AudioIn::Ext { .. }) { cur } else { AudioIn::default() });
            }
            if entry(ui, cur == AudioIn::Resampling, "Resampling", None) {
                pick = Some(AudioIn::Resampling);
            }
            if entry(ui, cur == AudioIn::None, "No Input", None) {
                pick = Some(AudioIn::None);
            }
            ui.separator();
            for (id, name) in &sources {
                if entry(ui, matches!(cur, AudioIn::Track { id: i, .. } if i == *id), name, None) {
                    pick = Some(AudioIn::Track { id: *id, tap: Tap::PostFx });
                }
            }
        });
    // Channel row hidden (short track): still run it invisibly so picks apply.
    let mut hidden = ui.new_child(egui::UiBuilder::new().invisible().max_rect(ui.max_rect()));
    let ui: &mut egui::Ui = if ch { ui } else { &mut hidden };
    // Channel / tap point.
    match cur {
        AudioIn::Ext { ch, stereo } => {
            let cfg = app.channel_config(true);
            let n = app.audio.info.input_channels;
            let entries = cfg.entries(n);
            let mut configure = false;
            let cr = egui::ComboBox::from_id_salt(("ainch", t))
                .wrap_mode(egui::TextWrapMode::Truncate)
                .width(cw)
                .height(420.0)
                .selected_text(metered(ui, ext_label(&cfg, ch, stereo)))
                .show_ui(ui, |ui| {
                    if n == 0 {
                        ui.label(small("No input device"));
                    }
                    for e in entries {
                        let lv = chan_level(app, e);
                        if entry(ui, e.ch == ch && e.stereo == stereo, &cfg.label(e), Some(lv)) {
                            pick = Some(AudioIn::Ext { ch: e.ch, stereo: e.stereo });
                        }
                    }
                    ui.separator();
                    if entry(ui, false, "Configure…", None) {
                        configure = true;
                    }
                });
            // Live: small stereo / mono input meter left of the channel name.
            let lv = |c: u16| {
                (app.input_meters.get(c as usize).copied().unwrap_or(0.0), app.input_clip.get(c as usize).copied().unwrap_or(false))
            };
            let peaks: Vec<(f32, bool)> = if stereo { vec![lv(ch), lv(ch + 1)] } else { vec![lv(ch)] };
            input_meter(&ui.painter_at(cr.response.rect), meter_slot(cr.response.rect), &peaks);
            if configure {
                app.open_settings(SettingsTab::Audio);
            }
        }
        AudioIn::Track { id, tap } => {
            let pads: Vec<String> = match app.project.tracks.iter().find(|x| x.id == id).map(|x| &x.instrument) {
                Some(InstrumentDef::Drums { pads }) => pads.iter().map(|p| p.name.clone()).collect(),
                _ => vec![],
            };
            let tap_text = |tp: Tap| match tp {
                Tap::PreFx => "Pre FX".to_string(),
                Tap::PostFx => "Post FX".into(),
                Tap::PostMixer => "Post Mixer".into(),
                Tap::Pad(i) => format!("{} {}", i + 1, pads.get(i as usize).map_or("", |s| s.as_str())),
            };
            egui::ComboBox::from_id_salt(("ainch", t))
                .wrap_mode(egui::TextWrapMode::Truncate)
                .width(cw)
                .height(420.0)
                .selected_text(small(tap_text(tap)))
                .show_ui(ui, |ui| {
                    for tp in [Tap::PreFx, Tap::PostFx, Tap::PostMixer] {
                        if entry(ui, tap == tp, &tap_text(tp), None) {
                            pick = Some(AudioIn::Track { id, tap: tp });
                        }
                    }
                    if !pads.is_empty() {
                        ui.separator();
                        for i in 0..pads.len().min(u8::MAX as usize) {
                            let tp = Tap::Pad(i as u8);
                            if entry(ui, tap == tp, &tap_text(tp), None) {
                                pick = Some(AudioIn::Track { id, tap: tp });
                            }
                        }
                    }
                });
        }
        _ => {
            ui.add_space(17.0);
        }
    }
    if let Some(v) = pick
        && v != cur
    {
        app.set_routing(t, |x| x.audio_in = v);
    }
}

fn midi_from(app: &mut DawApp, ui: &mut egui::Ui, t: usize, cw: f32, ch: bool) {
    let cur = app.project.tracks[t].midi_in.clone();
    let ports = app.io_map.midi_in.clone();
    let type_text = match &cur {
        MidiIn::None => "No Input".to_string(),
        MidiIn::AllIns => "All Ins".into(),
        MidiIn::Keyboard => "Computer Keyboard".into(),
        MidiIn::Port(n) if ports.contains(n) => n.clone(),
        MidiIn::Port(n) => format!("{n} (off)"),
        MidiIn::Track(id) => track_name(app, *id),
    };
    let act = app.midi.activity.clone();
    let any_port = |p: Option<usize>, ch: Option<u8>| match p {
        Some(p) => act.port_age(p, ch),
        None => (0..ports.len()).map(|p| act.port_age(p, ch)).fold(f32::INFINITY, f32::min),
    };
    let sources = other_tracks(app, t, |x| x.kind == TrackKind::Midi);
    let mut pick: Option<MidiIn> = None;
    egui::ComboBox::from_id_salt(("min", t))
        .wrap_mode(egui::TextWrapMode::Truncate)
        .width(cw)
        .height(420.0)
        .selected_text(small(type_text))
        .show_ui(ui, |ui| {
            if entry(ui, cur == MidiIn::AllIns, "All Ins", Some((activity_level(any_port(None, None)), false))) {
                pick = Some(MidiIn::AllIns);
            }
            if entry(ui, cur == MidiIn::Keyboard, "Computer Keyboard", None) {
                pick = Some(MidiIn::Keyboard);
            }
            for (i, n) in ports.iter().enumerate() {
                if entry(ui, cur == MidiIn::Port(n.clone()), n, Some((activity_level(any_port(Some(i), None)), false))) {
                    pick = Some(MidiIn::Port(n.clone()));
                }
            }
            if entry(ui, cur == MidiIn::None, "No Input", None) {
                pick = Some(MidiIn::None);
            }
            ui.separator();
            for (id, name) in &sources {
                if entry(ui, cur == MidiIn::Track(*id), name, None) {
                    pick = Some(MidiIn::Track(*id));
                }
            }
            ui.separator();
            if entry(ui, false, "Configure…", None) {
                pick = None;
                app.dialog = Some(crate::app::Dialog::Settings(SettingsTab::Midi));
            }
        });
    // Channel row hidden (short track): still run it invisibly so picks apply.
    let mut hidden = ui.new_child(egui::UiBuilder::new().invisible().max_rect(ui.max_rect()));
    let ui: &mut egui::Ui = if ch { ui } else { &mut hidden };
    // Channel chooser for external ports (and All Ins).
    let port = match &cur {
        MidiIn::Port(n) => ports.iter().position(|p| p == n),
        _ => None,
    };
    if matches!(cur, MidiIn::AllIns | MidiIn::Port(_) | MidiIn::Keyboard) {
        let ch = app.project.tracks[t].midi_channel;
        let label = |c: u8| if c == 0 { "All Channels".to_string() } else { format!("Ch. {c}") };
        let mut new_ch = None;
        let cr = egui::ComboBox::from_id_salt(("minch", t))
            .wrap_mode(egui::TextWrapMode::Truncate)
            .width(cw)
            .height(420.0)
            .selected_text(metered(ui, label(ch)))
            .show_ui(ui, |ui| {
                for c in 0..=16u8 {
                    let lv = if cur == MidiIn::Keyboard {
                        None
                    } else {
                        Some((activity_level(any_port(port, if c == 0 { None } else { Some(c - 1) })), false))
                    };
                    if entry(ui, ch == c, &label(c), lv) {
                        new_ch = Some(c);
                    }
                }
            });
        // Live: MIDI activity cells left of the channel name.
        let age = if cur == MidiIn::Keyboard { act.age(&act.track_in) } else { any_port(port, if ch == 0 { None } else { Some(ch - 1) }) };
        let slot = meter_slot(cr.response.rect);
        midi_meter(&ui.painter_at(cr.response.rect), Rect::from_min_size(slot.min, vec2(3.0, 9.0)), activity_level(age));
        if let Some(c) = new_ch
            && c != ch
        {
            app.set_routing(t, |x| x.midi_channel = c);
        }
    } else {
        ui.add_space(17.0);
    }
    if let Some(v) = pick
        && v != cur
    {
        app.set_routing(t, move |x| x.midi_in = v.clone());
    }
}

/// In / Auto / Off. Right-click: Keep Monitoring Latency, Return to Default;
/// Delete while hovering returns to the default too.
fn monitor_radio(app: &mut DawApp, ui: &mut egui::Ui, t: usize, cw: f32) {
    let tr = &app.project.tracks[t];
    let (mode, keep, kind) = (tr.monitor_mode(), tr.keep_monitor_latency, tr.kind);
    let bw = (cw - 4.0) / 3.0;
    let mut set = None;
    let r = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (m, label) in [(Monitor::In, "In"), (Monitor::Auto, "Auto"), (Monitor::Off, "Off")] {
            let on = mode == m;
            let fill = match (on, m) {
                (true, Monitor::In) => MON_BLUE,
                (true, _) => ACCENT,
                _ => gray(58),
            };
            let txt = RichText::new(label).size(super::theme::FS - 1.0).color(if on { Color32::BLACK } else { Color32::LIGHT_GRAY });
            let b = ui.add(egui::Button::new(txt).fill(fill).min_size(vec2(bw, 15.0)));
            if b.clicked() {
                set = Some(Some(m));
            }
            b.context_menu(|ui| {
                if kind == TrackKind::Audio && m != Monitor::Off {
                    let mut k = keep;
                    if ui.checkbox(&mut k, "Keep Monitoring Latency in Recorded Audio").changed() {
                        app.set_routing(t, |x| x.keep_monitor_latency = k);
                        ui.close();
                    }
                }
                if ui.button("Return to Default   Del").clicked() {
                    set = Some(None);
                    ui.close();
                }
            });
        }
    });
    if r.response.contains_pointer() {
        app.hover_monitor = Some(t);
    }
    r.response.on_hover_text("Monitor: In = always, Auto = while armed and no clip plays, Off");
    if let Some(m) = set {
        app.set_monitor(t, m);
    }
}

fn audio_to(app: &mut DawApp, ui: &mut egui::Ui, t: usize, cw: f32, ch: bool) {
    let tr = &app.project.tracks[t];
    let cur = tr.audio_out;
    let parent = tr.group;
    let type_text = match cur {
        AudioOut::Default => parent.map_or_else(|| "Master".to_string(), |g| track_name(app, g)),
        AudioOut::Master => "Master".into(),
        AudioOut::Ext { .. } => "Ext. Out".into(),
        AudioOut::Track(id) => track_name(app, id),
    };
    let targets = other_tracks(app, t, |x| x.kind != TrackKind::Midi || !x.sends_midi());
    let mut pick: Option<AudioOut> = None;
    egui::ComboBox::from_id_salt(("aout", t))
        .wrap_mode(egui::TextWrapMode::Truncate)
        .width(cw)
        .height(420.0)
        .selected_text(small(type_text))
        .show_ui(ui, |ui| {
            if let Some(g) = parent
                && entry(ui, cur == AudioOut::Default, &format!("{} (Group)", track_name(app, g)), None)
            {
                pick = Some(AudioOut::Default);
            }
            let master_sel = cur == AudioOut::Master || (cur == AudioOut::Default && parent.is_none());
            if entry(ui, master_sel, "Master", None) {
                pick = Some(if parent.is_some() { AudioOut::Master } else { AudioOut::Default });
            }
            if entry(ui, matches!(cur, AudioOut::Ext { .. }), "Ext. Out", None) {
                pick = Some(if matches!(cur, AudioOut::Ext { .. }) { cur } else { AudioOut::Ext { ch: 0, stereo: true } });
            }
            ui.separator();
            for (id, name) in &targets {
                if Some(*id) == parent {
                    continue;
                }
                if entry(ui, cur == AudioOut::Track(*id), name, None) {
                    pick = Some(AudioOut::Track(*id));
                }
            }
        });
    // Channel row hidden (short track): still run it invisibly so picks apply.
    let mut hidden = ui.new_child(egui::UiBuilder::new().invisible().max_rect(ui.max_rect()));
    let ui: &mut egui::Ui = if ch { ui } else { &mut hidden };
    match cur {
        AudioOut::Ext { ch, stereo } => {
            let cfg = app.channel_config(false);
            let mut configure = false;
            egui::ComboBox::from_id_salt(("aoutch", t))
                .wrap_mode(egui::TextWrapMode::Truncate)
                .width(cw)
                .height(420.0)
                .selected_text(small(ext_label(&cfg, ch, stereo)))
                .show_ui(ui, |ui| {
                    for e in cfg.entries(app.audio.info.channels) {
                        if entry(ui, e.ch == ch && e.stereo == stereo, &cfg.label(e), None) {
                            pick = Some(AudioOut::Ext { ch: e.ch, stereo: e.stereo });
                        }
                    }
                    ui.separator();
                    if entry(ui, false, "Configure…", None) {
                        configure = true;
                    }
                });
            if configure {
                app.open_settings(SettingsTab::Audio);
            }
        }
        AudioOut::Track(_) => {
            ui.add_enabled(false, egui::Button::new(small("Track In")).min_size(vec2(cw, 16.0)));
        }
        _ => {
            ui.add_space(17.0);
        }
    }
    if let Some(v) = pick
        && v != cur
    {
        app.set_routing(t, |x| x.audio_out = v);
    }
}

fn midi_to(app: &mut DawApp, ui: &mut egui::Ui, t: usize, cw: f32, ch: bool) {
    let cur = app.project.tracks[t].midi_out.clone();
    let ports = app.io_map.midi_out.clone();
    let type_text = match &cur {
        MidiOut::None => "No Output".to_string(),
        MidiOut::Port { name, .. } if ports.contains(name) => name.clone(),
        MidiOut::Port { name, .. } => format!("{name} (off)"),
        MidiOut::Track { id, .. } => track_name(app, *id),
    };
    let targets = other_tracks(app, t, |x| x.kind == TrackKind::Midi);
    let mut pick: Option<MidiOut> = None;
    egui::ComboBox::from_id_salt(("mout", t))
        .wrap_mode(egui::TextWrapMode::Truncate)
        .width(cw)
        .height(420.0)
        .selected_text(small(type_text))
        .show_ui(ui, |ui| {
            if entry(ui, cur == MidiOut::None, "No Output", None) {
                pick = Some(MidiOut::None);
            }
            for n in &ports {
                if entry(ui, matches!(&cur, MidiOut::Port { name, .. } if name == n), n, None) {
                    pick = Some(MidiOut::Port { name: n.clone(), channel: 1 });
                }
            }
            ui.separator();
            for (id, name) in &targets {
                if entry(ui, matches!(cur, MidiOut::Track { id: i, .. } if i == *id), name, None) {
                    pick = Some(MidiOut::Track { id: *id, instrument: false });
                }
            }
            ui.separator();
            if entry(ui, false, "Configure…", None) {
                app.dialog = Some(crate::app::Dialog::Settings(SettingsTab::Midi));
            }
        });
    // Channel row hidden (short track): still run it invisibly so picks apply.
    let mut hidden = ui.new_child(egui::UiBuilder::new().invisible().max_rect(ui.max_rect()));
    let ui: &mut egui::Ui = if ch { ui } else { &mut hidden };
    match &cur {
        MidiOut::Port { name, channel } => {
            let (name, channel) = (name.clone(), *channel);
            egui::ComboBox::from_id_salt(("moutch", t))
                .wrap_mode(egui::TextWrapMode::Truncate)
                .width(cw)
                .height(420.0)
                .selected_text(small(format!("Ch. {channel}")))
                .show_ui(ui, |ui| {
                    for c in 1..=16u8 {
                        if entry(ui, c == channel, &format!("Ch. {c}"), None) {
                            pick = Some(MidiOut::Port { name: name.clone(), channel: c });
                        }
                    }
                });
        }
        MidiOut::Track { id, instrument } => {
            let (id, instrument) = (*id, *instrument);
            let inst = app.project.tracks.iter().find(|x| x.id == id).map_or("", |x| x.instrument.label());
            let text = |i: bool| if i { inst.to_string() } else { "Track In".to_string() };
            egui::ComboBox::from_id_salt(("moutch", t))
                .wrap_mode(egui::TextWrapMode::Truncate)
                .width(cw)
                .height(420.0)
                .selected_text(small(text(instrument)))
                .show_ui(ui, |ui| {
                    if entry(ui, !instrument, "Track In", None) {
                        pick = Some(MidiOut::Track { id, instrument: false });
                    }
                    if !inst.is_empty() && inst != "None" && entry(ui, instrument, inst, None) {
                        pick = Some(MidiOut::Track { id, instrument: true });
                    }
                });
        }
        MidiOut::None => {
            ui.add_space(17.0);
        }
    }
    if let Some(v) = pick
        && v != cur
    {
        app.set_routing(t, move |x| x.midi_out = v.clone());
    }
}

/// Master column In/Out block: the main output's device channels.
pub fn master_io(app: &mut DawApp, ui: &mut egui::Ui, w: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(w, IO_H), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(3), gray(38));
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(4.0, 3.0))).layout(egui::Layout::top_down(egui::Align::Min)));
    let ui = &mut child;
    ui.spacing_mut().item_spacing = vec2(2.0, 1.0);
    ui.add_space(IO_H - 44.0);
    caption(ui, "Master Out");
    master_out_combo(app, ui, w - 8.0);
}

fn master_out_combo(app: &mut DawApp, ui: &mut egui::Ui, cw: f32) {
    let cfg = app.channel_config(false);
    let (ch, stereo) = (app.project.master_out_ch, app.project.master_out_stereo);
    let mut pick = None;
    let mut configure = false;
    let cr = egui::ComboBox::from_id_salt("master_out")
        .wrap_mode(egui::TextWrapMode::Truncate)
        .width(cw)
        .height(420.0)
        .selected_text(metered(ui, ext_label(&cfg, ch, stereo)))
        .show_ui(ui, |ui| {
            for e in cfg.entries(app.audio.info.channels) {
                if entry(ui, e.ch == ch && e.stereo == stereo, &cfg.label(e), None) {
                    pick = Some(e);
                }
            }
            ui.separator();
            if entry(ui, false, "Configure…", None) {
                configure = true;
            }
        });
    // Output level meter left of the channel name (Live).
    let (m, c) = (app.meter, app.clipped);
    let peaks: Vec<(f32, bool)> = if stereo { vec![(m[0], c), (m[1], c)] } else { vec![(m[0].max(m[1]), c)] };
    input_meter(&ui.painter_at(cr.response.rect), meter_slot(cr.response.rect), &peaks);
    if configure {
        app.open_settings(SettingsTab::Audio);
    }
    if let Some(e) = pick {
        app.edit(|p| {
            p.master_out_ch = e.ch;
            p.master_out_stereo = e.stereo;
        });
    }
}

// ----- Settings dialog --------------------------------------------------------

/// Settings body. Device changes restart the audio engine right away.
/// Returns true to close.
fn combo_opt(ui: &mut egui::Ui, id: &str, label: &str, cur: &Option<String>, list: &[String], none: &str) -> Option<Option<String>> {
    let mut out = None;
    ui.label(label);
    egui::ComboBox::from_id_salt(id).width(300.0).selected_text(cur.clone().unwrap_or_else(|| none.to_string())).show_ui(ui, |ui| {
        if ui.selectable_label(cur.is_none(), none).clicked() {
            out = Some(None);
        }
        for n in list {
            if ui.selectable_label(cur.as_deref() == Some(n.as_str()), n).clicked() {
                out = Some(Some(n.clone()));
            }
        }
    });
    ui.end_row();
    out
}

pub(super) fn audio_settings(app: &mut DawApp, ui: &mut egui::Ui) {
    if app.device_lists.is_none() {
        let hosts = crate::audio::host_names();
        let (outs, ins) = crate::audio::device_names(app.settings.host.as_deref());
        app.device_lists = Some((hosts, outs, ins));
    }
    let (hosts, outs, ins) = app.device_lists.clone().unwrap_or_default();
    let mut changed = false;
    egui::Grid::new("audio_settings").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        if let Some(v) = combo_opt(ui, "set_host", "Driver Type", &app.settings.host, &hosts, "Default") {
            app.settings.host = v;
            app.settings.output_device = None;
            app.settings.input_device = None;
            app.device_lists = None;
            changed = true;
        }
        if let Some(v) = combo_opt(ui, "set_out", "Audio Output Device", &app.settings.output_device, &outs, "Default") {
            app.settings.output_device = v;
            changed = true;
        }
        ui.label("Audio Input Device");
        let cur_in = if app.settings.no_input {
            "No Device".to_string()
        } else {
            app.settings.input_device.clone().unwrap_or_else(|| "Default".into())
        };
        egui::ComboBox::from_id_salt("set_in").width(300.0).selected_text(cur_in).show_ui(ui, |ui| {
            if ui.selectable_label(app.settings.no_input, "No Device").clicked() {
                app.settings.no_input = true;
                changed = true;
            }
            if ui.selectable_label(!app.settings.no_input && app.settings.input_device.is_none(), "Default").clicked() {
                app.settings.no_input = false;
                app.settings.input_device = None;
                changed = true;
            }
            for n in &ins {
                if ui.selectable_label(!app.settings.no_input && app.settings.input_device.as_deref() == Some(n.as_str()), n).clicked() {
                    app.settings.no_input = false;
                    app.settings.input_device = Some(n.clone());
                    changed = true;
                }
            }
        });
        ui.end_row();
        ui.label("Buffer Size");
        let bf = app.settings.buffer_frames;
        egui::ComboBox::from_id_salt("set_buf")
            .width(300.0)
            .selected_text(bf.map_or_else(|| "Default".to_string(), |b| format!("{b} samples")))
            .show_ui(ui, |ui| {
                for v in [None, Some(64), Some(128), Some(256), Some(512), Some(1024), Some(2048)] {
                    let text = v.map_or_else(|| "Default".to_string(), |b| format!("{b} samples"));
                    if ui.selectable_label(bf == v, text).clicked() && bf != v {
                        app.settings.buffer_frames = v;
                        changed = true;
                    }
                }
            });
        ui.end_row();
        // Sample rate: recordings in memory are resampled, sample positions move along.
        ui.label("Sample Rate");
        let cur = app.settings.sample_rate;
        let now = app.audio.info.sample_rate;
        egui::ComboBox::from_id_salt("set_sr")
            .width(300.0)
            .selected_text(cur.map_or_else(|| format!("Default ({now} Hz)"), |r| format!("{r} Hz")))
            .show_ui(ui, |ui| {
                for v in [None, Some(44_100), Some(48_000), Some(88_200), Some(96_000), Some(176_400), Some(192_000)] {
                    let text = v.map_or_else(|| "Default".to_string(), |r| format!("{r} Hz"));
                    if ui.selectable_label(cur == v, text).clicked() && cur != v {
                        app.settings.sample_rate = v;
                        changed = true;
                    }
                }
            });
        ui.end_row();
    });
    let info = &app.audio.info;
    let sr = info.sample_rate.max(1) as f32;
    let ms = |f: usize| f as f32 * 1000.0 / sr;
    ui.label(
        RichText::new(format!(
            "{} · {} Hz · out {} ch · in {} ch\nOutput latency {:.1} ms · Overall latency {:.1} ms",
            info.device,
            info.sample_rate,
            info.channels,
            info.input_channels,
            ms(info.output_latency_frames()),
            ms(info.round_trip_frames()),
        ))
        .small()
        .color(Color32::GRAY),
    );
    if let Some(r) = &info.null_reason {
        ui.colored_label(Color32::LIGHT_RED, r);
    }
    if changed {
        app.save_settings();
        let opts = app.options_from_settings();
        app.restart_audio(opts);
    }
    // Channel configuration (Live: Input Config / Output Config).
    let open_id = ui.make_persistent_id("chan_cfg_open");
    let mut open: Option<bool> = ui.data(|d| d.get_temp(open_id)).flatten();
    ui.horizontal(|ui| {
        if ui.selectable_label(open == Some(true), "Input Config").clicked() {
            open = if open == Some(true) { None } else { Some(true) };
        }
        if ui.selectable_label(open == Some(false), "Output Config").clicked() {
            open = if open == Some(false) { None } else { Some(false) };
        }
    });
    ui.data_mut(|d| d.insert_temp(open_id, open));
    if let Some(input) = open {
        channel_config_editor(app, ui, input);
    }
}

/// Enable mono/stereo channels and name them (Tab moves between names).
fn channel_config_editor(app: &mut DawApp, ui: &mut egui::Ui, input: bool) {
    let info = &app.audio.info;
    let (dev, n) = if input { (info.input.clone().unwrap_or_default(), info.input_channels) } else { (info.device.clone(), info.channels) };
    ui.label(
        RichText::new(format!(
            "{} channels of “{}”: enable mono / stereo pairs, click a name to rename.",
            if input { "Input" } else { "Output" },
            dev
        ))
        .small(),
    );
    if n == 0 {
        ui.label("No device.");
        return;
    }
    let key = Settings::device_key(input, &dev);
    let mut cfg = app.settings.channels.get(&key).cloned().unwrap_or_default();
    let mut changed = false;
    let all = ChannelConfig::all_entries(n);
    let count = all.len();
    let base = ui.make_persistent_id(("chan_name", input));
    let name_id = |i: usize| base.with(i);
    let mut jump = None;
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        egui::Grid::new(("chan_cfg", input)).num_columns(3).spacing([8.0, 3.0]).show(ui, |ui| {
            for (idx, e) in all.into_iter().enumerate() {
                let mut on = cfg.enabled(e);
                if ui.checkbox(&mut on, "").changed() {
                    cfg.set_enabled(e, on);
                    changed = true;
                }
                ui.label(format!("{} {}", e.number(), if e.stereo { "(stereo)" } else { "(mono)" }));
                let mut name = cfg.names.get(&e.key()).cloned().unwrap_or_default();
                // Tab / Shift+Tab move between the name fields (not the checkboxes).
                let r =
                    ui.add(egui::TextEdit::singleline(&mut name).id(name_id(idx)).desired_width(160.0).hint_text("name").lock_focus(true));
                if r.has_focus() {
                    let (tab, shift) = ui.input(|i| (i.key_pressed(egui::Key::Tab), i.modifiers.shift));
                    if tab {
                        jump = Some(if shift { (idx + count - 1) % count } else { (idx + 1) % count });
                    }
                }
                if r.changed() {
                    if name.is_empty() {
                        cfg.names.remove(&e.key());
                    } else {
                        cfg.names.insert(e.key(), name);
                    }
                    changed = true;
                }
                ui.end_row();
            }
        });
    });
    if let Some(i) = jump {
        ui.memory_mut(|m| m.request_focus(name_id(i)));
    }
    if changed {
        app.settings.channels.insert(key, cfg);
        app.save_settings();
    }
}

pub(super) fn midi_settings(app: &mut DawApp, ui: &mut egui::Ui) {
    if let Some(e) = &app.midi.error {
        ui.colored_label(Color32::LIGHT_RED, e);
    }
    let mut changed = false;
    let act = app.midi.activity.clone();
    let open_in: Vec<String> = app.io_map.midi_in.clone();
    for (inputs, title) in [(true, "Input"), (false, "Output")] {
        let ports = if inputs { app.midi.inputs.clone() } else { app.midi.outputs.clone() };
        egui::Grid::new(("midi_ports", inputs)).num_columns(3).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
            ui.label(RichText::new(format!("MIDI Ports ({title})")).strong());
            ui.label("Track");
            ui.label("");
            ui.end_row();
            if ports.is_empty() {
                ui.label(RichText::new("none").color(Color32::GRAY));
                ui.end_row();
            }
            for p in &ports {
                ui.label(format!("{title}: {}", p.name));
                let mut on = p.track;
                if ui.checkbox(&mut on, "").changed() {
                    let map = if inputs { &mut app.settings.midi_in } else { &mut app.settings.midi_out };
                    map.insert(p.name.clone(), on);
                    changed = true;
                }
                // Activity / open state.
                let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                let lit = inputs && open_in.iter().position(|n| *n == p.name).is_some_and(|i| act.port_age(i, None) < 0.2);
                let col = if !p.open && p.track {
                    REC_RED
                } else if lit {
                    PLAY_GREEN
                } else if p.open {
                    gray(110)
                } else {
                    gray(50)
                };
                ui.painter().circle_filled(rect.center(), 4.0, col);
                ui.end_row();
            }
        });
        ui.add_space(6.0);
    }
    if ui.button("Rescan MIDI devices").clicked() {
        app.rescan_midi();
    }
    if changed {
        app.save_settings();
        app.rescan_midi();
    }
}
