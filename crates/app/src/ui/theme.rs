//! Live 12 look: palette, fonts and egui visuals, driven by Settings > Theme & Colors.
//!
//! All UI greys are written against Live's Default (dark, neutral) theme and go
//! through [`gray`], which applies Theme / Appearance / Tone / High Contrast /
//! Brightness / Color Intensity + Hue. Swapping the palette is one call per change.

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
use serde::{Deserialize, Serialize};
use std::cell::Cell;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeName {
    #[default]
    Default,
    Dark,
    MidDark,
    MidLight,
    Light,
}

impl ThemeName {
    pub const ALL: [ThemeName; 5] = [Self::Default, Self::Dark, Self::MidDark, Self::MidLight, Self::Light];
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Dark => "Dark",
            Self::MidDark => "Mid Dark",
            Self::MidLight => "Mid Light",
            Self::Light => "Light",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Appearance {
    #[default]
    FollowSystem,
    Light,
    Dark,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Self::FollowSystem, Self::Light, Self::Dark];
    pub fn label(self) -> &'static str {
        match self {
            Self::FollowSystem => "Follow System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Tone {
    #[default]
    Neutral,
    Cool,
    Warm,
}

impl Tone {
    pub const ALL: [Tone; 3] = [Self::Neutral, Self::Cool, Self::Warm];
    pub fn label(self) -> &'static str {
        match self {
            Self::Neutral => "Neutral",
            Self::Cool => "Cool",
            Self::Warm => "Warm",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ClipColorMode {
    #[default]
    TrackColor,
    Random,
}

impl ClipColorMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::TrackColor => "Track Color",
            Self::Random => "Random",
        }
    }
}

/// Settings > Theme & Colors (saved in settings.ron).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeSettings {
    pub theme: ThemeName,
    pub appearance: Appearance,
    pub tone: Tone,
    pub high_contrast: bool,
    /// 0..2 (100 % = 1).
    pub grid_intensity: f32,
    /// 0.5..1.5 (100 % = 1).
    pub brightness: f32,
    /// 0..1: how much the greys take on `color_hue`.
    pub color_intensity: f32,
    /// Degrees 0..360.
    pub color_hue: f32,
    pub auto_assign_colors: bool,
    /// Used when auto-assign is off (None = first palette colour).
    pub default_track_color: Option<[u8; 3]>,
    pub reduced_colors: bool,
    pub clip_color: ClipColorMode,
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self {
            theme: ThemeName::Default,
            appearance: Appearance::FollowSystem,
            tone: Tone::Neutral,
            high_contrast: false,
            grid_intensity: 1.0,
            brightness: 1.0,
            color_intensity: 0.0,
            color_hue: 0.0,
            auto_assign_colors: true,
            default_track_color: None,
            reduced_colors: false,
            clip_color: ClipColorMode::TrackColor,
        }
    }
}

/// Resolved per-frame palette parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    light: bool,
    scale: f32,
    contrast: f32,
    tone: [f32; 3],
    tint: [f32; 3],
    tint_amt: f32,
    grid: f32,
}

impl Default for Palette {
    fn default() -> Self {
        Self { light: false, scale: 1.0, contrast: 1.0, tone: [0.0; 3], tint: [1.0; 3], tint_amt: 0.0, grid: 1.0 }
    }
}

thread_local! {
    static PAL: Cell<Palette> = Cell::new(Palette::default());
}

fn hue_rgb(h: f32) -> [f32; 3] {
    let h = (h.rem_euclid(360.0)) / 60.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    match h as u32 {
        0 => [1.0, x, 0.0],
        1 => [x, 1.0, 0.0],
        2 => [0.0, 1.0, x],
        3 => [0.0, x, 1.0],
        4 => [x, 0.0, 1.0],
        _ => [1.0, 0.0, x],
    }
}

impl Palette {
    pub fn resolve(s: &ThemeSettings, system_dark: Option<bool>) -> Self {
        let light = match s.theme {
            ThemeName::Default => match s.appearance {
                Appearance::FollowSystem => system_dark == Some(false),
                Appearance::Light => true,
                Appearance::Dark => false,
            },
            ThemeName::Dark | ThemeName::MidDark => false,
            ThemeName::MidLight | ThemeName::Light => true,
        };
        let variant = match s.theme {
            ThemeName::Dark => 0.78,
            ThemeName::MidDark => 1.2,
            ThemeName::MidLight => 0.86,
            _ => 1.0,
        };
        let tone = match s.tone {
            Tone::Neutral => [0.0, 0.0, 0.0],
            Tone::Cool => [-3.0, 0.0, 6.0],
            Tone::Warm => [6.0, 2.0, -4.0],
        };
        Self {
            light,
            scale: variant * s.brightness.clamp(0.5, 1.5),
            contrast: if s.high_contrast { 1.3 } else { 1.0 },
            tone,
            tint: hue_rgb(s.color_hue),
            tint_amt: s.color_intensity.clamp(0.0, 1.0) * 0.45,
            grid: s.grid_intensity.clamp(0.0, 2.0),
        }
    }

