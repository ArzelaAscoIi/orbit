//! The five themes, the fonts, and the egui style built from them.
//!
//! Views read colours through `p()`, the palette in use, and never write a
//! colour of their own. Every hex value in the app lives in this file.
//!
//! One colour, one meaning, in every theme:
//!   signal     it needs you: your move, the current step, the selected row
//!   btn        the primary button
//!   success    done, CI green, added lines
//!   danger     failed or blocked
//! Everything else is text on bg in three strengths (all 4.5:1 or more on
//! bg and sel), hairlines, and `control` borders (3:1).
//!
//! The instrument look: no corner radius anywhere, no filled cards, rows
//! split by hairlines, metadata in Geist Mono.

use std::sync::atomic::{AtomicU8, Ordering};

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
};
use serde::{Deserialize, Serialize};

use crate::story::{PrState, Stage, Tone};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Color32,
    pub side: Color32,
    pub line: Color32,
    pub line_2: Color32,
    /// Borders that mark a control (buttons, check boxes): 3:1 on bg.
    pub control: Color32,
    pub text: Color32,
    pub text_2: Color32,
    pub text_3: Color32,
    pub signal: Color32,
    pub signal_line: Color32,
    pub btn: Color32,
    pub on_btn: Color32,
    pub sel: Color32,
    pub success: Color32,
    pub danger: Color32,
    pub diff_bg: Color32,
    pub diff_text: Color32,
    pub dark: bool,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub const PHOSPHOR: Palette = Palette {
    bg: hex(0x0c0c0b),
    side: hex(0x0a0a09),
    line: hex(0x232220),
    line_2: hex(0x34332f),
    control: hex(0x65645f),
    text: hex(0xe9e7e1),
    text_2: hex(0x9b988f),
    text_3: hex(0x82807a),
    signal: hex(0xff5a1f),
    signal_line: hex(0x3a1c10),
    btn: hex(0xff5a1f),
    on_btn: hex(0x0c0c0b),
    sel: hex(0x171513),
    success: hex(0x8fd694),
    danger: hex(0xff6b5b),
    diff_bg: hex(0x121a12),
    diff_text: hex(0xa6d9a0),
    dark: true,
};

pub const AMBER: Palette = Palette {
    bg: hex(0x0b0a08),
    side: hex(0x080705),
    line: hex(0x221f18),
    line_2: hex(0x332e24),
    control: hex(0x686356),
    text: hex(0xf1ead8),
    text_2: hex(0xa39a85),
    text_3: hex(0x867e6e),
    signal: hex(0xffb000),
    signal_line: hex(0x3a2c0a),
    btn: hex(0xffb000),
    on_btn: hex(0x0b0a08),
    sel: hex(0x16130c),
    success: hex(0xb8d98a),
    danger: hex(0xff7a59),
    diff_bg: hex(0x15170d),
    diff_text: hex(0xc8d99a),
    dark: true,
};

pub const GRAPHITE: Palette = Palette {
    bg: hex(0x111213),
    side: hex(0x0d0e0f),
    line: hex(0x232527),
    line_2: hex(0x35383b),
    control: hex(0x66696c),
    text: hex(0xeceef0),
    text_2: hex(0x9a9ea3),
    text_3: hex(0x83868b),
    signal: hex(0xff3d2e),
    signal_line: hex(0x3b1714),
    btn: hex(0xeceef0),
    on_btn: hex(0x111213),
    sel: hex(0x1a1c1e),
    success: hex(0x5fd38d),
    danger: hex(0xff5c50),
    diff_bg: hex(0x10201a),
    diff_text: hex(0x86dcaa),
    dark: true,
};

/// Ghostty's default look: slate grey, not black.
pub const SLATE: Palette = Palette {
    bg: hex(0x282c34),
    side: hex(0x22262d),
    line: hex(0x363b44),
    line_2: hex(0x464c56),
    control: hex(0x858b95),
    text: hex(0xeceef1),
    text_2: hex(0xb0b5bd),
    text_3: hex(0xa0a6af),
    signal: hex(0xff9b5e),
    signal_line: hex(0x4a3528),
    btn: hex(0xeceef1),
    on_btn: hex(0x282c34),
    sel: hex(0x31363f),
    success: hex(0x7fd99a),
    danger: hex(0xff8a80),
    diff_bg: hex(0x2b3a33),
    diff_text: hex(0x9be0b0),
    dark: true,
};

