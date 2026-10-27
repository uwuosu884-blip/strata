//! Browser: instruments, factory sounds and the file system. Items are dragged
//! onto tracks, slots, lanes or devices.

use super::widgets::*;
use crate::app::{DawApp, DragItem};
use eframe::egui::{self, Color32, Sense};
use std::path::PathBuf;

const AUDIO_EXT: [&str; 10] = ["wav", "flac", "mp3", "ogg", "opus", "aif", "aiff", "m4a", "aac", "wave"];

fn item(ui: &mut egui::Ui, label: &str, payload: DragItem, color: Color32) -> egui::Response {
    let r = ui.add(egui::Label::new(egui::RichText::new(label).color(color)).sense(Sense::click_and_drag()).selectable(false).truncate());
    r.dnd_set_drag_payload(payload);
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    r
}

pub fn browser(app: &mut DawApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Browser").strong());
    ui.label(egui::RichText::new("Drag items onto tracks, slots or devices").small().color(Color32::GRAY));
    ui.horizontal(|ui| {
        let r = ui.selectable_label(app.preview_on, "Preview").on_hover_text("Click a sound to hear it on the Cue bus (master Cue volume)");
        if r.clicked() {
            app.preview_on = !app.preview_on;
            if !app.preview_on {
                app.stop_preview();
            }
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().id_salt("browser").auto_shrink([false, false]).show(ui, |ui| {
        egui::CollapsingHeader::new("Instruments").default_open(true).show(ui, |ui| {
            item(ui, "Drum Rack (Factory Kit)", DragItem::DrumKit, ACCENT);
            for p in daw_engine::SynthParams::PRESETS {
                item(ui, &format!("Synth: {p}"), DragItem::SynthPreset(p.to_string()), Color32::from_gray(220));
            }
        });
        egui::CollapsingHeader::new("Factory Sounds").default_open(true).show(ui, |ui| {
            for n in daw_core::factory::NAMES {
                let key = format!("{}{n}", daw_project::FACTORY_PREFIX);
                let r = item(ui, n, DragItem::Sample { key: key.clone(), name: n.to_string() }, Color32::from_rgb(150, 200, 255));
                if r.clicked() && app.preview_on {
                    app.preview_sample(&key);
                }
            }
        });
        egui::CollapsingHeader::new("Files").default_open(true).show(ui, |ui| files(app, ui));
    });
}

fn files(app: &mut DawApp, ui: &mut egui::Ui) {
    let mut dir = app.browser_dir.to_string_lossy().into_owned();
    let r = ui.add(egui::TextEdit::singleline(&mut dir).desired_width(f32::INFINITY));
    if r.lost_focus() {
        let p = PathBuf::from(&dir);
        if p.is_dir() {
            app.browser_dir = p;
        }
    }
    if let Some(parent) = app.browser_dir.parent()
        && ui.selectable_label(false, ".. (up)").clicked()
    {
        app.browser_dir = parent.to_path_buf();
    }
    // Directory listing is re-read only when the folder changes (cached in egui memory).
    let id = ui.scope_id().with("listing");
    let cached: Option<(PathBuf, Vec<(bool, PathBuf)>)> = ui.data(|d| d.get_temp(id));
    let entries = match cached {
        Some((d, e)) if d == app.browser_dir => e,
        _ => {
            let mut e: Vec<(bool, PathBuf)> = std::fs::read_dir(&app.browser_dir)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| e.path())
                        .filter(|p| !p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.')))
                        .filter_map(|p| {
                            if p.is_dir() {
                                Some((true, p))
                            } else if p
                                .extension()
                                .and_then(|e| e.to_str())
                                .is_some_and(|e| AUDIO_EXT.contains(&e.to_ascii_lowercase().as_str()))
                            {
                                Some((false, p))
                            } else {
                                None
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            e.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            e.truncate(2000);
            ui.data_mut(|d| d.insert_temp(id, (app.browser_dir.clone(), e.clone())));
            e
        }
    };
    for (is_dir, path) in entries {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if is_dir {
            if ui.selectable_label(false, format!("{name}/")).clicked() {
                app.browser_dir = path;
            }
        } else {
            let key = path.to_string_lossy().into_owned();
            let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name.clone());
            let r = item(ui, &name, DragItem::Sample { key: key.clone(), name: stem }, Color32::from_rgb(150, 200, 255));
            if r.clicked() && app.preview_on {
                app.preview_sample(&key);
            }
            if let Some(e) = app.cache.error(&key) {
                r.on_hover_text(format!("Failed: {e}"));
            }
        }
    }
}