    fn map(&self, n: u8) -> Color32 {
        let mut v = n as f32;
        if self.light {
            v = 255.0 - (v - 20.0) * 1.22;
        }
        v = 128.0 + (v - 128.0) * self.contrast;
        v *= self.scale;
        let k = v / 128.0;
        let mut c = [v + self.tone[0] * k, v + self.tone[1] * k, v + self.tone[2] * k];
        if self.tint_amt > 0.0 {
            for (ch, t) in c.iter_mut().zip(self.tint) {
                *ch += ((v * (0.55 + 0.9 * t)) - *ch) * self.tint_amt;
            }
        }
        let q = |x: f32| x.round().clamp(0.0, 255.0) as u8;
        Color32::from_rgb(q(c[0]), q(c[1]), q(c[2]))
    }
}

pub fn set_palette(p: Palette) {
    PAL.with(|c| c.set(p));
}

pub fn palette() -> Palette {
    PAL.with(|c| c.get())
}

/// A UI grey (authored for Live's Default dark theme) in the current theme.
pub fn gray(n: u8) -> Color32 {
    palette().map(n)
}

/// Grid line between lane background `bg` and line grey `n`, by Grid Line Intensity.
pub fn grid(n: u8, bg: u8) -> Color32 {
    let k = palette().grid;
    let v = (bg as f32 + (n as f32 - bg as f32) * k).clamp(0.0, 255.0) as u8;
    gray(v)
}

/// Light themes: true (dark text on light greys).
pub fn is_light() -> bool {
    palette().light
}

// ---- Live 12 Default (dark, neutral) metrics, measured at 1920×1080 ----

/// Window background between panels.
pub fn win_bg() -> Color32 {
    gray(0x2a)
}
/// Panel background (browser, headers, detail, status bar).
pub fn panel() -> Color32 {
    gray(0x36)
}
/// Darker lane / alternate bar shade.
pub fn lane() -> Color32 {
    gray(0x32)
}
/// Separators and value-box / chooser fills.
pub fn sep() -> Color32 {
    gray(0x24)
}
/// Selected track header / hover.
pub fn sel() -> Color32 {
    gray(0x46)
}
/// 1 px control outline.
pub fn outline() -> Color32 {
    gray(0x11)
}
/// Off toggle fill.
pub fn off_fill() -> Color32 {
    gray(0x1e)
}
/// Focused panel outline.
pub fn focus() -> Color32 {
    gray(0x75)
}
/// Default text.
pub fn text() -> Color32 {
    gray(0xb5)
}
/// Dim text (disabled controls, labels).
pub fn dim() -> Color32 {
    gray(0x75)
}

pub const ORANGE: Color32 = Color32::from_rgb(0xff, 0xad, 0x56);
/// Volume slider fill (teal) and Cue fill (blue).
pub const VOL_TEAL: Color32 = Color32::from_rgb(0x00, 0x73, 0x83);
pub const CUE_BLUE: Color32 = Color32::from_rgb(0x4a, 0x6c, 0xc8);
/// Selected browser row.
pub const SEL_ROW: Color32 = Color32::from_rgb(0x63, 0x7e, 0x86);
/// Clip / selection light blue (selected audio clip body).
pub const SEL_BLUE: Color32 = Color32::from_rgb(0xb4, 0xdc, 0xe8);

/// Font sizes (Live: ~11 px UI text).
pub const FS: f32 = 11.0;
pub const FS_SMALL: f32 = 9.5;

pub fn font(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// Inter (OFL) embedded: Medium for UI text, SemiBold for titles.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("inter".into(), FontData::from_static(include_bytes!("../../assets/Inter-Medium.ttf")).into());
    fonts.font_data.insert("inter-bold".into(), FontData::from_static(include_bytes!("../../assets/Inter-SemiBold.ttf")).into());
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "inter".into());
    let mut b = vec!["inter-bold".to_string()];
    b.extend(fonts.families[&FontFamily::Proportional].iter().skip(1).cloned());
    fonts.families.insert(FontFamily::Name("bold".into()), b);
    ctx.set_fonts(fonts);
}

