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

/// The fonts, read from the Mac at start: none ship with Orbit, so its
/// licences stay MIT/Apache. Geist and Geist Mono when the user has them
/// installed, else the system's SF Pro and SF Mono (Menlo before macOS 10.15);
/// Apple Symbols and Menlo behind them for the glyphs the text fonts lack
/// (⌘ ✳ ⟳ ▲, the terminal's box drawing), STIX Two Math for ⏺, and Phosphor's icons
/// (`crate::ui::widgets::icon`) for buttons.
pub fn fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::empty();
    // ponytail: egui-phosphor 0.14 is built on egui 0.36, so only its font
    // bytes are used here; back to `add_to_fonts` once it moves to 0.37.
    fonts.font_data.insert(
        "phosphor".into(),
        FontData::from_static(egui_phosphor::Variant::Regular.font_bytes()).into(),
    );
    let mut found: Vec<&str> = Vec::new();
    for (name, data) in system_fonts(&font_dirs()) {
        fonts.font_data.insert(name.into(), data.into());
        found.push(name);
    }
    if !found.iter().any(|n| n.starts_with("text")) {
        eprintln!("orbit: no system font found (Geist, SF Pro, Menlo); text may not show");
    }
    // Phosphor right behind the text face: Apple Symbols has glyphs at the
    // private-use code points its icons sit on.
    let chain = |first: &[&str]| -> Vec<String> {
        first
            .iter()
            .chain(["phosphor", "symbols", "menlo", "stix"].iter())
            .filter(|n| **n == "phosphor" || found.contains(n))
            .map(|n| (*n).to_owned())
            .collect()
    };
    let families = &mut fonts.families;
    families.insert(FontFamily::Proportional, chain(&["text"]));
    families.insert(medium(), chain(&["text-medium", "text"]));
    families.insert(semibold(), chain(&["text-semibold", "text"]));
    families.insert(FontFamily::Monospace, chain(&["mono"]));
    ctx.set_fonts(fonts);
}

/// Where the Mac keeps fonts: the user's, the machine's, the system's.
fn font_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(std::path::Path::new(&home).join("Library/Fonts"));
    }
    dirs.push("/Library/Fonts".into());
    dirs.push("/System/Library/Fonts".into());
    dirs
}

/// The faces Orbit uses, by role (`text`, `text-medium`, `text-semibold`,
/// `mono`, `symbols`, `menlo`, `stix`), from the first of `dirs` that has each:
/// Geist when installed, else SF Pro, its variable weight axis set to 400,
/// 510 and 590 for the three roles.
fn system_fonts(dirs: &[std::path::PathBuf]) -> Vec<(&'static str, FontData)> {
    let read = |file: &str| {
        dirs.iter()
            .find_map(|d| std::fs::read(d.join(file)).ok())
            .map(|b| -> egui::epaint::text::Blob { std::sync::Arc::new(b) })
    };
    let weight = |blob: &egui::epaint::text::Blob, wght: f32| {
        FontData::from_blob(blob.clone(), 0).tweak(egui::epaint::text::FontTweak {
            coords: egui::epaint::text::VariationCoords::new([(
                egui::epaint::text::Tag::new(b"wght"),
                wght,
            )]),
            ..Default::default()
        })
    };
    let mut out = Vec::new();
    let geist = [
        ("text", "Geist-Regular.ttf"),
        ("text-medium", "Geist-Medium.ttf"),
        ("text-semibold", "Geist-SemiBold.ttf"),
    ];
    if let Some(regular) = read(geist[0].1) {
        out.push(("text", FontData::from_blob(regular, 0)));
        for (role, file) in &geist[1..] {
            if let Some(blob) = read(file) {
                out.push((*role, FontData::from_blob(blob, 0)));
            }
        }
    } else if let Some(sf) = read("SFNS.ttf") {
        out.push(("text", weight(&sf, 400.0)));
        out.push(("text-medium", weight(&sf, 510.0)));
        out.push(("text-semibold", weight(&sf, 590.0)));
    }
    let menlo = read("Menlo.ttc");
    if let Some(mono) = read("GeistMono-Regular.ttf") {
        out.push(("mono", FontData::from_blob(mono, 0)));
    } else if let Some(mono) = read("SFNSMono.ttf") {
        out.push(("mono", weight(&mono, 400.0)));
    } else if let Some(menlo) = &menlo {
        out.push(("mono", FontData::from_blob(menlo.clone(), 0)));
    }
    if let Some(symbols) = read("Apple Symbols.ttf") {
        out.push(("symbols", FontData::from_blob(symbols, 0)));
    }
    if let Some(menlo) = menlo {
        out.push(("menlo", FontData::from_blob(menlo, 0)));
    }
    // ⏺, which Claude Code starts its lines with: no other text face has it.
    if let Some(stix) = read("Supplemental/STIXTwoMath.otf") {
        out.push(("stix", FontData::from_blob(stix, 0)));
    }
    out
}

