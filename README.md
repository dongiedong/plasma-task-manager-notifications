# Plasma Task Manager Notifications

Taskbar badge notifications for KDE Plasma 6. Dynamically discovers running applications via KWin, monitors desktop notifications via D-Bus, and sets badge counts on taskbar icons using the Unity Launcher API. No hardcoded app list needed.

## How it works

1. **Discovers apps dynamically** by querying KWin via D-Bus — enumerates open windows using `WindowsRunner.Match`, then calls `KWin.getWindowInfo` for each to get the `desktopFile` property. Rediscovers every 30 seconds in a background thread.
2. Runs an **unfiltered `dbus-monitor --session`** subprocess (unfiltered is required to capture method return messages which contain the assigned notification ID).
3. Parses the dbus-monitor output as a stream, tracking three message types:
   - `method_call` with `member=Notify` — extract the `app_name` and message `serial`
   - `method_return` with matching `reply_serial` — extract the notification ID
   - `signal` with `member=NotificationClosed` — notification was dismissed
4. Matches `app_name` (case-insensitive) against the dynamically discovered app map. Also supports Flatpak apps via `desktop-entry` hints.
5. Emits `com.canonical.Unity.LauncherEntry.Update` signals via `gdbus emit` to set/clear badge counts and the `urgent` flag on the corresponding taskbar icon.
6. On shutdown (SIGINT/SIGTERM), clears all badges.

## Requirements

- KDE Plasma 6 with KWin and Task Manager
- `dbus-monitor` (from `dbus-tools`, typically pre-installed)
- `gdbus` (from `glib2`, typically pre-installed)
- Rust toolchain (to build)

## Install

```bash
git clone ssh://git@git.nmm.ee/asko/plasma-task-manager-notifications.git
cd plasma-task-manager-notifications
make install
```

This installs the binary to `~/.local/bin/` and the systemd service to `~/.config/systemd/user/`.

### Enable at login

```bash
systemctl --user enable --now plasma-task-manager-notifications.service
```

### Check status

```bash
systemctl --user status plasma-task-manager-notifications.service
journalctl --user -u plasma-task-manager-notifications.service -f
```

## Uninstall

```bash
make uninstall
```

## Usage

Run manually (logs to stderr):

```bash
plasma-task-manager-notifications
```

Test with a sample notification:

```bash
notify-send --app-name=firefox "Test" "Badge should appear"
```

## Development

```bash
cargo test        # run all tests
cargo build       # debug build
make build        # release build
```

## Architecture

The crate is split into four modules:

| Module | Purpose |
|---|---|
| `parser` | Streaming parser for `dbus-monitor` text output |
| `app_map` | Tracks discovered apps, pending notifications, and active badge counts |
| `discovery` | Queries KWin D-Bus interfaces to discover running apps and their desktop file IDs |
| `badge` | Emits Unity LauncherEntry Update signals via `gdbus` |

All modules are designed for testability — external D-Bus calls are injected as function parameters in tests.

## Known limitations

- Processes all session bus traffic (unfiltered monitor). Lightweight in practice but could theoretically be optimized with `BecomeMonitor` D-Bus API.
- Only badges apps that currently have an open window. Pinned taskbar icons without a running instance won't be badged.
- If two apps share the same last dot-segment in their desktop file ID, they would collide in the match map. Unlikely in practice.
- Firefox web app notifications all badge under the single Firefox icon.
- Notifications that expire via timeout also emit `NotificationClosed`, so badges clear when notifications auto-dismiss.

## License

MIT
