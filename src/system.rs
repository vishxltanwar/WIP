//! The supervisor and everything that touches running programs: start order, health checks, game mode for
//! anti-cheat, the panic hotkey, start with Windows and the safe start after crashes.
//! Ported from the hand-built Vagabond scripts (rice.ps1, play-league.ps1, zebar-restart.ps1).
use std::{collections::HashMap, os::windows::process::CommandExt, path::PathBuf, process::Command, thread::sleep, time::{Duration, Instant}};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, INVALID_HANDLE_VALUE},
    System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS},
    System::Threading::{CreateMutexW, OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE},
    UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT},
    UI::Shell::{SHAppBarMessage, ABM_SETSTATE, APPBARDATA},
    UI::WindowsAndMessaging::{
        FindWindowExW, FindWindowW, GetMessageW, IsWindowVisible, MessageBoxW, SendMessageW, SetTimer, MB_ICONINFORMATION, MSG, WM_COMMAND,
        WM_HOTKEY, WM_TIMER,
    },
};

const NO_WINDOW: u32 = 0x0800_0000;

/// Where the bundled tools live: `apps` beside the exe in a real install.
pub fn apps_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.ancestors().skip(1).take(4).map(|d| d.join("apps")).find(|d| d.join("zebar").exists())
        // ponytail: dev builds in target\ fall back to the hand-built rice; the installer (Phase 2 follow-up) removes this
        .unwrap_or_else(|| PathBuf::from(r"D:\VagabondRice\apps"))
}
pub fn lwm() -> PathBuf { apps_dir().join("leopardwm").join("lwm.exe") }

/// Programs that mean a game with anti-cheat is starting. Vanguard's own service is left out: it can run all day.
pub const GAMES: &[&str] = &[
    "riotclientservices.exe", "riotclientux.exe", "leagueclient.exe", "leagueclientux.exe", "league of legends.exe",
    "valorant-win64-shipping.exe", "easyanticheat.exe", "easyanticheat_eos.exe", "beservice.exe", "beservice_x64.exe",
    "faceit.exe", "faceitclient.exe",
];

/// name (lower case) → pid and parent pid of every running process
fn processes() -> Vec<(String, u32, u32)> {
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE { return out; }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            out.push((String::from_utf16_lossy(&e.szExeFile[..n]).to_lowercase(), e.th32ProcessID, e.th32ParentProcessID));
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
    }
    out
}
fn running(name: &str) -> bool { processes().iter().any(|p| p.0 == name) }
pub fn bar_running() -> bool { running("zebar.exe") }
pub fn game_running() -> bool { processes().iter().any(|p| GAMES.contains(&p.0.as_str())) }

#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationProcess(h: windows_sys::Win32::Foundation::HANDLE, class: u32, buf: *mut core::ffi::c_void, len: u32, ret: *mut u32) -> i32;
}
/// A process's command line (ProcessCommandLineInformation), lower case.
fn cmdline(pid: u32) -> Option<String> {
    #[repr(C)]
    struct UnicodeString { len: u16, _max: u16, buf: *const u16 }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() { return None; }
        let mut buf = vec![0u64; 2048];
        let mut need = 0u32;
        let mut st = NtQueryInformationProcess(h, 60, buf.as_mut_ptr().cast(), (buf.len() * 8) as u32, &mut need);
        if st < 0 && need as usize > buf.len() * 8 {
            buf = vec![0u64; need as usize / 8 + 1];
            st = NtQueryInformationProcess(h, 60, buf.as_mut_ptr().cast(), (buf.len() * 8) as u32, &mut need);
        }
        CloseHandle(h);
        if st < 0 { return None; }
        let u = &*(buf.as_ptr() as *const UnicodeString);
        if u.buf.is_null() { return None; }
        Some(String::from_utf16_lossy(std::slice::from_raw_parts(u.buf, u.len as usize / 2)).to_lowercase())
    }
}

