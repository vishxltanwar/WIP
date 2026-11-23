# Rice Studio - Agent Instructions

## Build & Run

```powershell
cargo build --release        # Windows GUI app (no console window)
cargo test --release         # Tests pass/fail silently
```

## CLI Modes (headless, no UI window)

| Command | Purpose |
|---------|---------|
| `--apply <id>` | Apply theme by ID (e.g., `--apply relic`) |
| `--undo` | Undo last apply |
| `--apply --undo` | Restore stock Windows |
| `--supervise` | Run background supervisor (started at sign-in) |
| `--autostart on\|off` | Toggle Windows startup |
| `--panic` | Emergency: turn everything off, restore stock |
| `--uninstall` | Restore stock + remove installed fonts |
| `--wallpaper <n>` | Switch to wallpaper n for current theme |
| `--pack-test <dir> <id>` | Export Zebar widget pack to folder |

Headless results written to `data\cli-result.txt`.

## Key Files

| File | Purpose |
|------|---------|
| `src/main.rs` | Entrypoint, CLI router, UI wiring |
| `src/theme.rs` | 12 preset themes + sanitisation |
| `src/apply.rs` | Apply/undo, history files in `data\history` |
| `src/system.rs` | Supervisor, anti-cheat detection, panic hotkey |
| `src/apply.rs:35-40` | Data dir is `data/` next to exe |
| `ui/app.slint` | Slint UI definition (slint-build generates Rust) |

## Directory Structure

```
data/               # Runtime data (next to exe)
  history/          # Apply snapshots (subdirs by timestamp)
  stock.json        # Pre-first-apply snapshot
  stock-files/      # Original config files
  art/<theme-id>/   # User's wallpapers

apps/               # Bundled tools (Zebar, LeopardWM, Flow, Windhawk)
```

## Important Details

- **Per-user only**: No admin rights, taskbar auto-hidden (never removed)
- **History**: `data\history` subdirs sorted by timestamp, popped for undo
- **Apps dir**: `apps_dir()` looks 1-4 levels up from exe, falls back to `D:\VagabondRice\apps` for dev
- **Fonts**: ~40 families embedded in WOFF2, unpacked and installed on first apply
- **Anti-cheat**: Games detected by process name; leopardwm/Windhawk disabled during gameplay
- **Panic hotkey**: `Ctrl+Alt+Shift+Q` (registered at startup, released on exit)

## Tests

Run with `cargo test --release`. Tests include:
- Theme sanitisation against XSS/HTML injection
- History snapshot backwards compatibility
- Font WOFF→sfnt conversion
- Wallpaper composition treatments
- Safe path checking for pack files