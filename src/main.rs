#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod apply;
mod art;
mod components;
mod backgrounds;
mod config;
mod error;
mod fonts;
mod logging;
mod gpu;
mod paint;
mod preview;
mod scene;
mod system;
mod theme;
mod wallpaper;
mod zebar;

use scene::Scene;
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};
use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::{mpsc, Arc}, time::{Duration, Instant}};
use theme::{Rgb, Theme, BAR_STYLES, CLOCKS, MOTIONS, TREATMENTS};
use tiny_skia::Pixmap;

slint::include_modules!();

const CARD: (u32, u32) = (768, 432);

enum Job {
    Card(usize, u64, Pixmap),
    Desk(usize, u64, preview::Desk),
    Images(Vec<(PathBuf, Pixmap, SharedPixelBuffer<Rgba8Pixel>)>, usize),
    Done(&'static str, Result<(), String>),
}

struct State {
    presets: Vec<Vec<Theme>>,
    variant: Vec<usize>,
    themes: Vec<Theme>,
    card_gen: Vec<u64>,
    card_px: Vec<Option<Pixmap>>,
    desk_gen: u64,
    desk: Option<(usize, preview::Desk)>,
    desk_fresh: bool,
    images: Arc<Vec<Pixmap>>,
    paths: Vec<PathBuf>,
    thumbs: Vec<Image>,
    layout: usize,
    strength: f32,
    swatch: usize,
    page: i32,
    wheel: f32,
    busy: bool,
    unlock_at: Option<Instant>,
    dirty: Option<Instant>,
    name_at: f32,
    scene: Scene,
    stage: Option<gpu::Stage>,
    tx: mpsc::Sender<Job>,
}

fn title_case(s: &str) -> String { let mut c = s.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() }
fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> { ModelRc::new(VecModel::from(v)) }
fn strings(v: impl IntoIterator<Item = String>) -> ModelRc<SharedString> { model(v.into_iter().map(SharedString::from).collect()) }

fn hsv(c: Rgb) -> (f32, f32, f32) {
    let [r, g, b] = c.f();
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let h = if d == 0. { 0. } else if mx == r { 60. * ((g - b) / d).rem_euclid(6.) } else if mx == g { 60. * ((b - r) / d + 2.) } else { 60. * ((r - g) / d + 4.) };
    (h, if mx == 0. { 0. } else { d / mx }, mx)
}
fn from_hsv(h: f32, s: f32, v: f32) -> Rgb {
    let c = v * s;
    let x = c * (1. - ((h / 60.).rem_euclid(2.) - 1.).abs());
    let (r, g, b) = match (h.rem_euclid(360.) / 60.) as u32 { 0 => (c, x, 0.), 1 => (x, c, 0.), 2 => (0., c, x), 3 => (0., x, c), 4 => (x, 0., c), _ => (c, 0., x) };
    let k = |u: f32| ((u + v - c) * 255.).round().clamp(0., 255.) as u8;
    Rgb(k(r), k(g), k(b))
}

fn screen() -> (u32, u32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    (w.max(640) as u32, h.max(480) as u32)
}
/// The person is looking at Rice Studio: it has focus, or the pointer is over it.
fn foreground() -> bool {
    use windows_sys::Win32::{Foundation::{HWND, POINT}, UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId, WindowFromPoint}};
    let ours = |h: HWND| { let mut pid = 0u32; unsafe { GetWindowThreadProcessId(h, &mut pid) }; pid == std::process::id() };
    let mut p = POINT { x: 0, y: 0 };
    ours(unsafe { GetForegroundWindow() }) || (unsafe { GetCursorPos(&mut p) } != 0 && ours(unsafe { WindowFromPoint(p) }))
}
/// An apply starts the bar, the window manager, Windhawk and Flow, and Windows lets each take focus from us as its
/// window opens (we started them), which left the app throttled as if in the background. Lock the foreground
/// while that happens; any click or Alt by the person unlocks it anyway.
fn hold_focus(on: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{LockSetForegroundWindow, LSFW_LOCK, LSFW_UNLOCK};
    unsafe { LockSetForegroundWindow(if on { LSFW_LOCK } else { LSFW_UNLOCK }) };
}
/// Windows "Show animations in Windows" off → keep the stage nearly still.
fn reduced_motion() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION};
    let mut on = 1i32;
    unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &mut on as *mut i32 as *mut _, 0) };
    on == 0
}

