//! Apply and undo on the real desktop. Every apply is kept as a step in `data\history`, so Undo walks back one
//! apply at a time; the state from before Rice Studio's first apply is kept as `stock.json` for the very last
//! Undo, the panic hotkey and uninstall.
use crate::components::{self, Components};
use serde::{Deserialize, Serialize};
use std::{fs, os::windows::process::CommandExt, path::{Path, PathBuf}, process::Command};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_GETDESKWALLPAPER, SPI_SETDESKWALLPAPER,
};

pub const PACK: &str = "rice-studio";
const NO_WINDOW: u32 = 0x0800_0000;
const KEEP: usize = 10;

pub enum Data { Text(String), Bytes(Vec<u8>) }
pub struct PackFile { pub path: String, pub data: Data }

#[derive(Serialize, Deserialize)]
struct Stock {
    settings: String,
    wallpaper: String,
    style: String,
    /// every config file a theme writes (components.rs): (path, whether it existed); originals in stock-files\<index>
    #[serde(default)]
    files: Vec<(String, bool)>,
    /// tools that were already running before Rice Studio (started by hand) stay running after the last Undo
    /// (a snapshot that predates this field assumes they were: never stop tools Rice Studio did not start)
    #[serde(default = "yes")]
    tools_were_running: bool,
}
fn yes() -> bool { true }
fn stock_files() -> PathBuf { data_dir().join("stock-files") }

fn zebar_dir() -> PathBuf { PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join(".glzr").join("zebar") }
/// portable: data lives next to the exe, never in %APPDATA%
pub fn data_dir() -> PathBuf {
    let d = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.join("data"))).unwrap_or_else(|| "data".into());
    let _ = fs::create_dir_all(&d);
    d
}
fn stock_path() -> PathBuf { data_dir().join("stock.json") }
fn history_dir() -> PathBuf { data_dir().join("history") }
fn e<T: std::fmt::Display>(x: T) -> String { x.to_string() }

/// applied steps, oldest first (names are zero-padded timestamps, so they sort)
fn history() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(history_dir()).map(|r| r.flatten().map(|d| d.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
    v.sort();
    v
}

fn reg(args: &[&str]) -> Result<String, String> {
    let o = Command::new("reg").args(args).creation_flags(NO_WINDOW).output().map_err(e)?;
    if !o.status.success() { return Err(String::from_utf8_lossy(&o.stderr).into()); }
    Ok(String::from_utf8_lossy(&o.stdout).into())
}
fn current_wallpaper() -> String {
    let mut buf = [0u16; 1024];
    unsafe { SystemParametersInfoW(SPI_GETDESKWALLPAPER, buf.len() as u32, buf.as_mut_ptr().cast(), 0) };
    String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)])
}
fn current_style() -> String {
    reg(&["query", r"HKCU\Control Panel\Desktop", "/v", "WallpaperStyle"]).ok()
        .and_then(|s| s.split_whitespace().last().map(String::from)).unwrap_or_else(|| "10".into())
}
fn set_wallpaper(path: &str, style: &str) -> Result<(), String> {
    reg(&["add", r"HKCU\Control Panel\Desktop", "/v", "WallpaperStyle", "/t", "REG_SZ", "/d", style, "/f"])?;
    reg(&["add", r"HKCU\Control Panel\Desktop", "/v", "TileWallpaper", "/t", "REG_SZ", "/d", "0", "/f"])?;
    let mut w: Vec<u16> = path.encode_utf16().chain([0]).collect();
    let ok = unsafe { SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, w.as_mut_ptr().cast(), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) };
    if ok == 0 { Err("Windows refused the wallpaper".into()) } else { Ok(()) }
}

/// packages are data: relative paths inside the pack only
fn safe_rel(p: &str) -> bool { !(p.is_empty() || p.contains("..") || p.contains(':') || p.starts_with(['/', '\\'])) }

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(e)?;
    for f in fs::read_dir(from).map_err(e)?.flatten() {
        let t = to.join(f.file_name());
        if f.path().is_dir() { copy_dir(&f.path(), &t)?; } else { fs::copy(f.path(), t).map_err(e)?; }
    }
    Ok(())
}