pub const PAPER: Palette = Palette {
    bg: hex(0xe8e6e0),
    side: hex(0xe0ded7),
    line: hex(0xc4c1b7),
    line_2: hex(0xa9a59a),
    control: hex(0x68675f),
    text: hex(0x111111),
    text_2: hex(0x37362f),
    text_3: hex(0x47463f),
    signal: hex(0x0038ff),
    signal_line: hex(0xc9d3fb),
    btn: hex(0x0038ff),
    on_btn: hex(0xffffff),
    sel: hex(0xdadcea),
    success: hex(0x1a7f37),
    danger: hex(0xb71c1c),
    diff_bg: hex(0xd8e6d6),
    diff_text: hex(0x1d5e2b),
    dark: false,
};

/// A second light theme unlike Paper: cool light grey (not glaring white),
/// ink text, a deep green signal (Paper is warm grey with Klein blue).
pub const DAYLIGHT: Palette = Palette {
    bg: hex(0xe8ecf1),
    side: hex(0xdde3e9),
    line: hex(0xd0d5db),
    line_2: hex(0xaeb5be),
    control: hex(0x68707a),
    text: hex(0x0a0e13),
    text_2: hex(0x2c333b),
    text_3: hex(0x3f4750),
    signal: hex(0x08604a),
    signal_line: hex(0xc4e0d3),
    btn: hex(0x08604a),
    on_btn: hex(0xffffff),
    sel: hex(0xd5dfe6),
    success: hex(0x1a7f37),
    danger: hex(0xa81e24),
    diff_bg: hex(0xe3f2e8),
    diff_text: hex(0x175c2c),
    dark: false,
};

/// What the user picked in Settings; saved in `settings.json`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    Phosphor,
    Amber,
    Graphite,
    /// The default: Ghostty's look.
    #[default]
    Slate,
    Paper,
    Daylight,
    /// Paper when the Mac is in Light mode, otherwise Phosphor.
    System,
}

impl Choice {
    pub const ALL: [Choice; 7] = [
        Choice::Phosphor,
        Choice::Amber,
        Choice::Graphite,
        Choice::Slate,
        Choice::Paper,
        Choice::Daylight,
        Choice::System,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Choice::Phosphor => "Phosphor · Signal Orange",
            Choice::Amber => "Amber CRT",
            Choice::Graphite => "Graphite · Signal Red",
            Choice::Slate => "Slate · Ghostty",
            Choice::Paper => "Lab Paper · Klein Blue",
            Choice::Daylight => "Daylight · Ink Green",
            Choice::System => "Match the system",
        }
    }

    /// The next theme, for ⌘⇧T.
    pub fn next(self) -> Choice {
        let i = Self::ALL.iter().position(|c| *c == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn palette(self, system_light: bool) -> &'static Palette {
        PALETTES[self.index(system_light) as usize]
    }

    fn index(self, system_light: bool) -> u8 {
        match self {
            Choice::Phosphor => 0,
            Choice::Amber => 1,
            Choice::Graphite => 2,
            Choice::Slate => 4,
            Choice::Paper => 3,
            Choice::Daylight => 5,
            Choice::System if system_light => 3,
            Choice::System => 0,
        }
    }
}

const PALETTES: [&Palette; 6] = [&PHOSPHOR, &AMBER, &GRAPHITE, &PAPER, &SLATE, &DAYLIGHT];

/// Index into `PALETTES`; `u8::MAX` until the first `follow`.
static CURRENT: AtomicU8 = AtomicU8::new(u8::MAX);

/// The palette in use.
pub fn p() -> &'static Palette {
    PALETTES
        .get(CURRENT.load(Ordering::Relaxed) as usize)
        .copied()
        .unwrap_or(&PHOSPHOR)
}

/// Puts `choice` on screen. Cheap when nothing changed, so it runs every
/// frame: that is how `System` follows the Mac's appearance.
pub fn follow(ctx: &egui::Context, choice: Choice) {
    let light = ctx.system_theme() == Some(egui::Theme::Light);
    let index = choice.index(light);
    if CURRENT.swap(index, Ordering::Relaxed) != index {
        style(ctx, PALETTES[index as usize]);
    }
}

