# clipboard2path-wsl

English | [日本語](README.ja.md)

A lightweight daemon for WSL2 that automatically saves clipboard images to files and lets you paste their paths via shell hooks.

**Never writes to the clipboard** -- designed so image pasting on the Windows side (Slack, etc.) keeps working.

## Problem

On WSL2, there are cases where an image stored in the clipboard cannot be pasted directly.
WSLg synchronizes the clipboard bidirectionally over the RDP CLIPRDR channel, so writing to
the Wayland clipboard with `wl-copy` also overwrites the Windows clipboard.

`clipboard2path-wsl` solves this by using the clipboard **read-only** and delivering paths
to shell hooks through files instead.

## How it works

1. Watches the clipboard — event-driven via X11 XFixes owner-change notifications, falling back to polling (`wl-paste` only) when X11 is unavailable
2. Grabs the image (BMP) when one is detected
3. Converts it to PNG and saves it under `$XDG_RUNTIME_DIR/clipboard2path/`
4. Updates the `latest-path` file and the `latest.png` symlink
5. The shell's Alt+V hook reads `latest-path` and inserts the path
6. The wl-paste wrapper (`~/.local/bin/wl-paste`) answers `image/png` requests with `latest.png`, enabling image paste in Claude Code and similar tools

## Requirements

- WSL2 (with WSLg enabled)
- `wl-paste` (`wl-clipboard` package)
- Rust toolchain (build time only)

```bash
# Ubuntu/Debian
sudo apt install wl-clipboard
```

## Installation

### One-liner (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/ba0918/clipboard2path-wsl/main/scripts/install.sh | bash
```

Installs the binary into `~/.local/bin`. Override with `INSTALL_DIR`:

```bash
INSTALL_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/ba0918/clipboard2path-wsl/main/scripts/install.sh | bash
```

### Build from source

```bash
git clone https://github.com/ba0918/clipboard2path-wsl.git
cd clipboard2path-wsl
cargo install --path .
```

## Setup

A single `init` installs the shell hook, the systemd service, and the wl-paste wrapper, and starts the daemon:

```bash
# Auto-detect (from $SHELL)
clipboard2path-wsl init

# Specify the shell explicitly
clipboard2path-wsl init fish
clipboard2path-wsl init bash
clipboard2path-wsl init zsh
```

After reloading your shell (open a new terminal), pressing Alt+V inserts the file path
when the clipboard holds an image, or inserts the text when it holds text.

Check the current state:

```bash
clipboard2path-wsl status
```

## Usage

### Daemon mode (default)

```bash
clipboard2path-wsl
```

Watches the clipboard and saves each detected image to
`$XDG_RUNTIME_DIR/clipboard2path/clipboard-{timestamp}.png`.

Detection is event-driven (X11 XFixes selection notifications — near-zero
latency, zero idle wakeups). When no X server is reachable, the daemon falls
back to polling at `--interval` (default 500ms).

### One-shot

```bash
clipboard2path-wsl --once
```

### Setup management

```bash
clipboard2path-wsl init [fish|bash|zsh]         # Install (hook + service + wrapper)
clipboard2path-wsl init --force [fish|bash|zsh] # Force overwrite
clipboard2path-wsl uninstall [fish|bash|zsh]    # Uninstall (remove hook + service + wrapper)
clipboard2path-wsl status                       # Show status
```

### Options

```
COMMANDS:
    (default)           Watch the clipboard (daemon mode)
    init [SHELL]        Install the shell hook and systemd service
    uninstall [SHELL]   Uninstall the shell hook and systemd service
    status              Show the current status

WATCH OPTIONS:
    --once              Run once (no daemon loop)
    --poll              Force polling mode (default: X11 event-driven, polling fallback)
    --interval <ms>     Polling interval in ms (100-60000, default: 500)
    --output-dir <path> Output directory (default: $XDG_RUNTIME_DIR/clipboard2path/)
    --max-files <n>     Maximum number of files to keep (min: 1, default: 20)
    --verbose           Verbose logging
    -q, --quiet         Suppress all output except errors

