//! Thin wrappers around `gdbus` / `Command` calls.
//!
//! These functions are excluded from code coverage because they require
//! a live D-Bus session and KDE Plasma environment.

use std::process::Command;

/// Emit a Unity LauncherEntry Update signal via gdbus.
pub fn emit_badge(desktop_id: &str, properties: &str) -> Result<(), String> {
    let output = Command::new("gdbus")
        .args([
            "emit",
            "--session",
            "--object-path",
            "/",
            "--signal",
            "com.canonical.Unity.LauncherEntry.Update",
            desktop_id,
            properties,
        ])
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Query KWin WindowsRunner for open windows.
pub fn kwin_match() -> Option<String> {
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.kde.KWin",
            "--object-path",
            "/WindowsRunner",
            "--method",
            "org.kde.krunner1.Match",
            "",
        ])
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}

/// Get window info for a specific KWin UUID.
pub fn kwin_window_info(uuid: &str) -> Option<String> {
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.kde.KWin",
            "--object-path",
            "/KWin",
            "--method",
            "org.kde.KWin.getWindowInfo",
            &format!("{{{uuid}}}"),
        ])
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}