impl State {
    fn f(&self) -> usize { self.scene.focused() }
    fn theme(&self) -> &Theme { &self.themes[self.f()] }

    fn regen_card(&mut self, i: usize) {
        self.card_gen[i] += 1;
        let (g, t, tx) = (self.card_gen[i], self.themes[i].clone(), self.tx.clone());
        std::thread::spawn(move || { let _ = tx.send(Job::Card(i, g, preview::card(&t, CARD.0, CARD.1))); });
    }
    fn regen_desk(&mut self) {
        self.desk_gen += 1;
        let (i, g, t, tx, imgs, layout, strength) = (self.f(), self.desk_gen, self.theme().clone(), self.tx.clone(), self.images.clone(), self.layout, self.strength);
        std::thread::spawn(move || {
            let wall = wallpaper::compose(&wallpaper::Shot { w: 1280, h: 720, theme: &t, images: &imgs, layout: wallpaper::LAYOUTS[layout].0, treatment: t.treatment, strength });
            let _ = tx.send(Job::Desk(i, g, preview::desk(&t, wall)));
        });
    }
    /// an edit to the focused theme: repaint shortly (coalesces slider drags)
    fn edited(&mut self) { self.dirty = Some(Instant::now()); self.colours(); }
    fn colours(&mut self) {
        let p = self.theme().palette;
        self.scene.target_colours = [p.bg.f(), p.accent.f(), p.accent2.f(), p.ink.f()];
        for (i, t) in self.themes.iter().enumerate() { self.scene.rims[i] = t.palette.accent.f(); }
    }

    fn sync(&mut self, app: &AppWindow) {
        let t = self.theme().clone();
        let f = self.f();
        app.set_current(f as i32);
        app.set_count(self.themes.len() as i32);
        app.set_name_chars(strings(t.name.chars().map(String::from)));
        app.set_tagline(t.tagline.into());
        app.set_display_font(t.display.clone().into());
        app.set_variants(strings(self.presets[f].iter().map(|v| v.name.clone())));
        app.set_variant(self.variant[f] as i32);
        app.set_swatches(model(t.palette.all().iter().map(|c| c.slint()).collect()));
        app.set_swatch(self.swatch as i32);
        let c = t.palette.all()[self.swatch];
        app.set_swatch_hex(c.css().into());
        let (h, s, v) = hsv(c);
        app.set_hue(h);
        app.set_sat(s);
        app.set_val(v);
        let fi = |fam: &str| fonts::LIBRARY.iter().position(|x| x.0 == fam).unwrap_or(0) as i32;
        app.set_font_display(fi(&t.display));
        app.set_font_body(fi(&t.body));
        app.set_font_mono(fi(&t.mono));
        app.set_style_index(BAR_STYLES.iter().position(|s| *s == t.style).unwrap_or(0) as i32);
        app.set_bar_height(t.height as f32);
        app.set_workspaces(t.ws.join(" ").into());
        app.set_motion_index(MOTIONS.iter().position(|s| *s == t.motion).unwrap_or(0) as i32);
        app.set_clock_index(CLOCKS.iter().position(|s| *s == t.clock).unwrap_or(0) as i32);
        app.set_treatment_index(TREATMENTS.iter().position(|s| s.0 == t.treatment).unwrap_or(0) as i32);
        app.set_layout_index(self.layout as i32);
        app.set_strength(self.strength);
        let ui = app.global::<Ui>();
        ui.set_accent(t.palette.accent.slint());
        ui.set_on_accent(if t.palette.accent.luma() > 150. { Rgb(0x0f, 0x0e, 0x0c).slint() } else { Rgb(0xf7, 0xf4, 0xee).slint() });
        self.colours();
    }