/// Geist for text, Geist Mono for metadata. egui's own fonts stay behind
/// them for the glyphs Geist lacks (✳ ↗ ■ ▲ ⟳).
pub fn fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        (
            "geist",
            &include_bytes!("../assets/fonts/Geist-Regular.ttf")[..],
        ),
        (
            "geist-medium",
            &include_bytes!("../assets/fonts/Geist-Medium.ttf")[..],
        ),
        (
            "geist-semibold",
            &include_bytes!("../assets/fonts/Geist-SemiBold.ttf")[..],
        ),
        (
            "geist-mono",
            &include_bytes!("../assets/fonts/GeistMono-Regular.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), FontData::from_static(bytes).into());
    }
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    let family = |first: &str| [vec![first.to_owned()], fallback.clone()].concat();
    fonts
        .families
        .insert(FontFamily::Proportional, family("geist"));
    fonts.families.insert(medium(), family("geist-medium"));
    fonts.families.insert(semibold(), family("geist-semibold"));
    let mono = &mut fonts.families.get_mut(&FontFamily::Monospace).unwrap();
    mono.insert(0, "geist-mono".into());
    ctx.set_fonts(fonts);
}

/// Geist 500: titles, buttons, names.
pub fn medium() -> FontFamily {
    FontFamily::Name("medium".into())
}

/// Geist 600: headings.
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

/// The dim laid over the app around the tutorial's highlight.
pub fn scrim() -> Color32 {
    Color32::from_black_alpha(if p().dark { 170 } else { 110 })
}

/// A signal colour as a quiet background: removed diff lines.
pub fn wash(color: Color32) -> Color32 {
    color.gamma_multiply(0.14)
}

/// Who moves next, not which column it sits in; the label tells the
/// stages apart.
pub fn stage(stage: Stage) -> Color32 {
    let p = p();
    match stage {
        Stage::Intake | Stage::Planning | Stage::Building => p.text_2,
        Stage::Review | Stage::Testing => p.signal,
        Stage::Blocked => p.danger,
        Stage::Done => p.success,
    }
}

/// A PR's status in GitHub's own colours, so it reads as it does there:
/// Primer's open, done, closed, attention (the merge queue) and neutral
/// (draft) foregrounds, light or dark with the palette. An approved PR is
/// still an open one.
pub fn pr(state: PrState) -> Color32 {
    let (light, dark) = match state {
        PrState::Open | PrState::Approved => (0x1a7f37, 0x3fb950),
        PrState::Merging => (0x9a6700, 0xd29922),
        PrState::Merged => (0x8250df, 0xab7df8),
        PrState::Closed => (0xd1242f, 0xf85149),
        PrState::Draft => (0x59636e, 0x9198a1),
    };
    hex(if p().dark { dark } else { light })
}

/// Timeline marks: people stand out, bots recede.
pub fn tone(tone: Tone) -> Color32 {
    let p = p();
    match tone {
        Tone::Bot | Tone::Claude => p.text_3,
        Tone::Human => p.text,
        Tone::Problem => p.danger,
    }
}

/// The terminal's 16 ANSI colours. The rest of the 256 are computed.
pub const ANSI: [Color32; 16] = [
    hex(0x1a1b21),
    hex(0xeb5f5f),
    hex(0x4cc38a),
    hex(0xf2b24a),
    hex(0x5a9cf2),
    hex(0xc678dd),
    hex(0x3cb8c4),
    hex(0xc8cad0),
    hex(0x5c606b),
    hex(0xff7b7b),
    hex(0x6ee0a6),
    hex(0xffcb6b),
    hex(0x7cb4ff),
    hex(0xd896f0),
    hex(0x5ed4df),
    hex(0xf4f5f7),
];

fn style(ctx: &egui::Context, p: &Palette) {
    let mut style = (*ctx.global_style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::new(20.0, semibold())),
        (TextStyle::Body, FontId::proportional(13.5)),
        (TextStyle::Button, FontId::new(13.0, medium())),
        (TextStyle::Small, FontId::proportional(11.5)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(14.0, 6.0);
    // A hairline that shows on hover, in the widget colour: egui's default
    // handle takes the text colour and reads as a white block.
    style.spacing.scroll = egui::style::ScrollStyle {
        bar_width: 6.0,
        floating_width: 2.0,
        foreground_color: false,
        active_handle_opacity: 0.8,
        ..egui::style::ScrollStyle::floating()
    };

    let v = &mut style.visuals;
    *v = if p.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = p.bg;
    v.window_fill = p.bg;
    v.extreme_bg_color = p.side;
    v.text_edit_bg_color = Some(p.side);
    v.faint_bg_color = p.sel;
    v.code_bg_color = p.sel;
    v.hyperlink_color = p.text;
    v.override_text_color = Some(p.text);
    v.selection.bg_fill = p.signal_line;
    v.selection.stroke = Stroke::new(1.0, p.signal);
    v.window_stroke = Stroke::new(1.0, p.control);
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::ZERO;
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    for (w, fill, stroke) in [
        (&mut v.widgets.noninteractive, p.bg, p.line),
        (&mut v.widgets.inactive, Color32::TRANSPARENT, p.control),
        (&mut v.widgets.hovered, p.sel, p.text_2),
        (&mut v.widgets.active, p.sel, p.text),
        (&mut v.widgets.open, p.sel, p.control),
    ] {
        w.corner_radius = CornerRadius::ZERO;
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, stroke);
        w.fg_stroke.color = p.text;
        w.expansion = 0.0;
    }
    v.widgets.noninteractive.fg_stroke.color = p.text_2;
    // Same style under both egui themes, whatever the system appearance.
    ctx.set_theme(if p.dark {
        egui::ThemePreference::Dark
    } else {
        egui::ThemePreference::Light
    });
    ctx.set_style_of(egui::Theme::Light, style.clone());
    ctx.set_style_of(egui::Theme::Dark, style);
}

