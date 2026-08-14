//! Integration test: simulate a full notification lifecycle using AppMap + DbusParser together,
//! feeding realistic dbus-monitor output through the parser and processing results with AppMap.

use plasma_task_manager_notifications::{AppMap, DbusMessage, DbusParser};
use std::collections::HashMap;

fn setup_app_map() -> AppMap {
    let mut map = AppMap::new();
    let mut patterns = HashMap::new();
    patterns.insert(
        "firefox".into(),
        "application://org.mozilla.firefox.desktop".into(),
    );
    patterns.insert(
        "org.mozilla.firefox".into(),
        "application://org.mozilla.firefox.desktop".into(),
    );
    patterns.insert(
        "slack".into(),
        "application://com.slack.Slack.desktop".into(),
    );
    patterns.insert(
        "com.slack.slack".into(),
        "application://com.slack.Slack.desktop".into(),
    );
    map.update_patterns(patterns);
    map
}

fn process_stream(input: &str, map: &mut AppMap) -> Vec<(String, usize)> {
    let mut parser = DbusParser::new();
    let mut badge_updates = Vec::new();

    let mut handle = |msg: DbusMessage, map: &mut AppMap| match msg {
        DbusMessage::Notify {
            serial,
            app_name,
            replaces_id,
            desktop_entry_hint,
            ..
        } => {
            let hint_ref = desktop_entry_hint.as_deref().unwrap_or("");
            if let Some(desktop_id) = map.match_app(&[&app_name, hint_ref]) {
                map.record_notify(serial, replaces_id, &desktop_id);
            }
        }
        DbusMessage::NotifyReply {
            reply_serial,
            notification_id,
        } => {
            if let Some((desktop_id, count)) = map.resolve_reply(reply_serial, notification_id) {
                badge_updates.push((desktop_id, count));
            }
        }
        DbusMessage::NotificationClosed { notification_id } => {
            if let Some((desktop_id, count)) = map.notification_closed(notification_id) {
                badge_updates.push((desktop_id, count));
            }
        }
    };

    for line in input.lines() {
        if let Some(msg) = parser.feed_line(line) {
            handle(msg, map);
        }
    }
    if let Some(msg) = parser.flush() {
        handle(msg, map);
    }

    badge_updates
}

/// Full lifecycle: Firefox notification arrives, badge set to 1, then dismissed, badge set to 0.
#[test]
fn test_full_lifecycle_notify_and_dismiss() {
    let stream = r#"method call time=1700000000.000 sender=:1.100 -> destination=:1.5 serial=500 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify
   string "firefox"
   uint32 0
   string "firefox"
   string "New tab notification"
   string "You have updates"
   array [
   ]
   array [
   ]
   int32 -1
method return time=1700000000.100 sender=:1.5 -> destination=:1.100 serial=600 reply_serial=500
   uint32 42
signal time=1700000001.000 sender=:1.5 -> destination=(null destination) serial=700 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=NotificationClosed
   uint32 42
   uint32 2
"#;

    let mut map = setup_app_map();
    let updates = process_stream(stream, &mut map);

    assert_eq!(updates.len(), 2);
    assert_eq!(
        updates[0],
        ("application://org.mozilla.firefox.desktop".into(), 1)
    );
    assert_eq!(
        updates[1],
        ("application://org.mozilla.firefox.desktop".into(), 0)
    );
}

/// Multiple notifications from different apps accumulate independently.
#[test]
fn test_multiple_apps_independent_counts() {
    let stream = r#"method call time=1700000000.000 sender=:1.100 -> destination=:1.5 serial=500 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify
   string "firefox"
   uint32 0
   string ""
   string "Firefox notification"
   string ""
method return time=1700000000.100 sender=:1.5 -> destination=:1.100 serial=600 reply_serial=500
   uint32 42
method call time=1700000000.200 sender=:1.200 -> destination=:1.5 serial=501 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify
   string "Slack"
   uint32 0
   string ""
   string "Slack message"
   string ""
method return time=1700000000.300 sender=:1.5 -> destination=:1.200 serial=601 reply_serial=501
   uint32 43
method call time=1700000000.400 sender=:1.100 -> destination=:1.5 serial=502 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify
   string "firefox"
   uint32 0
   string ""
   string "Another Firefox notification"
   string ""
method return time=1700000000.500 sender=:1.5 -> destination=:1.100 serial=602 reply_serial=502
   uint32 44
"#;

    let mut map = setup_app_map();
    let updates = process_stream(stream, &mut map);

    assert_eq!(updates.len(), 3);
    // Firefox: 1
    assert_eq!(
        updates[0],
        ("application://org.mozilla.firefox.desktop".into(), 1)
    );
    // Slack: 1
    assert_eq!(
        updates[1],
        ("application://com.slack.Slack.desktop".into(), 1)
    );
    // Firefox: 2
    assert_eq!(
        updates[2],
        ("application://org.mozilla.firefox.desktop".into(), 2)
    );

    assert_eq!(map.count("application://org.mozilla.firefox.desktop"), 2);
    assert_eq!(map.count("application://com.slack.Slack.desktop"), 1);
}