/// egui widgets in Live colours: flat boxes, 1 px outlines, compact spacing.
pub fn apply_style(ctx: &egui::Context) {
    use egui::{CornerRadius, Stroke};
    let light = is_light();
    let mut st = (*ctx.global_style()).clone();
    st.text_styles = [
        (TextStyle::Small, font(FS_SMALL)),
        (TextStyle::Body, font(FS)),
        (TextStyle::Button, font(FS)),
        (TextStyle::Heading, bold(13.0)),
        (TextStyle::Monospace, FontId::monospace(FS)),
    ]
    .into();
    st.spacing.item_spacing = egui::vec2(4.0, 3.0);
    st.spacing.button_padding = egui::vec2(4.0, 1.0);
    st.spacing.interact_size = egui::vec2(24.0, 15.0);
    st.spacing.combo_height = 300.0;
    st.spacing.icon_width = 11.0;
    st.spacing.icon_width_inner = 6.0;
    let mut v = if light { egui::Visuals::light() } else { egui::Visuals::dark() };
    let r0 = CornerRadius::ZERO;
    v.override_text_color = Some(text());
    v.panel_fill = panel();
    v.window_fill = panel();
    v.window_stroke = Stroke::new(1.0, outline());
    v.window_corner_radius = CornerRadius::same(2);
    v.menu_corner_radius = CornerRadius::same(2);
    v.extreme_bg_color = sep();
    v.faint_bg_color = lane();
    v.code_bg_color = sep();
    v.selection.bg_fill = SEL_ROW;
    v.selection.stroke = Stroke::new(1.0, text());
    v.hyperlink_color = ORANGE;
    v.window_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(110) };
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 2], blur: 6, spread: 0, color: Color32::from_black_alpha(90) };
    let w = &mut v.widgets;
    for (ws, fill, fg) in [
        (&mut w.noninteractive, panel(), text()),
        (&mut w.inactive, sep(), text()),
        (&mut w.hovered, gray(0x2e), gray(0xd0)),
        (&mut w.active, gray(0x46), gray(0xe0)),
        (&mut w.open, sep(), text()),
    ] {
        ws.bg_fill = fill;
        ws.weak_bg_fill = fill;
        ws.bg_stroke = Stroke::new(1.0, outline());
        ws.fg_stroke = Stroke::new(1.0, fg);
        ws.corner_radius = r0;
        ws.expansion = 0.0;
    }
    w.noninteractive.bg_stroke = Stroke::new(1.0, sep());
    st.visuals = v;
    // Same style for egui's dark and light slots: the palette decides light/dark.
    ctx.set_style_of(egui::Theme::Light, st.clone());
    ctx.set_style_of(egui::Theme::Dark, st);
}

/// Recompute the palette (and egui style) when the settings or OS theme change.
pub fn update(ctx: &egui::Context, s: &ThemeSettings) {
    let sys = ctx.system_theme().map(|t| t == egui::Theme::Dark);
    let p = Palette::resolve(s, sys);
    if p != palette() || !ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("theme_applied")).unwrap_or(false)) {
        set_palette(p);
        apply_style(ctx);
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("theme_applied"), true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_identity_and_settings_change_it() {
        let s = ThemeSettings::default();
        let p = Palette::resolve(&s, Some(true));
        assert_eq!(p.map(0x36), Color32::from_gray(0x36));
        let light = Palette::resolve(&ThemeSettings { appearance: Appearance::Light, ..s.clone() }, Some(true));
        assert!(light.map(0x36).r() > 180 && light.map(0xb5).r() < 80, "light theme inverts greys");
        let follow = Palette::resolve(&s, Some(false));
        assert!(follow.light, "Follow System picks up a light OS theme");
        let warm = Palette::resolve(&ThemeSettings { tone: Tone::Warm, ..s.clone() }, None).map(0x80);
        assert!(warm.r() > warm.b());
        let hc = Palette::resolve(&ThemeSettings { high_contrast: true, ..s.clone() }, None);
        assert!(hc.map(0xb5).r() > 0xb5 && hc.map(0x36).r() < 0x36);
        let tinted = Palette::resolve(&ThemeSettings { color_intensity: 1.0, color_hue: 240.0, ..s.clone() }, None).map(0x80);
        assert!(tinted.b() > tinted.r());
        let dim = Palette::resolve(&ThemeSettings { brightness: 0.5, ..s }, None).map(0x80);
        assert_eq!(dim, Color32::from_gray(0x40));
    }
}
