# Distribution Instructions

## Building

1. Install Rust (https://rustup.rs)
2. Clone the repository:
   ```bash
   git clone https://github.com/mzqef/IntelliBoard.git
   cd IntelliBoard
   ```
3. Build in release mode:
   ```bash
   cargo build --release
   ```

## Packaging

- The release binary will be in `target/release/IntelliBoard(.exe)`
- Copy the `config/` directory next to the binary for runtime configuration
- Optionally include `README.md`, `LICENSE`, and example `.env`

## Windows Distribution
- Zip the following files:
  - `target/release/IntelliBoard.exe`
  - `config/`
  - `README.md`, `LICENSE`, `.env.example`

## Linux/macOS Distribution
- Tarball the following files:
  - `target/release/IntelliBoard`
  - `config/`
  - `README.md`, `LICENSE`, `.env.example`

### Linux prerequisites

IntelliBoard on Linux relies on several system tools and libraries. Install
them with your distribution's package manager:

**X11 (traditional desktop):**
```bash
# Debian / Ubuntu
sudo apt install libxcb-randr0-dev libxcb-xfixes0-dev libx11-dev libxkbcommon-dev \
    libssl-dev pkg-config xdotool wmctrl

# Fedora / RHEL
sudo dnf install xcb-util-devel libX11-devel libxkbcommon-devel openssl-devel \
    pkg-config xdotool wmctrl

# Arch Linux
sudo pacman -S libxcb libx11 libxkbcommon openssl pkgconf xdotool wmctrl
```

**Wayland:**
- The clipboard polling listener works on Wayland via `arboard` (wl-clipboard
  / Mutter clipboard bridge).
- Global hotkeys via `rdev::grab` require `/dev/uinput` write permission.
  Without it, hotkeys will not fire — the floating toolbar (triggered by
  clipboard copy) still works.
- `xdotool` / `wmctrl` are X11-only; selection-popup positioning falls back
  to a fixed default on Wayland.
- For `/dev/uinput` access:
  ```bash
  sudo usermod -aG input $USER
  # Log out and back in for the group change to take effect.
  ```

**System tray:**
- IntelliBoard uses `tray-icon` which needs a StatusNotifierWatcher (AppIndicator)
  compatible tray. On GNOME, install:
  ```bash
  sudo apt install gnome-shell-extension-appindicator  # Ubuntu
  ```
  KDE Plasma and most other desktop environments include tray support by default.

### `.desktop` file (system integration)

Install `scripts/linux/intelliboard.desktop` to
`~/.local/share/applications/` for application menu integration:

```bash
mkdir -p ~/.local/share/applications
cp scripts/linux/intelliboard.desktop ~/.local/share/applications/
update-desktop-database ~/.local/share/applications/
```

Edit the `Exec=` and `Icon=` paths in the `.desktop` file to point to where
you installed IntelliBoard.

### Autostart (optional)

Copy the `.desktop` file to `~/.config/autostart/` to start IntelliBoard on
login:

```bash
mkdir -p ~/.config/autostart
cp scripts/linux/intelliboard.desktop ~/.config/autostart/
```

## Example .env
```
API_KEY=sk-your-actual-key-here
```

See `.env.example` in the repository root for a ready-to-copy template.

## License
Distributed under the GPLv3. See LICENSE for details.