/// Kill a program and everything it started.
fn kill_tree(name: &str) { let all = processes(); kill_pids(&all, all.iter().filter(|p| p.0 == name).map(|p| p.1).collect()); }
/// Zebar and its WebView2 processes. When zebar.exe crashes those are orphaned, keep port 6124 and show
/// "refused to connect" pages, so they are found by their command line, not only by their parent.
fn stop_bar() {
    let all = processes();
    let roots = all.iter()
        .filter(|p| p.0 == "zebar.exe" || (p.0 == "msedgewebview2.exe" && cmdline(p.1).is_some_and(|c| c.contains("zebar"))))
        .map(|p| p.1).collect();
    kill_pids(&all, roots);
}
fn kill_pids(all: &[(String, u32, u32)], mut doomed: Vec<u32>) {
    let mut i = 0;
    while i < doomed.len() {
        let parent = doomed[i];
        let kids: Vec<u32> = all.iter().filter(|p| p.2 == parent && !doomed.contains(&p.1)).map(|p| p.1).collect();
        doomed.extend(kids);
        i += 1;
    }
    for pid in doomed { terminate(pid); }
}
fn terminate(pid: u32) {
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !h.is_null() { TerminateProcess(h, 1); CloseHandle(h); }
    }
}

fn spawn(exe: PathBuf, args: &[&str]) -> Result<(), String> {
    Command::new(&exe).args(args).current_dir(exe.parent().unwrap_or(&exe)).creation_flags(NO_WINDOW).spawn()
        .map(|_| ()).map_err(|e| format!("{}: {e}", exe.display()))
}
fn bar_up() -> bool { std::net::TcpStream::connect_timeout(&"127.0.0.1:6124".parse().unwrap(), Duration::from_millis(500)).is_ok() }

/// One bar (re)start at a time, across processes. The app (after an apply) and the supervisor (its watchdog) both call
/// start_bar; two overlapping starts kill each other's half-made widgets (no bar) or open every widget twice (two bars).
/// The lock is a file whose age says whether its holder is still at it; a crashed holder's lock goes stale.
const BAR_LOCK_STALE: Duration = Duration::from_secs(150);
fn bar_lock() -> PathBuf { crate::apply::data_dir().join("bar-starting.lock") }
fn bar_starting() -> bool {
    std::fs::metadata(bar_lock()).and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().unwrap_or_default() < BAR_LOCK_STALE)
}
struct BarLock;
impl BarLock {
    fn take() -> Option<BarLock> {
        if !bar_starting() { let _ = std::fs::remove_file(bar_lock()); }
        std::fs::OpenOptions::new().write(true).create_new(true).open(bar_lock()).ok().map(|_| BarLock)
    }
}
impl Drop for BarLock { fn drop(&mut self) { let _ = std::fs::remove_file(bar_lock()); } }

pub fn start_bar() -> Result<(), String> {
    let _lock = match BarLock::take() {
        Some(l) => l,
        None => {
            // someone else is starting it: wait for them, then only step in if they failed
            let t = Instant::now();
            while bar_starting() && t.elapsed() < BAR_LOCK_STALE { sleep(Duration::from_millis(500)); }
            if bar_up() { return Ok(()); }
            BarLock::take().ok_or("the bar is being started by another copy")?
        }
    };
    for _ in 0..3 {
        // a zebar that is running but not serving is stuck: clear it out before every try, never start a second one
        stop_bar();
        let t = Instant::now();
        while bar_up() && t.elapsed() < Duration::from_secs(15) { sleep(Duration::from_millis(500)); }
        spawn(apps_dir().join("zebar").join("zebar.exe"), &["startup"])?;
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(20) {
            sleep(Duration::from_millis(700));
            if bar_up() { return Ok(()); }
            if !running("zebar.exe") { break; }
        }
        sleep(Duration::from_secs(3));
    }
    Err("the bar did not start".into())
}
fn start_wm() -> Result<(), String> { if running("leopardwm.exe") { return Ok(()); } spawn(apps_dir().join("leopardwm").join("leopardwm.exe"), &[]) }
fn stop_wm() {
    let _ = Command::new(lwm()).arg("stop").creation_flags(NO_WINDOW).status();
    sleep(Duration::from_millis(500));
    kill_tree("leopardwm.exe");
}
fn start_hooks() -> Result<(), String> { if running("windhawk.exe") { return Ok(()); } spawn(apps_dir().join("windhawk").join("windhawk.exe"), &["-tray-only"]) }
/// unloads every Windhawk mod from every process
fn stop_hooks() { let _ = Command::new(apps_dir().join("windhawk").join("windhawk.exe")).args(["-exit", "-wait"]).creation_flags(NO_WINDOW).status(); }
// a second Flow launch opens its search box, so never start one next to a running one
fn start_flow() -> Result<(), String> { if running("flow.launcher.exe") { return Ok(()); } spawn(apps_dir().join("flow").join("Flow.Launcher.exe"), &[]) }

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain([0]).collect() }