/// `fg` moved toward black or white, away from `bg`, until it reads on `bg` (4.5:1, as body text):
/// a terminal program picks its colours for a dark background, and on
/// Paper its greys would vanish.
pub fn readable(fg: Color32, bg: Color32) -> Color32 {
    let far = if luminance(bg) > 0.18 {
        hex(0x111111)
    } else {
        hex(0xeeeeee)
    };
    let mut out = fg;
    for step in 1..=10 {
        if ratio(out, bg) >= 4.5 {
            break;
        }
        out = fg.lerp_to_gamma(far, step as f32 / 10.0);
    }
    out
}

fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

fn ratio(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dark_terminals_grey_still_reads_on_paper() {
        let grey = hex(0xc8cad0);
        assert!(ratio(grey, PAPER.bg) < 3.0);
        assert!(ratio(readable(grey, PAPER.bg), PAPER.bg) >= 4.5);
        // A colour that already reads is left alone.
        assert_eq!(readable(PAPER.text, PAPER.bg), PAPER.text);
    }

    #[test]
    fn every_palette_is_readable() {
        for p in PALETTES {
            for surface in [p.bg, p.side, p.sel] {
                // text_3 carries 11px metadata, so it needs 4.5:1 too
                // (HIG accessibility, text up to 17 pt).
                for text in [p.text, p.text_2, p.text_3, p.signal, p.danger] {
                    assert!(ratio(text, surface) >= 4.5, "{text:?} on {surface:?}");
                }
                // A control's border is seen, not read: 3:1.
                assert!(ratio(p.control, surface) >= 3.0, "{:?}", p.control);
            }
            assert!(ratio(p.on_btn, p.btn) >= 4.5);
            assert!(ratio(p.diff_text, p.diff_bg) >= 4.5);
        }
    }

    #[test]
    fn system_follows_the_mac_and_cycling_visits_every_choice() {
        assert!(!Choice::System.palette(true).dark);
        assert!(Choice::System.palette(false).dark);
        assert_eq!(
            Choice::default(),
            Choice::Slate,
            "Ghostty's look by default"
        );
        let mut seen = vec![Choice::Phosphor];
        let n = Choice::ALL.len();
        while seen.len() <= n {
            seen.push(seen.last().unwrap().next());
        }
        assert_eq!(seen[n], Choice::Phosphor);
        assert_eq!(&seen[..n], &Choice::ALL);
    }

    #[test]
    fn the_two_light_themes_look_apart() {
        const { assert!(!DAYLIGHT.dark && !PAPER.dark) };
        let far = |a: Color32, b: Color32| {
            (a.r() as i32 - b.r() as i32).abs()
                + (a.g() as i32 - b.g() as i32).abs()
                + (a.b() as i32 - b.b() as i32).abs()
        };
        // As light as each other (neither glares), told apart by hue:
        // Paper warm, Daylight cool.
        let warmth = |c: Color32| c.r() as i32 - c.b() as i32;
        assert!(
            warmth(PAPER.bg) > 4 && warmth(DAYLIGHT.bg) < -4,
            "warm vs cool"
        );
        assert!(far(DAYLIGHT.signal, PAPER.signal) > 200, "signals apart");
        assert_eq!(Choice::Daylight.palette(false).bg, DAYLIGHT.bg);
    }

    #[test]
    fn choice_saves_as_its_lowercase_name() {
        assert_eq!(serde_json::to_string(&Choice::Paper).unwrap(), "\"paper\"");
        let back: Choice = serde_json::from_str("\"graphite\"").unwrap();
        assert_eq!(back, Choice::Graphite);
    }
}