/// Write one history step to disk without touching the desktop.
fn save_step(files: Vec<PackFile>, wallpaper_png: &[u8], parts: &Components) -> Result<PathBuf, String> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(e)?.as_millis();
    let step = history_dir().join(format!("{stamp:016}"));
    for f in files {
        if !safe_rel(&f.path) { return Err(format!("bad path in pack: {}", f.path)); }
        let p = step.join("pack").join(&f.path);
        fs::create_dir_all(p.parent().unwrap()).map_err(e)?;
        match f.data { Data::Text(t) => fs::write(&p, t), Data::Bytes(b) => fs::write(&p, b) }.map_err(e)?;
    }
    fs::write(step.join("wallpaper.png"), wallpaper_png).map_err(e)?;
    fs::write(step.join("components.json"), serde_json::to_string(parts).map_err(e)?).map_err(e)?;
    Ok(step)
}

/// Put a saved step on the desktop: its pack becomes Zebar's, its picture the wallpaper, and the tiler, search and
/// Windows styling get the theme's settings.
fn activate(step: &Path) -> Result<(), String> {
    let zdir = zebar_dir();
    let pack = zdir.join(PACK);
    let _ = fs::remove_dir_all(&pack);
    copy_dir(&step.join("pack"), &pack)?;
    // start every widget the pack has (a full rice: bar, dashboard, desk; an older pack: the bar alone)
    let widgets: Vec<String> = fs::read_to_string(pack.join("zpack.json")).ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["widgets"].as_array().map(|a| a.iter().filter_map(|w| w["name"].as_str().map(String::from)).collect()))
        .filter(|v: &Vec<String>| !v.is_empty()).unwrap_or_else(|| vec!["bar".into()]);
    let cfg = serde_json::json!({
        "$schema": "https://github.com/glzr-io/zebar/raw/v3.3.1/resources/settings-schema.json",
        "startupConfigs": widgets.iter().map(|w| serde_json::json!({ "pack": PACK, "widget": w, "preset": "default" })).collect::<Vec<_>>()
    });
    fs::write(zdir.join("settings.json"), serde_json::to_string_pretty(&cfg).map_err(e)?).map_err(e)?;
    set_wallpaper(&step.join("wallpaper.png").to_string_lossy(), "10")?;
    let flow_on = crate::system::stop_flow();
    // steps saved before Rice Studio themed the other tools carry no components: those files are left alone
    let parts: Option<Components> = fs::read_to_string(step.join("components.json")).ok().and_then(|t| serde_json::from_str(&t).ok());
    if let Some(p) = parts { components::write(&p)?; }
    crate::system::after_theme(flow_on)
}

pub fn apply(files: Vec<PackFile>, wallpaper_png: Vec<u8>, parts: Components, fonts: &[&str]) -> Result<(), String> {
    crate::logging::info("apply: starting");
    let old: Option<Stock> = fs::read_to_string(stock_path()).ok().and_then(|t| serde_json::from_str(&t).ok());
    let stock = match old {
        None => Some(Stock {
            settings: fs::read_to_string(zebar_dir().join("settings.json")).unwrap_or_default(),
            wallpaper: current_wallpaper(),
            style: current_style(),
            files: snapshot_files()?,
            tools_were_running: crate::system::any_tool_running(),
        }),
        // a snapshot from before themes reached the other tools: those files are still the originals, save them now
        Some(mut s) if s.files.is_empty() => { s.files = snapshot_files()?; Some(s) }
        Some(_) => None,
    };
    if let Some(s) = stock { fs::write(stock_path(), serde_json::to_string_pretty(&s).map_err(e)?).map_err(e)?; }
    let step = save_step(files, &wallpaper_png, &parts)?;
    let h = history();
    for old in &h[..h.len().saturating_sub(KEEP)] { let _ = fs::remove_dir_all(old); }
    // Windows Terminal and friends can only use the theme's type once it is installed for the user
    crate::fonts::install(fonts)?;
    activate(&step)
}