/// Flatpak app sends notification via portal with desktop-entry hint.
#[test]
fn test_flatpak_desktop_entry_hint() {
    let stream = r#"method call time=1700000000.000 sender=:1.100 -> destination=:1.5 serial=500 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify
   string "System Notifications"
   uint32 0
   string ""
   string "New message from channel"
   string "Message body"
   array [
   ]
   dict entry(
      string "desktop-entry"
      variant string "com.slack.Slack"
   )
   int32 -1
method return time=1700000000.100 sender=:1.5 -> destination=:1.100 serial=600 reply_serial=500
   uint32 50
"#;

    let mut map = setup_app_map();
    let updates = process_stream(stream, &mut map);

    assert_eq!(updates.len(), 1);
    assert_eq!(
        updates[0],
        ("application://com.slack.Slack.desktop".into(), 1)
    );
}

/// Replacement notification: new notification replaces an existing one, count stays at 1.
#[test]
fn test_replacement_notification() {
    let stream = r#"method call time=1700000000.000 sender=:1.100 -> destination=:1.5 serial=500 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify
   string "firefox"
   uint32 0
   string ""
   string "First"
   string ""
method return time=1700000000.100 sender=:1.5 -> destination=:1.100 serial=600 reply_serial=500
   uint32 42
method call time=1700000000.200 sender=:1.100 -> destination=:1.5 serial=501 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify
   string "firefox"
   uint32 42
   string ""
   string "Replaced"
   string ""
method return time=1700000000.300 sender=:1.5 -> destination=:1.100 serial=601 reply_serial=501
   uint32 43
"#;

    let mut map = setup_app_map();
    let updates = process_stream(stream, &mut map);

    assert_eq!(updates.len(), 2);
    // First notification: count=1
    assert_eq!(updates[0].1, 1);
    // Replacement: old removed, new added, still count=1
    assert_eq!(updates[1].1, 1);
    assert_eq!(map.count("application://org.mozilla.firefox.desktop"), 1);
}

/// Unknown app notifications are ignored entirely.
#[test]
fn test_unknown_app_ignored() {
    let stream = r#"method call time=1700000000.000 sender=:1.100 -> destination=:1.5 serial=500 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify
   string "unknown-app"
   uint32 0
   string ""
   string "Some notification"
   string ""
method return time=1700000000.100 sender=:1.5 -> destination=:1.100 serial=600 reply_serial=500
   uint32 99
"#;

    let mut map = setup_app_map();
    let updates = process_stream(stream, &mut map);

    assert_eq!(updates.len(), 0);
}

/// Interleaved traffic: unrelated D-Bus messages mixed with notifications.
#[test]
fn test_interleaved_unrelated_traffic() {
    let stream = r#"signal time=1700000000.000 sender=:1.1 -> destination=(null destination) serial=100 path=/org/freedesktop/DBus; interface=org.freedesktop.DBus; member=NameOwnerChanged
   string ":1.200"
   string ""
   string ":1.200"
method call time=1700000000.100 sender=:1.50 -> destination=:1.5 serial=200 path=/org/kde/StatusNotifierWatcher; interface=org.kde.StatusNotifierWatcher; member=RegisterStatusNotifierItem
   string "/StatusNotifierItem"
method call time=1700000000.200 sender=:1.100 -> destination=:1.5 serial=500 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify
   string "firefox"
   uint32 0
   string ""
   string "Real notification"
   string ""
method return time=1700000000.250 sender=:1.5 -> destination=:1.50 serial=201 reply_serial=200
   string "ok"
method return time=1700000000.300 sender=:1.5 -> destination=:1.100 serial=600 reply_serial=500
   uint32 42
signal time=1700000000.400 sender=:1.1 -> destination=(null destination) serial=101 path=/org/freedesktop/DBus; interface=org.freedesktop.DBus; member=NameOwnerChanged
   string ":1.300"
   string ":1.300"
   string ""
"#;

    let mut map = setup_app_map();
    let updates = process_stream(stream, &mut map);

    assert_eq!(updates.len(), 1);
    assert_eq!(
        updates[0],
        ("application://org.mozilla.firefox.desktop".into(), 1)
    );
}

/// Google Chrome: app_name "Google Chrome" must match pattern "google-chrome" (space vs hyphen).
#[test]
fn test_google_chrome_space_vs_hyphen() {
    let mut map = AppMap::new();
    let mut patterns = HashMap::new();
    patterns.insert(
        "google-chrome".into(),
        "application://google-chrome.desktop".into(),
    );
    map.update_patterns(patterns);

    let stream = r#"method call time=1700000000.000 sender=:1.100 -> destination=:1.5 serial=500 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify
   string "Google Chrome"
   uint32 0
   string ""
   string "New message"
   string ""
method return time=1700000000.100 sender=:1.5 -> destination=:1.100 serial=600 reply_serial=500
   uint32 55
signal time=1700000001.000 sender=:1.5 -> destination=(null destination) serial=700 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=NotificationClosed
   uint32 55
   uint32 2
"#;

    let updates = process_stream(stream, &mut map);

    assert_eq!(updates.len(), 2);
    assert_eq!(
        updates[0],
        ("application://google-chrome.desktop".into(), 1)
    );
    assert_eq!(
        updates[1],
        ("application://google-chrome.desktop".into(), 0)
    );
}