INIT OPTIONS:
    -f, --force         Force-overwrite an existing hook
    --no-service        Skip installing the systemd service

UNINSTALL OPTIONS:
    --no-service        Skip removing the systemd service

GLOBAL OPTIONS:
    -h, --help          Show help
    -v, --version       Show version
```

### Examples

```bash
# Poll every second, save to an explicit directory
clipboard2path-wsl --interval 1000 --output-dir ~/Pictures

# Run with verbose logging
clipboard2path-wsl --verbose
```

## Autostart with systemd

The `init` command also installs and enables a systemd user service:

```bash
clipboard2path-wsl init          # Shell hook + systemd service
clipboard2path-wsl init --no-service  # Shell hook only
```

Manual control:

```bash
systemctl --user status clipboard2path   # Check status
systemctl --user restart clipboard2path  # Restart
journalctl --user -u clipboard2path -f   # Follow logs
```

SIGTERM/SIGINT trigger a clean shutdown (runtime directory cleanup).

## Building

```bash
cargo build --release    # Release build (~800KB)
cargo test               # Run tests (233 tests)
cargo clippy             # Lint
```

## Architecture

```
src/
  main.rs                  # Entry point (DI wiring + subcommand routing)
  domain/                  # Pure functions (no I/O)
    image_convert.rs       #   BMP -> PNG conversion
    path_gen.rs            #   Output path generation
    wsl_detect.rs          #   WSL2 environment detection
    clipboard_change.rs    #   Clipboard change detection
    runtime_dir.rs         #   Runtime directory resolution
    cleanup.rs             #   Temporary file cleanup
    cli.rs                 #   CLI argument parsing (with subcommands)
    shell_detect.rs        #   Shell detection
    shell_hook.rs          #   Shell hook generation
    path_validate.rs       #   Path validation for shell/systemd embedding
    systemd_unit.rs        #   systemd unit file generation
    wl_paste_wrapper.rs    #   wl-paste wrapper script generation
  infra/                   # I/O layer (abstracted behind traits)
    change_signal.rs       #   Event-driven change detection (X11 XFixes)
    clipboard.rs           #   wl-paste invocation (read-only)
    command_runner.rs      #   External command execution abstraction
    file_system.rs         #   File writing
    path_notifier.rs       #   Path notification (latest-path + symlink)
    lifecycle.rs           #   Daemon lifecycle management
    shell_installer.rs     #   Shell hook installation
    systemd_installer.rs   #   systemd unit installation/enabling
    wrapper_installer.rs   #   wl-paste wrapper installation (marker-based ownership check)
  service/                 # Orchestration
    converter.rs           #   Conversion flow
    daemon.rs              #   Polling loop
```

- **Domain layer**: All pure functions. Zero external dependencies.
- **Infra layer**: Abstracted behind traits, DI-friendly. Swapped for mocks in tests.
- **Service layer**: Only calls domain functions. No business logic.

## Design highlights

- **Clipboard is never written**: Uses `wl-paste` only. The Windows clipboard is untouched.
- **Event-driven detection**: Subscribes to X11 XFixes CLIPBOARD owner-change notifications (WSLg mirrors every Windows-side copy to X11), so images are detected near-instantly with zero idle wakeups. Only the notification comes from X11 — data still flows read-only through `wl-paste`. Falls back to polling when X11 is unavailable.
- **File-based path notification**: `latest-path` file + `latest.png` symlink.
- **Atomic updates**: Temp file -> rename keeps both the path file and the symlink update safe.
- **Shell hook integration**: Alt+V inserts the path for an image clipboard, or performs a normal paste for text.
- **wl-paste wrapper**: Answers `image/png` requests with the daemon-saved PNG, enabling image paste in Claude Code and similar tools. Existing files are protected by a marker-based ownership check.

## License

[MIT](LICENSE)
