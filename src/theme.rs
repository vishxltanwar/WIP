//! The 10 preset themes. Each differs in palette, type, bar style, motion, workspace glyphs, procedural
//! background and the treatment applied to the user's own wallpapers.

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);
impl Rgb {
    pub fn hex(s: &str) -> Option<Rgb> {
        let s = s.trim().trim_start_matches('#');
        let v = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
        match s.len() {
            3 => { let d = |i| u8::from_str_radix(&s[i..i + 1].repeat(2), 16).ok(); Some(Rgb(d(0)?, d(1)?, d(2)?)) }
            6 | 8 => Some(Rgb(v(0)?, v(2)?, v(4)?)),
            _ => None,
        }
    }
    pub fn css(self) -> String { format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2) }
    pub fn luma(self) -> f32 { 0.299 * self.0 as f32 + 0.587 * self.1 as f32 + 0.114 * self.2 as f32 }
    pub fn f(self) -> [f32; 3] { [self.0 as f32 / 255., self.1 as f32 / 255., self.2 as f32 / 255.] }
    pub fn mix(self, o: Rgb, k: f32) -> Rgb {
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * k).round().clamp(0., 255.) as u8;
        Rgb(m(self.0, o.0), m(self.1, o.1), m(self.2, o.2))
    }
    pub fn slint(self) -> slint::Color { slint::Color::from_rgb_u8(self.0, self.1, self.2) }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Palette { pub bg: Rgb, pub surface: Rgb, pub ink: Rgb, pub accent: Rgb, pub accent2: Rgb, pub muted: Rgb }