/// Taskbar auto-hide, applied live (Explorer saves it itself). Never killed, only hidden.
fn taskbar_autohide(on: bool) {
    unsafe {
        let mut d: APPBARDATA = std::mem::zeroed();
        d.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
        d.hWnd = FindWindowW(wide("Shell_TrayWnd").as_ptr(), std::ptr::null());
        d.lParam = if on { 1 } else { 2 }; // ABS_AUTOHIDE : ABS_ALWAYSONTOP
        SHAppBarMessage(ABM_SETSTATE, &mut d);
    }
}
/// Same toggle as right-click the desktop, View, Show desktop icons.
fn desktop_icons(show: bool) {
    unsafe {
        let null = std::ptr::null();
        let mut view = FindWindowExW(FindWindowW(wide("Progman").as_ptr(), null), std::ptr::null_mut(), wide("SHELLDLL_DefView").as_ptr(), null);
        let mut w = std::ptr::null_mut();
        while view.is_null() {
            w = FindWindowExW(std::ptr::null_mut(), w, wide("WorkerW").as_ptr(), null);
            if w.is_null() { return; }
            view = FindWindowExW(w, std::ptr::null_mut(), wide("SHELLDLL_DefView").as_ptr(), null);
        }
        let list = FindWindowExW(view, std::ptr::null_mut(), wide("SysListView32").as_ptr(), null);
        if (IsWindowVisible(list) != 0) != show { SendMessageW(view, WM_COMMAND, 0x7402, 0); }
    }
}

/// Everything on, in the order that keeps the bar docked: hooks, tiler, then the bar, then search.
pub fn start_all(game: bool) -> Vec<String> {
    let mut errs = Vec::new();
    taskbar_autohide(true);
    desktop_icons(false);
    if !game {
        if let Err(e) = start_hooks() { errs.push(e); }
        if let Err(e) = start_wm() { errs.push(e); }
        sleep(Duration::from_secs(3));
    }
    if let Err(e) = start_bar() { errs.push(e); }
    if let Err(e) = start_flow() { errs.push(e); }
    errs
}
pub fn stop_all() {
    stop_wm();
    stop_hooks();
    kill_tree("flow.launcher.exe");
    stop_bar();
    taskbar_autohide(false);
    desktop_icons(true);
}
/// Game mode: nothing hooks into or moves the game's windows. The bar and search stay.
fn game_mode(on: bool) {
    if on { stop_wm(); stop_hooks(); } else {
        let _ = start_hooks();
        let _ = start_wm();
        sleep(Duration::from_secs(3));
        let _ = start_bar(); // a fresh tiler drops the bar's reserved space; restarting the bar docks it again
    }
}

/// Stock Windows: everything stopped, the original wallpaper and bar settings back, nothing starts at sign-in.
pub fn panic() -> Result<(), String> {
    crate::logging::info("panic: emergency restore");
    stop_supervisor(); // or it would restart the bar and the tiler a moment later
    stop_all();
    set_autostart(false)?;
    crate::apply::restore_stock()
}

/// The supervisor could not get Ctrl+Alt+Shift+Q the last time it started.
pub fn hotkey_taken() -> bool { std::fs::read_to_string(crate::apply::data_dir().join("hotkey.txt")).is_ok_and(|s| s == "taken") }