/// The files a theme may write, as they are now, kept in stock-files.
fn snapshot_files() -> Result<Vec<(String, bool)>, String> {
    let _ = fs::remove_dir_all(stock_files());
    fs::create_dir_all(stock_files()).map_err(e)?;
    let mut saved = Vec::new();
    for (i, f) in components::files().into_iter().enumerate() {
        let existed = f.is_file();
        if existed { fs::copy(&f, stock_files().join(i.to_string())).map_err(e)?; }
        saved.push((f.to_string_lossy().into_owned(), existed));
    }
    Ok(saved)
}

/// One apply back; past the first apply, back to how the desktop was before Rice Studio.
pub fn undo() -> Result<(), String> {
    crate::logging::info("undo: starting");
    let mut h = history();
    let last = h.pop().ok_or("Nothing to undo")?;
    fs::remove_dir_all(&last).map_err(e)?;
    match h.last() { Some(prev) => activate(prev), None => restore_stock() }
}

/// The desktop exactly as it was before the first apply. Safe to call when nothing was ever applied.
pub fn restore_stock() -> Result<(), String> {
    crate::logging::info("restore_stock: starting");
    let _ = fs::remove_dir_all(history_dir());
    let Ok(text) = fs::read_to_string(stock_path()) else { return Ok(()) };
    let s: Stock = serde_json::from_str(&text).map_err(e)?;
    let zdir = zebar_dir();
    fs::write(zdir.join("settings.json"), &s.settings).map_err(e)?;
    let _ = fs::remove_dir_all(zdir.join(PACK));
    if !s.wallpaper.is_empty() { set_wallpaper(&s.wallpaper, &s.style)?; }
    let flow_on = crate::system::stop_flow();
    for (i, (path, existed)) in s.files.iter().enumerate() {
        if *existed { fs::copy(stock_files().join(i.to_string()), path).map_err(e)?; } else { let _ = fs::remove_file(path); }
    }
    let _ = fs::remove_dir_all(components::terminal_fragment().parent().unwrap());
    fs::remove_file(stock_path()).map_err(e)?;
    let _ = fs::remove_dir_all(stock_files());
    if s.tools_were_running {
        // they were there before Rice Studio: keep them, on their own settings again
        crate::system::stop_supervisor();
        crate::system::reload_running(flow_on)?;
    } else {
        crate::system::stop_supervisor();
        crate::system::stop_all();
    }
    Ok(())
}

/// The bar's wallpaper picker: one of the theme's wallpapers (0 up), or the number after the last for the wallpaper
/// made from your pictures (the pack is written afresh on every apply, so no other theme's wallpapers linger).
pub fn pick_wallpaper(n: usize) -> Result<(), String> {
    let own = zebar_dir().join(PACK).join("art").join(format!("wall-{n}.jpg"));
    let path = if own.is_file() { own } else { history().pop().ok_or("Nothing applied yet")?.join("wallpaper.png") };
    if !path.is_file() { return Err(format!("{} is missing", path.display())); }
    set_wallpaper(&path.to_string_lossy(), "10")
}

pub fn can_undo() -> bool { !history().is_empty() }

#[cfg(test)]
mod tests {
    #[test]
    fn a_snapshot_from_an_older_version_never_stops_the_users_tools() {
        let s: super::Stock = serde_json::from_str(r#"{"settings":"{}","wallpaper":"w.png","style":"10"}"#).unwrap();
        assert!(s.tools_were_running && s.files.is_empty());
    }
    #[test]
    fn pack_paths_stay_inside_the_pack() {
        for bad in ["", "../x", "..\\x", "C:\\x", "/x", "\\x", "a/../../b"] { assert!(!super::safe_rel(bad), "{bad}"); }
        for ok in ["bar.html", "fonts/anton.woff2", "lib/zebar.js"] { assert!(super::safe_rel(ok), "{ok}"); }
    }
}