    fn focus(&mut self, app: &AppWindow, i: usize) {
        if i == self.f() { return; }
        self.scene.set_focus(i);
        self.name_at = self.scene.time;
        self.sync(app);
        self.regen_desk();
    }

    fn drain(&mut self, app: &AppWindow, rx: &mpsc::Receiver<Job>) {
        while let Ok(j) = rx.try_recv() {
            match j {
                Job::Card(i, g, px) if g == self.card_gen[i] => self.card_px[i] = Some(px),
                Job::Desk(i, g, d) if g == self.desk_gen => {
                    self.scene.desk_geo = Some([d.bar.height() as f32 / d.wall.height() as f32, 0.036,
                        0.945 - d.clock.height() as f32 / d.wall.height() as f32, d.clock.width() as f32 / d.wall.width() as f32, d.clock.height() as f32 / d.wall.height() as f32]);
                    self.desk = Some((i, d));
                    self.desk_fresh = true;
                }
                Job::Images(got, failed) => {
                    let mut imgs = (*self.images).clone();
                    for (p, px, thumb) in got {
                        self.paths.push(p);
                        imgs.push(px);
                        if self.thumbs.len() < 4 { self.thumbs.push(Image::from_rgba8(thumb)); }
                    }
                    self.images = Arc::new(imgs);
                    app.set_thumbs(model(self.thumbs.clone()));
                    app.set_image_count(self.paths.len() as i32);
                    app.set_status(if failed > 0 { format!("{failed} files could not be read and were skipped").into() } else { "".into() });
                    self.regen_desk();
                }
                Job::Done(what, r) => {
                    // the supervisor brings the tools up a few seconds after the job ends
                    self.unlock_at = Some(Instant::now() + Duration::from_secs(20));
                    self.busy = false;
                    app.set_busy(false);
                    app.set_can_undo(apply::can_undo());
                    match r {
                        Ok(()) if what == "apply" => {
                            app.set_status(format!("{} is on your desktop", self.theme().name).into());
                            self.scene.start_veil(0.5, 0.5);
                        }
                        Ok(()) if what == "off" => app.set_status("Everything is off and Windows is back the way it was".into()),
                        Ok(()) => app.set_status("Your previous desktop is back".into()),
                        Err(e) => app.set_status(format!("That did not work: {e}").into()),
                    }
                }
                _ => {}
            }
        }
    }
}

fn cli_result(r: Result<(), String>) {
    let _ = std::fs::write(apply::data_dir().join("cli-result.txt"), match &r { Ok(()) => "ok".into(), Err(e) => format!("error: {e}") });
    std::process::exit(if r.is_ok() { 0 } else { 1 });
}

const HELP: &str = "\
Rice Studio — pick a desktop theme, make it yours, apply it to Windows

USAGE:
    rice-studio                    Open the GUI
    rice-studio --apply <id>       Apply theme by ID (e.g. --apply relic)
    rice-studio --undo             Undo last apply
    rice-studio --apply --undo     Restore stock Windows
    rice-studio --supervise        Run background supervisor (started at sign-in)
    rice-studio --autostart on|off Toggle Windows startup
    rice-studio --panic            Emergency: turn everything off, restore stock
    rice-studio --uninstall        Restore stock + remove installed fonts
    rice-studio --wallpaper <n>    Switch to wallpaper n for current theme
    rice-studio --pack-test <dir> <id>  Export Zebar widget pack to folder

THEMES:
    vagabond, fresco, relic, inferno, schematic, marquee,
    dossier, xerox, etch, dither, ghost, y2k

GUI:
    Left/Right arrows or click cards to switch themes
    Click a card to open its editor (colors, fonts, layout, wallpapers)
    Drag images into the editor to use as wallpapers
    Apply / Undo / Turn Off buttons at bottom

PANIC HOTKEY:
    Ctrl+Alt+Shift+Q  (restores stock Windows immediately)
";