/// Uninstall: every other running copy of this exe, the open window included, so its files can be deleted.
pub fn stop_other_copies() {
    let me = std::process::id();
    let exe = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase())).unwrap_or_default();
    for p in processes().iter().filter(|p| p.0 == exe && p.1 != me) { terminate(p.1); }
}

/// Ends a background supervisor started by another copy of this exe (never this process). Only the supervisor
/// itself: the tools it started are stopped properly by stop_all (Windhawk must unload its mods, not be killed).
/// A deliberate stop (Undo, Off, uninstall) is not a failed start: the guard only counts crashes.
pub fn stop_supervisor() { for pid in supervisors() { terminate(pid); } StartGuard::new(crate::apply::data_dir()).healthy(); }
fn supervisors() -> Vec<u32> {
    let me = std::process::id();
    let exe = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase())).unwrap_or_default();
    processes().into_iter().filter(|p| p.0 == exe && p.1 != me && cmdline(p.1).is_some_and(|c| c.contains("--supervise"))).map(|p| p.1).collect()
}

/// Any of the bundled tools running (they may have been started by hand, not by Rice Studio).
pub fn any_tool_running() -> bool { ["zebar.exe", "leopardwm.exe", "windhawk.exe", "flow.launcher.exe"].iter().any(|n| running(n)) }
/// Flow saves its own settings when it closes, so it is stopped before a theme writes them. Returns whether it ran.
pub fn stop_flow() -> bool { let on = running("flow.launcher.exe"); if on { kill_tree("flow.launcher.exe"); } on }
/// The tools that are running pick up new settings (never the hooks while a game runs).
pub fn reload_running(flow_was_on: bool) -> Result<(), String> {
    if running("leopardwm.exe") { let _ = Command::new(lwm()).arg("reload").creation_flags(NO_WINDOW).status(); }
    if running("windhawk.exe") && !game_running() { stop_hooks(); let _ = start_hooks(); }
    if flow_was_on { let _ = start_flow(); }
    if bar_running() { start_bar() } else { Ok(()) }
}
/// After a theme wrote new settings: running tools reload, then the supervisor makes sure everything else is up
/// and stays up (it starts in the right order whatever is not running yet).
pub fn after_theme(flow_was_on: bool) -> Result<(), String> {
    reload_running(flow_was_on)?;
    if !supervisors().is_empty() { return Ok(()); }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    spawn(exe, &["--supervise"])
}

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
pub fn set_autostart(on: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut c = Command::new("reg");
    if on { c.args(["add", RUN_KEY, "/v", "RiceStudio", "/t", "REG_SZ", "/d", &format!("\"{}\" --supervise", exe.display()), "/f"]); }
    else { c.args(["delete", RUN_KEY, "/v", "RiceStudio", "/f"]); }
    let ok = c.creation_flags(NO_WINDOW).output().map_err(|e| e.to_string())?.status.success();
    if ok || !on { Ok(()) } else { Err("could not set start with Windows".into()) }
}
pub fn autostart() -> bool {
    Command::new("reg").args(["query", RUN_KEY, "/v", "RiceStudio"]).creation_flags(NO_WINDOW).output().is_ok_and(|o| o.status.success())
}

/// Two sign-ins in a row that never reached a minute of healthy running: the next one comes up plain.
pub struct StartGuard { path: PathBuf }
impl StartGuard {
    pub fn new(dir: PathBuf) -> StartGuard { StartGuard { path: dir.join("failed-starts.txt") } }
    fn count(&self) -> u32 { std::fs::read_to_string(&self.path).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0) }
    /// call at start; false means come up plain this time
    pub fn begin(&self) -> bool {
        let n = self.count();
        if n >= 2 { let _ = std::fs::write(&self.path, "0"); return false; }
        let _ = std::fs::write(&self.path, (n + 1).to_string());
        true
    }
    pub fn healthy(&self) { let _ = std::fs::write(&self.path, "0"); }
}

fn notice(text: &str) {
    let t = text.to_string();
    std::thread::spawn(move || unsafe {
        MessageBoxW(std::ptr::null_mut(), wide(&t).as_ptr(), wide("Rice Studio").as_ptr(), MB_ICONINFORMATION);
    });
}

