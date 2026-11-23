# Changelog

All notable changes to Rice Studio will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- CLI help (`--help` / `-h`) with comprehensive usage and theme list
- Compile-time theme ID validation (`--apply invalid` lists valid themes)
- Unified error types in `src/error.rs`
- File-based logging to `data\rice-studio.log`
- User preferences config file (`data\config.json`)
- GitHub Actions CI workflow (build, test, clippy on Windows)
- CHANGELOG.md following Keep a Changelog format

### Changed
- Removed unused `base64` dependency

### Fixed

## [0.1.0] - 2024-01-XX

### Added
- 12 preset themes with variants
- GUI for theme customization (colors, fonts, layouts, wallpapers)
- Headless CLI modes:
  - `--apply <id>` - Apply theme by ID
  - `--undo` - Undo last apply
  - `--apply --undo` - Restore stock Windows
  - `--supervise` - Background supervisor
  - `--autostart on|off` - Toggle Windows startup
  - `--panic` - Emergency restore
  - `--uninstall` - Restore stock + remove fonts
  - `--wallpaper <n>` - Switch wallpaper
  - `--pack-test <dir> <id>` - Export Zebar widget pack
- Anti-cheat detection (disables LeopardWM/Windhawk during gameplay)
- Panic hotkey (Ctrl+Alt+Shift+Q)
- Font management (~40 WOFF2 families)
- History snapshots for undo
- Wallpaper composition with treatments

### Security
- Theme sanitization against XSS/HTML injection
- Safe path checking for pack files