fn print_help() {
    eprint!("{}", HELP);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("--help") || args.get(1).map(|s| s.as_str()) == Some("-h") {
        print_help();
        return;
    }
    logging::init();
    // background modes, no window: the supervisor started at sign-in, the panic switch, uninstall
    match args.get(1).map(|s| s.as_str()) {
        Some("--supervise") => return system::supervise(),
        Some("--panic") => { let _ = system::panic(); return; }
        Some("--uninstall") => { system::stop_other_copies(); let _ = system::panic(); fonts::uninstall_all(); return; }
        Some("--autostart") => { let _ = system::set_autostart(args.get(2).map(|s| s.as_str()) == Some("on")); return; }
        // the same work as the Apply and Undo buttons, headless (tests now, Claude's connector later)
        Some("--apply") => {
            let id = args.get(2).cloned().unwrap_or_default();
            if !theme::THEME_IDS.contains(&id.as_str()) {
                return cli_result(Err(format!("no theme called {id}; valid: {}", theme::THEME_IDS.join(", "))));
            }
            let r = theme::presets().into_iter().flatten().find(|t| t.id == id).ok_or(format!("no theme called {id}")).and_then(|t| {
                let (w, h) = screen();
                let px = wallpaper::compose(&wallpaper::Shot { w, h, theme: &t, images: &[], layout: "fill", treatment: t.treatment, strength: 0.85 });
                apply::apply(zebar::build_pack(&t, &px), paint::to_png(&px), components::build(&t), &[&t.display, &t.body, &t.mono])
            });
            return cli_result(r);
        }
        Some("--undo") => return cli_result(apply::undo()),
        Some("--wallpaper") => return cli_result(args.get(2).and_then(|n| n.parse().ok()).ok_or_else(|| "which wallpaper?".to_string()).and_then(apply::pick_wallpaper)),
        _ => {}
    }
    if args.get(1).map(|s| s.as_str()) == Some("--render-test") {
        let out = PathBuf::from(&args[2]);
        for t in theme::presets().into_iter().flatten() {
            preview::card(&t, 960, 540).save_png(out.join(format!("card-{}.png", t.id))).unwrap();
        }
        // a theme name after the folder: also its five art pieces
        if let Some(t) = args.get(3).and_then(|id| theme::presets().into_iter().flatten().find(|t| &t.id == id)) {
            for i in 0..art::PIECES { art::piece(&t, i, 1600, 900).save_png(out.join(format!("art-{}-{i}.png", t.id))).unwrap(); }
        }
        return;
    }
    // a theme's whole Zebar pack written to a folder, for looking at its widgets in a browser
    if args.get(1).map(|s| s.as_str()) == Some("--pack-test") {
        let out = PathBuf::from(&args[2]);
        let t = args.get(3).and_then(|id| theme::presets().into_iter().flatten().find(|t| &t.id == id)).expect("which theme?");
        let (w, h) = screen();
        for f in zebar::build_pack(&t, &art::piece(&t, 0, w, h)) {
            let p = out.join(&f.path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            match f.data { apply::Data::Text(s) => std::fs::write(&p, s), apply::Data::Bytes(b) => std::fs::write(&p, b) }.unwrap();
        }
        return;
    }
    slint::BackendSelector::new().require_wgpu_30(slint::wgpu_30::WGPUConfiguration::default()).select().expect("a GPU with wgpu support");
    fonts::register_with_slint();
    let app = AppWindow::new().unwrap();

    let presets = theme::presets();
    let themes: Vec<Theme> = presets.iter().map(|v| v[0].clone()).collect();
    let n = themes.len();
    let (tx, rx) = mpsc::channel();
    let st = Rc::new(RefCell::new(State {
        variant: vec![0; n], card_gen: vec![0; n], card_px: vec![None; n], themes, presets,
        desk_gen: 0, desk: None, desk_fresh: false, images: Arc::new(Vec::new()), paths: Vec::new(), thumbs: Vec::new(),
        layout: 0, strength: 0.85, swatch: 3, page: 0, wheel: 0., busy: false, unlock_at: None, dirty: None, name_at: 0.,
        scene: Scene::new(n, reduced_motion()), stage: None, tx,
    }));
    {
        let mut s = st.borrow_mut();
        app.set_fonts(strings(fonts::LIBRARY.iter().map(|f| f.0.to_string())));
        app.set_styles(strings(BAR_STYLES.iter().map(|s| title_case(s))));
        app.set_motions(strings(MOTIONS.iter().map(|s| title_case(s))));
        app.set_clocks(strings(CLOCKS.iter().map(|s| title_case(s))));
        app.set_treatments(strings(TREATMENTS.iter().map(|t| t.1.to_string())));
        app.set_layouts(strings(wallpaper::LAYOUTS.iter().map(|l| l.1.to_string())));
        app.set_swatch_names(strings(theme::Palette::KEYS.iter().map(|k| k.to_string())));
        app.set_can_undo(apply::can_undo());
        app.set_start_with_windows(system::autostart());
        app.set_hotkey_taken(system::hotkey_taken());
        s.sync(&app);
        // paint every card face, the nearest first, on one worker
        let all: Vec<(usize, Theme)> = s.themes.iter().cloned().enumerate().collect();
        let tx = s.tx.clone();
        std::thread::spawn(move || for (i, t) in all { let _ = tx.send(Job::Card(i, 0, preview::card(&t, CARD.0, CARD.1))); });
        s.regen_desk();
    }

    macro_rules! on {
        ($cb:ident, |$a:ident, $s:ident $(, $arg:ident)*| $body:expr) => {{
            let (w, st) = (app.as_weak(), st.clone());
            app.$cb(move |$($arg),*| { let $a = w.unwrap(); let mut g = st.borrow_mut(); let $s = &mut *g; $body; });
        }};
    }
    on!(on_hover_at, |a, s, x, y| { s.scene.pointer(x, y); a.set_hovering(s.scene.hover.is_some()); });
    on!(on_press, |a, s, x, y| {
        if s.page == 0 {
            if let Some(i) = s.scene.hit(x, y) {
                if i == s.f() { s.page = 1; s.scene.set_page(1); a.set_page(1); } else { s.focus(&a, i); }
            }
        }
    });
    on!(on_wheel, |a, s, dy| {
        if s.page != 0 { return; }
        s.wheel += dy;
        if s.wheel.abs() >= 50. {
            let i = (s.f() as i32 - s.wheel.signum() as i32).clamp(0, s.themes.len() as i32 - 1) as usize;
            s.wheel = 0.;
            s.focus(&a, i);
        }
    });
    on!(on_step, |a, s, d| { let i = (s.f() as i32 + d).clamp(0, s.themes.len() as i32 - 1) as usize; s.focus(&a, i); });
    on!(on_go, |a, s, p| { s.page = p; s.scene.set_page(p); a.set_page(p); });
    on!(on_pick_variant, |a, s, i| {
        let f = s.f();
        s.variant[f] = i as usize;
        s.themes[f] = s.presets[f][i as usize].clone();
        s.scene.start_veil(0.25, 0.8);
        s.name_at = s.scene.time;
        s.regen_card(f);
        s.regen_desk();
        s.sync(&a);
    });
    on!(on_pick_swatch, |a, s, i| { s.swatch = i as usize; s.sync(&a); });
    on!(on_set_hex, |a, s, t| {
        if let Some(c) = Rgb::hex(&t) { let (f, k) = (s.f(), s.swatch); s.themes[f].palette.set(k, c); s.edited(); s.sync(&a); }
    });
    on!(on_set_hsv, |a, s, h, sa, v| {
        let (f, k) = (s.f(), s.swatch);
        let c = from_hsv(h, sa, v);
        s.themes[f].palette.set(k, c);
        s.edited();
        a.set_swatches(model(s.themes[f].palette.all().iter().map(|c| c.slint()).collect()));
        a.set_swatch_hex(c.css().into());
        if k == 3 { a.global::<Ui>().set_accent(c.slint()); }
    });
    on!(on_set_font, |a, s, which, i| {
        let (f, fam) = (s.f(), fonts::LIBRARY[i as usize].0.to_string());
        match which { 0 => { s.themes[f].display = fam.clone(); a.set_display_font(fam.into()); } 1 => s.themes[f].body = fam, _ => s.themes[f].mono = fam }
        s.edited();
    });
    on!(on_set_style, |_a, s, i| { let f = s.f(); s.themes[f].style = BAR_STYLES[i as usize]; s.edited(); });
    on!(on_set_height, |_a, s, v| { let f = s.f(); s.themes[f].height = v.round() as u32; s.edited(); });
    on!(on_set_workspaces, |_a, s, t| {
        let ws: Vec<String> = t.split_whitespace().map(String::from).collect();
        if !ws.is_empty() { let f = s.f(); s.themes[f].ws = ws; s.edited(); }
    });
    on!(on_set_motion, |_a, s, i| { let f = s.f(); s.themes[f].motion = MOTIONS[i as usize]; s.edited(); });
    on!(on_set_clock, |_a, s, i| { let f = s.f(); s.themes[f].clock = CLOCKS[i as usize]; s.edited(); });
    on!(on_reset, |a, s| { let f = s.f(); s.themes[f] = s.presets[f][s.variant[f]].clone(); s.edited(); s.sync(&a); });
    on!(on_set_layout, |_a, s, i| { s.layout = i as usize; s.edited(); });
    on!(on_set_treatment, |_a, s, i| { let f = s.f(); s.themes[f].treatment = TREATMENTS[i as usize].0; s.edited(); });
    on!(on_set_strength, |_a, s, v| { s.strength = v; s.edited(); });
    on!(on_clear_images, |a, s| {
        s.images = Arc::new(Vec::new());
        s.paths.clear();
        s.thumbs.clear();
        a.set_thumbs(model(Vec::new()));
        a.set_image_count(0);
        s.regen_desk();
    });
    on!(on_add_images, |a, s| {
        let Some(files) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp"]).pick_files() else { return };
        a.set_status("Reading your images".into());
        let tx = s.tx.clone();
        std::thread::spawn(move || {
            let mut got = Vec::new();
            let mut failed = 0;
            for p in files {
                match image::open(&p) {
                    Ok(img) => {
                        let prev = if img.width() > 1920 { img.thumbnail(1920, 1920) } else { img.clone() };
                        let th = img.thumbnail(296, 192).to_rgba8();
                        let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(th.as_raw(), th.width(), th.height());
                        got.push((p, paint::to_pixmap(&prev.to_rgba8()), buf));
                    }
                    Err(_) => failed += 1,
                }
            }
            let _ = tx.send(Job::Images(got, failed));
        });
    });
    on!(on_apply, |a, s| {
        if s.busy { return; }
        s.busy = true;
        a.set_busy(true);
        hold_focus(true);
        a.set_status("Applying".into());
        s.scene.start_sweep();
        let (t, paths, layout, strength, tx) = (s.theme().clone(), s.paths.clone(), s.layout, s.strength, s.tx.clone());
        std::thread::spawn(move || {
            let (w, h) = screen();
            let imgs: Vec<Pixmap> = paths.iter().filter_map(|p| image::open(p).ok()).map(|i| paint::to_pixmap(&i.to_rgba8())).collect();
            let px = wallpaper::compose(&wallpaper::Shot { w, h, theme: &t, images: &imgs, layout: wallpaper::LAYOUTS[layout].0, treatment: t.treatment, strength });
            let _ = tx.send(Job::Done("apply", apply::apply(zebar::build_pack(&t, &px), paint::to_png(&px), components::build(&t), &[&t.display, &t.body, &t.mono])));
        });
    });
    on!(on_set_start, |a, _s, on| {
        if let Err(e) = system::set_autostart(on) { a.set_status(e.into()); a.set_start_with_windows(!on); }
    });
    on!(on_turn_off, |a, s| {
        if s.busy { return; }
        s.busy = true;
        a.set_busy(true);
        hold_focus(true);
        a.set_start_with_windows(false);
        let tx = s.tx.clone();
        std::thread::spawn(move || { let _ = tx.send(Job::Done("off", system::panic())); });
    });
    on!(on_undo, |a, s| {
        if s.busy { return; }
        s.busy = true;
        a.set_busy(true);
        hold_focus(true);
        let tx = s.tx.clone();
        std::thread::spawn(move || { let _ = tx.send(Job::Done("undo", apply::undo())); });
    });

    // the GPU arrives with the window's first frame
    let gpu: Rc<RefCell<Option<(slint::wgpu_30::wgpu::Device, slint::wgpu_30::wgpu::Queue)>>> = Rc::default();
    {
        let gpu = gpu.clone();
        app.window().set_rendering_notifier(move |state, api| {
            if let (slint::RenderingState::RenderingSetup, slint::GraphicsAPI::WGPU30 { device, queue, .. }) = (state, api) {
                *gpu.borrow_mut() = Some((device.clone(), queue.clone()));
            }
        }).expect("rendering notifier");
    }

    let timer = slint::Timer::default();
    let (w, st2) = (app.as_weak(), st.clone());
    let mut last = Instant::now();
    let mut last_frame = Instant::now();
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(8), move || {
        let Some(app) = w.upgrade() else { return };
        let Ok(mut g) = st2.try_borrow_mut() else { return };
        let s = &mut *g;
        s.drain(&app, &rx);
        if s.unlock_at.is_some_and(|t| Instant::now() >= t) { s.unlock_at = None; hold_focus(false); }
        if s.dirty.is_some_and(|t| t.elapsed() > Duration::from_millis(120)) {
            s.dirty = None;
            let f = s.f();
            s.regen_card(f);
            s.regen_desk();
        }
        if app.window().is_minimized() { last = Instant::now(); return; }
        // full rate while anything moves, then ease off
        let idle = s.scene.time - s.scene.last_input;
        // every UI frame costs ~5 ms of CPU, so only animate at full rate for the person looking at it
        let budget = if !foreground() { 1. / 4. } else if s.scene.busy() { 1. / 60. } else if idle < 10. { 1. / 30. } else if idle < 60. { 1. / 12. } else { 1. / 4. };
        if last_frame.elapsed().as_secs_f32() < budget - 0.002 { return; }
        last_frame = Instant::now();
        let dt = last.elapsed().as_secs_f32();
        last = Instant::now();
        s.scene.tick(dt);
        if s.stage.is_none() {
            let Some((d, q)) = gpu.borrow().clone() else { return };
            s.stage = Some(gpu::Stage::new(d, q));
        }
        let stage = s.stage.as_mut().unwrap();
        for i in 0..s.card_px.len() {
            if let Some(px) = s.card_px[i].take() { stage.set_slab(i, &px); }
        }
        if s.desk_fresh {
            if let Some((_, d)) = &s.desk {
                stage.set_slab(scene::WALL, &d.wall);
                stage.set_slab(scene::BAR, &d.bar);
                stage.set_slab(scene::CLOCK, &d.clock);
            }
            s.desk_fresh = false;
        }
        let size = app.window().size();
        let k = (2560. / size.width.max(1) as f32).min(1.);
        let (rw, rh) = ((size.width as f32 * k) as u32, (size.height as f32 * k) as u32);
        s.scene.panel = (408. * app.window().scale_factor() / size.width.max(1) as f32).min(0.6);
        let has_desk = s.desk.as_ref().is_some_and(|d| d.0 == s.scene.focused());
        let draw = s.scene.draw(rw, rh, has_desk);
        let stage = s.stage.as_mut().unwrap();
        if let Some((a, b)) = stage.render(rw, rh, &draw) {
            if let (Ok(a), Ok(b)) = (Image::try_from(a), Image::try_from(b)) { app.set_stage(a); app.set_overlay(b); }
        }
        app.set_overlay_on(draw.veil);
        let r = ((s.scene.time - s.name_at) / 0.9).clamp(0., 1.);
        if r < 1. || app.get_reveal() < 1. { app.set_reveal(if s.scene.reduced { 1. } else { r }); }
        // ponytail: Slint 1.18's timer frame throttle on Windows re-checks pending_redraw before the draw has run, so
        // every window.request_redraw() costs two full UI renders. One winit redraw draws once; drop this when Slint fixes it.
        use slint::winit_030::WinitWindowAccessor;
        if app.window().with_winit_window(|w| w.request_redraw()).is_none() { app.window().request_redraw(); }
    });

    app.run().unwrap();
}