impl Palette {
    pub const KEYS: [&'static str; 6] = ["Background", "Surface", "Text", "Accent", "Second accent", "Muted"];
    pub fn all(&self) -> [Rgb; 6] { [self.bg, self.surface, self.ink, self.accent, self.accent2, self.muted] }
    pub fn set(&mut self, i: usize, c: Rgb) {
        match i { 0 => self.bg = c, 1 => self.surface = c, 2 => self.ink = c, 3 => self.accent = c, 4 => self.accent2 = c, _ => self.muted = c }
    }
}

pub const BAR_STYLES: [&str; 12] = ["islands", "cartouche", "collage", "ember", "callout", "credits", "dossier", "xerox", "etch", "dither", "hud", "chrome"];
pub const MOTIONS: [&str; 5] = ["calm", "majestic", "snappy", "flicker", "glitch"];
pub const CLOCKS: [&str; 5] = ["brush", "roman", "headline", "digital", "serif"];
pub const TREATMENTS: [(&str, &str); 12] = [("none", "Original"), ("grain", "Film grain"), ("duotone", "Duotone"), ("halftone", "Halftone"),
    ("dither", "1-bit dither"), ("xerox", "Photocopy"), ("bw", "High-contrast black and white"), ("inkwash", "Ink wash"),
    ("warm", "Warm bloom"), ("pixel", "Pixelate"), ("engrave", "Engraving"), ("oil", "Oil paint")];

#[derive(Clone, PartialEq, Debug)]
pub struct Theme {
    pub id: &'static str,
    pub name: String,
    pub tagline: &'static str,
    pub palette: Palette,
    pub display: String,
    pub body: String,
    pub mono: String,
    pub ws: Vec<String>,
    pub style: &'static str,
    pub height: u32,
    pub motion: &'static str,
    pub clock: &'static str,
    pub treatment: &'static str,
}

fn pal(c: [&str; 6]) -> Palette {
    let h = |s| Rgb::hex(s).unwrap();
    Palette { bg: h(c[0]), surface: h(c[1]), ink: h(c[2]), accent: h(c[3]), accent2: h(c[4]), muted: h(c[5]) }
}
#[allow(clippy::too_many_arguments)]
fn t(id: &'static str, name: &str, tagline: &'static str, c: [&str; 6], f: [&str; 3], ws: &[&str], style: &'static str, height: u32,
     motion: &'static str, clock: &'static str, treatment: &'static str) -> Theme {
    Theme { id, name: name.into(), tagline, palette: pal(c), display: f[0].into(), body: f[1].into(), mono: f[2].into(),
        ws: ws.iter().map(|s| s.to_string()).collect(), style, height, motion, clock, treatment }
}

pub const THEME_IDS: &[&str] = &[
    "vagabond", "fresco", "relic", "inferno", "schematic",
    "marquee", "dossier", "xerox", "etch", "dither", "ghost", "y2k",
];

/// The ten cards; the second entry of a card is its one-click variant.
pub fn presets() -> Vec<Vec<Theme>> {
    vec![
        vec![t("vagabond", "Vagabond", "Musashi's Five Rings in sumi and washi", ["#14161A", "#1C2026", "#E8E2D4", "#C8402E", "#C8824B", "#6F7E7B"],
            ["Yuji Syuku", "Shippori Mincho B1", "JetBrains Mono"], &["地", "水", "火", "風", "空"], "islands", 52, "calm", "brush", "inkwash")],
        vec![t("fresco", "Fresco", "Wet plaster, painted one day's patch at a time", ["#E3E7E8", "#F3F5F4", "#1E232B", "#2E68B8", "#9A4632", "#7C8388"],
            ["Marcellus", "Alegreya Sans", "Red Hat Mono"], &["Azzurro", "Verdaccio", "Sinopia", "Giallorino", "Lacca"], "cartouche", 48, "calm", "roman", "warm")],
        vec![t("relic", "Relic", "A vanitas shelf lit by one candle that burns down with the day", ["#15100B", "#221A13", "#E6DCC6", "#D9A848", "#8E2F22", "#7A6C5A"],
            ["Castoro Italic", "Castoro", "DM Mono"], &["Skull", "Crown", "Key", "Book", "Butterfly"], "collage", 50, "calm", "serif", "grain")],
        vec![t("inferno", "Inferno", "Black shapes cut out against the fire", ["#120806", "#B3361C", "#F3DCC4", "#F7B940", "#E25A1C", "#6E4436"],
            ["UnifrakturCook", "Grenze", "Chivo Mono"], &["Acheron", "Styx", "Phlegethon", "Cocytus", "Lethe"], "ember", 46, "flicker", "serif", "oil")],
        vec![t("schematic", "Schematic", "Blueprint meets the battle painting", ["#18222B", "#1F2B35", "#E6E4D8", "#F2A900", "#78A6CC", "#6A7C88"],
            ["Rajdhani", "Archivo Narrow", "Share Tech Mono"], &["Plan", "Section", "Profile", "Detail", "Notes"], "callout", 50, "snappy", "digital", "duotone")],
        vec![t("marquee", "Marquee", "Red and white crime-film poster", ["#F4F1EA", "#FFFFFF", "#111111", "#E3262E", "#111111", "#8A8580"],
                ["Anton", "Oswald", "Courier Prime"], &["Act I", "Act II", "Act III", "Act IV", "Finale"], "credits", 46, "snappy", "headline", "bw"),
             t("dossier", "Dossier", "Beige case file, red annotations", ["#D9D2C3", "#E6E0D3", "#1d1b18", "#A3161A", "#1d1b18", "#7d766a"],
                ["League Gothic", "Special Elite", "Courier Prime"], &["Evidence", "Suspects", "Timeline", "Reports", "Closed"], "dossier", 44, "snappy", "headline", "grain")],
        vec![t("xerox", "Xerox", "Photocopied poster, toner and halftone", ["#E9E4D8", "#E9E4D8", "#141414", "#4FA3C7", "#141414", "#6e6a61"],
            ["Rubik Dirt", "Space Grotesk", "Space Mono"], &["No. 1", "No. 2", "No. 3", "No. 4", "No. 5"], "xerox", 46, "snappy", "headline", "xerox")],
        vec![t("etch", "Etch", "Engraving, hatching and one point of light", ["#0B0B0B", "#151412", "#EDEAE2", "#F3DDA6", "#9a968c", "#5c5a55"],
                ["IM Fell English SC", "Cormorant Garamond", "IBM Plex Mono"], &["Plate I", "Plate II", "Plate III", "Plate IV", "Plate V"], "etch", 46, "calm", "serif", "engrave"),
             t("dither", "Dither", "1-bit, Bayer dots and light beams", ["#000000", "#000000", "#FFFFFF", "#FFFFFF", "#FFFFFF", "#777777"],
                ["Silkscreen", "Pixelify Sans", "VT323"], &["Sun", "Moon", "Star", "Eye", "Door"], "dither", 42, "calm", "digital", "dither")],
        vec![t("ghost", "Ghost Signal", "Teal halftone, HUD glass, cyberpunk", ["#050608", "#0d1f1d", "#9CC9C0", "#F0A23B", "#1F4F4A", "#D8433A"],
            ["Chakra Petch", "Rajdhani", "Share Tech Mono"], &["CH-01", "CH-02", "CH-03", "CH-04", "CH-05"], "hud", 46, "glitch", "digital", "halftone")],
        vec![t("y2k", "Y2K Blue", "Early-2000s blue pixel collage", ["#0b1170", "#1A2BD6", "#FFFFFF", "#E040A0", "#6C8CFF", "#B6C7FF"],
            ["Pixelify Sans", "Space Grotesk", "VT323"], &["Home", "Music", "Pics", "Chat", "Games"], "chrome", 44, "glitch", "digital", "pixel")],
    ]
}

#[cfg(test)]
pub fn preset(id: &str) -> Theme {
    presets().into_iter().flatten().find(|t| t.id == id).unwrap_or_else(|| presets()[0][0].clone())
}

fn family(s: &str) -> String {
    let f: String = s.chars().filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-').take(60).collect();
    if f.trim().is_empty() { "sans-serif".into() } else { f }
}
/// Specs come from users, packages and Claude: clamp every value that lands in CSS or HTML.
pub fn sanitize(t: &Theme) -> Theme {
    let mut s = t.clone();
    s.display = family(&t.display);
    s.body = family(&t.body);
    s.mono = family(&t.mono);
    s.ws = t.ws.iter().take(9).map(|w| w.chars().take(12).collect()).collect();
    if s.ws.is_empty() { s.ws = vec!["1".into()]; }
    s.height = t.height.clamp(32, 72);
    if !BAR_STYLES.contains(&s.style) { s.style = "islands"; }
    if !MOTIONS.contains(&s.motion) { s.motion = "calm"; }
    if !CLOCKS.contains(&s.clock) { s.clock = "digital"; }
    s
}

pub fn roman(mut n: u32) -> String {
    if n == 0 { return "N".into(); }
    let mut s = String::new();
    for (v, r) in [(50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")] {
        while n >= v { s += r; n -= v; }
    }
    s
}
pub fn clock_text(t: &Theme, h: u32, m: u32) -> String {
    if t.clock == "roman" { format!("{} \u{00B7} {}", roman(h), roman(m)) } else { format!("{h:02}:{m:02}") }
}

/// Local time from the Windows clock, without pulling in a date crate.
pub fn now_hm() -> (u32, u32, String) {
    #[repr(C)] #[derive(Default)]
    struct St { y: u16, mo: u16, dow: u16, d: u16, h: u16, mi: u16, s: u16, ms: u16 }
    extern "system" { fn GetLocalTime(st: *mut St); }
    let mut st = St::default();
    unsafe { GetLocalTime(&mut st) };
    const DAYS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
    const MON: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
    (st.h as u32, st.mi as u32, format!("{} {} {}", DAYS[st.dow as usize % 7], st.d, MON[(st.mo as usize + 11) % 12]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sanitize_clamps_hostile_specs() {
        let mut t = preset("vagabond");
        t.display = "x';}</style><script>".into();
        t.height = 400;
        t.ws = vec!["1234567890123456".into(); 20];
        let s = sanitize(&t);
        assert!(!s.display.contains(['\'', ';', '<', '}']));
        assert_eq!(s.height, 72);
        assert_eq!(s.ws.len(), 9);
        assert_eq!(s.ws[0].chars().count(), 12);
        assert_eq!(roman(49), "XLIX");
        assert_eq!(Rgb::hex("#abc"), Some(Rgb(0xaa, 0xbb, 0xcc)));
    }
}
