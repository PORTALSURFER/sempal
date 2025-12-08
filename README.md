
⚠️ **Warning:** Early alpha software. Use at your own risk, this tool can modify, rename, or delete files, and bugs could damage your sample library. Keep backups and proceed with caution. ⚠️

# SEMPAL

Audio sample triage and collection building tool built with Rust and egui.

---

[![Buy Me A Coffee](https://img.buymeacoffee.com/button-api/?text=Buy%20me%20a%20coffee&slug=portalsurfm&button_colour=FFDD00&font_colour=000000&font_family=Inter&outline_colour=000000&coffee_colour=ffffff)](https://buymeacoffee.com/portalsurfm)

## Downloads

- Windows binaries are published on GitHub Releases (Windows only for now).

## Build from source

- Requires Rust (stable toolchain) and `cargo`.
- From the project root: `cargo run --release`.
- Or build once and run the binary: `cargo build --release` then `target/release/sempal`.
- Playback uses your default audio output device.

## Configuration and data

- Each source folder gets a hidden `.sempal_samples.db` that tracks indexed `.wav` files and their tags.
- App config lives in your OS config directory:
  - Linux: `$HOME/.config/com/sempal/sempal/config.json`
  - Windows: `%APPDATA%\com\sempal\sempal\config.json`
  - macOS: `~/Library/Application Support/com.sempal.sempal/config.json`

## Logging

- Startup initializes console logging and a per-launch log file in your OS data directory:
  - Linux: `$HOME/.local/share/com/sempal/sempal/logs`
  - Windows: `%LOCALAPPDATA%\com\sempal\sempal\logs`
  - macOS: `~/Library/Application Support/com.sempal.sempal/logs`
- Log filenames include the launch timestamp, and the 10 most recent files are retained by pruning the oldest.

## Documentation

- [Usage guide](docs/usage.md)