/// `rice-studio.exe --supervise`: started at sign-in, lives in the background, costs one process list every 2 s.
pub fn supervise() {
    unsafe {
        CreateMutexW(std::ptr::null(), 0, wide("Local\\RiceStudioSupervisor").as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS { return; }
    }
    let guard = StartGuard::new(crate::apply::data_dir());
    let plain = !guard.begin();
    if plain {
        notice("Rice Studio did not start cleanly twice in a row, so this time Windows came up plain. It will try again at your next sign-in. Press Ctrl+Alt+Shift+Q at any time to switch everything off.");
    }
    // another program (a Start-menu shortcut key, say) can already own the combination; the app says so
    let hotkey = unsafe { RegisterHotKey(std::ptr::null_mut(), 1, MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT, 'Q' as u32) } != 0;
    let _ = std::fs::write(crate::apply::data_dir().join("hotkey.txt"), if hotkey { "ok" } else { "taken" });
    let mut game = game_running();
    if !plain {
        let errs = start_all(game);
        if !errs.is_empty() { notice(&format!("Some parts did not start:\n{}", errs.join("\n"))); }
    }
    let started = Instant::now();
    let mut healthy_marked = plain;
    let mut game_gone_since: Option<Instant> = None;
    let mut restarts: HashMap<&str, Vec<Instant>> = HashMap::new();
    unsafe { SetTimer(std::ptr::null_mut(), 0, 2000, None) };
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
        match msg.message {
            WM_HOTKEY => {
                let r = panic();
                notice(&match r { Ok(()) => "Everything is off and Windows is back to how it was. Open Rice Studio to turn it on again.".into(), Err(e) => format!("Switched off, but: {e}") });
                return;
            }
            WM_TIMER if !plain => {
                let now_game = game_running();
                if now_game && !game { game = true; game_mode(true); }
                if !now_game && game {
                    // wait 10 s after the last game process closes; clients restart between matches
                    let since = *game_gone_since.get_or_insert_with(Instant::now);
                    if since.elapsed() > Duration::from_secs(10) { game = false; game_gone_since = None; game_mode(false); }
                } else { game_gone_since = None; }
                // restart what crashed, at most 3 times in 10 minutes each
                let mut checks: Vec<(&str, fn() -> Result<(), String>)> = vec![("zebar.exe", start_bar)];
                if !game { checks.push(("leopardwm.exe", start_wm)); }
                for (name, start) in checks {
                    // a bar process that no longer serves its widgets counts as crashed
                    if running(name) && (name != "zebar.exe" || bar_up()) { continue; }
                    // the app is restarting the bar for a new theme: leave it to finish
                    if name == "zebar.exe" && bar_starting() { continue; }
                    let r = restarts.entry(name).or_default();
                    r.retain(|t| t.elapsed() < Duration::from_secs(600));
                    if r.len() < 3 { r.push(Instant::now()); let _ = start(); }
                }
                if !healthy_marked && started.elapsed() > Duration::from_secs(60) && bar_up() { guard.healthy(); healthy_marked = true; }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_list_sees_this_test() {
        let me = std::process::id();
        assert!(processes().iter().any(|p| p.1 == me));
        assert!(GAMES.iter().all(|g| g.ends_with(".exe") && g.to_lowercase() == *g));
    }
    #[test]
    fn reads_a_command_line() {
        let me = std::env::current_exe().unwrap();
        let stem = me.file_stem().unwrap().to_string_lossy().to_lowercase();
        assert!(cmdline(std::process::id()).is_some_and(|c| c.contains(&stem)));
    }
    #[test]
    fn two_failed_starts_then_plain() {
        let dir = std::env::temp_dir().join(format!("rs-guard-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let g = StartGuard::new(dir.clone());
        assert!(g.begin());
        assert!(g.begin());
        assert!(!g.begin(), "third start after two failures comes up plain");
        assert!(g.begin(), "and the one after tries again");
        g.healthy();
        assert!(g.begin() && g.begin());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