/// Medium (500): titles, buttons, names.
pub fn medium() -> FontFamily {
    FontFamily::Name("medium".into())
}

/// Semibold (600): headings.
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

/// A diff token's colour, read on `on` (the line's background). One set
/// for dark themes, one for light, then moved until it reads (4.5:1).
pub fn syntax(kind: crate::syntax::Kind, on: Color32) -> Color32 {
    use crate::syntax::Kind;
    let p = p();
    let color = match (kind, p.dark) {
        (Kind::Comment, _) => p.text_3,
        (Kind::Keyword, true) => hex(0xc792ea),
        (Kind::String, true) => hex(0xc3e88d),
        (Kind::Number, true) => hex(0xf78c6c),
        (Kind::Name, true) => hex(0x82aaff),
        (Kind::Keyword, false) => hex(0x8a3fb0),
        (Kind::String, false) => hex(0x3b7d23),
        (Kind::Number, false) => hex(0xa0521d),
        (Kind::Name, false) => hex(0x2f5fc4),
    };
    readable(color, on)
}

/// `fg` moved toward black or white, away from `bg`, until it reads on `bg` (4.5:1, as body text):
/// a terminal program picks its colours for a dark background, and on
/// Paper its greys would vanish.
pub fn readable(fg: Color32, bg: Color32) -> Color32 {
    // A terminal asks for every cell on every frame, from a few colours.
    thread_local! {
        static SEEN: std::cell::RefCell<std::collections::HashMap<(Color32, Color32), Color32>> =
            Default::default();
    }
    SEEN.with(|seen| {
        *seen
            .borrow_mut()
            .entry((fg, bg))
            .or_insert_with(|| readable_uncached(fg, bg))
    })
}

fn readable_uncached(fg: Color32, bg: Color32) -> Color32 {
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

/// The text colour for a button filled with `fill` (an intent's colour:
/// success, signal): the palette's background, text or button text,
/// whichever reads best.
pub fn on_fill(fill: Color32) -> Color32 {
    let pal = p();
    [pal.bg, pal.text, pal.on_btn]
        .into_iter()
        .max_by(|a, b| ratio(*a, fill).total_cmp(&ratio(*b, fill)))
        .unwrap_or(pal.text)
}

fn ratio(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {

    #[test]
    fn readable_is_the_same_from_its_cache() {
        let (fg, bg) = (hex(0x777777), hex(0xe8e6e0));
        assert_eq!(readable(fg, bg), readable_uncached(fg, bg));
        assert_eq!(
            readable(fg, bg),
            readable_uncached(fg, bg),
            "second call, cached"
        );
    }

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
            // The PR buttons filled by intent (Approve, Merge: success;
            // Fix: signal) read with the background or the text on them.
            for fill in [p.success, p.signal] {
                let best = [p.bg, p.text, p.on_btn]
                    .into_iter()
                    .map(|c| ratio(c, fill))
                    .fold(0.0, f32::max);
                assert!(best >= 4.5, "nothing reads on {fill:?}");
            }
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

#[cfg(test)]
mod font_tests {
    /// Every non-ASCII glyph Orbit's strings use, and what Claude Code's
    /// terminal draws, has a face on a Mac (skipped where there is none).
    #[test]
    fn the_macs_fonts_cover_every_glyph_orbit_draws() {
        if !std::path::Path::new("/System/Library/Fonts/SFNS.ttf").exists() {
            return;
        }
        let ctx = egui::Context::default();
        super::fonts(&ctx);
        let _ = ctx.run_ui(Default::default(), |_| {});
        let ui = "«·»ÄÖ×äü–—“”•…›←↑→↓↗↩⇧−⌃⌘⌥⏎─■▲▸▼▾●◐◧⚙⚠✓✗✳✻❯⟳⠋⠙";
        let terminal = "⏺⎿╭╮╰╯│─┌┐└┘├┤✽✶✢✻·▐▛▜▝▘█░▒▓";
        for (family, text) in [
            (egui::FontFamily::Proportional, ui),
            (super::medium(), ui),
            (egui::FontFamily::Monospace, ui),
            (egui::FontFamily::Monospace, terminal),
        ] {
            let id = egui::FontId::new(13.0, family.clone());
            let missing: String =
                ctx.fonts_mut(|f| text.chars().filter(|c| !f.has_glyph(&id, *c)).collect());
            assert!(missing.is_empty(), "{family:?} lacks {missing}");
        }
    }
}
