use regex::Regex;
use std::collections::HashMap;

/// Query KWin for open windows and return a pattern map:
/// pattern (lowercase) -> "application://<desktop_file>.desktop"
///
/// Two keys per app:
/// - The last dot-segment (e.g. "firefox") -- matches native app_name
/// - The full desktop file ID (e.g. "org.mozilla.firefox") -- matches Flatpak desktop-entry hints
pub fn discover_apps() -> HashMap<String, String> {
    discover_apps_with(
        crate::dbus_glue::kwin_match,
        crate::dbus_glue::kwin_window_info,
    )
}

/// Testable version that accepts function pointers for the D-Bus calls.
pub fn discover_apps_with<F, G>(match_fn: F, info_fn: G) -> HashMap<String, String>
where
    F: Fn() -> Option<String>,
    G: Fn(&str) -> Option<String>,
{
    let output = match match_fn() {
        Some(s) => s,
        None => return HashMap::new(),
    };

    let uuid_re = Regex::new(r"\{([0-9a-f-]{36})\}").unwrap();
    let desktop_re = Regex::new(r"'desktopFile':\s*<'([^']+)'>").unwrap();

    let mut map = HashMap::new();

    for caps in uuid_re.captures_iter(&output) {
        let uid = &caps[1];
        if let Some(entries) = extract_desktop_entries(uid, &info_fn, &desktop_re) {
            map.extend(entries);
        }
    }

    map
}

fn extract_desktop_entries<G>(
    uid: &str,
    info_fn: &G,
    desktop_re: &Regex,
) -> Option<Vec<(String, String)>>
where
    G: Fn(&str) -> Option<String>,
{
    let info_output = info_fn(uid)?;
    let dcaps = desktop_re.captures(&info_output)?;
    let desktop_file = &dcaps[1];
    if desktop_file.is_empty() {
        return None;
    }
    let key = desktop_file
        .rsplit('.')
        .next()
        .unwrap_or(desktop_file)
        .to_lowercase();
    let desktop_id = format!("application://{desktop_file}.desktop");
    Some(vec![
        (key, desktop_id.clone()),
        (desktop_file.to_lowercase(), desktop_id),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_apps_parses_kwin_output() {
        let match_output = r#"([('0_{aabbccdd-1234-5678-9abc-def012345678}', 'Some Window — Firefox', 'firefox', 100, 0.8, {'subtext': <'Activate running window on Desktop 1'>}), ('0_{11223344-5566-7788-99aa-bbccddeeff00}', 'Slack', 'com.slack.Slack', 100, 0.8, {'subtext': <'Activate running window on Desktop 1'>})],)"#;

        let info_firefox =
            r#"({'desktopFile': <'org.mozilla.firefox'>, 'caption': <'Some Window — Firefox'>},)"#;
        let info_slack = r#"({'desktopFile': <'com.slack.Slack'>, 'caption': <'Slack'>},)"#;

        let match_fn = || Some(match_output.to_string());
        let info_fn = |uuid: &str| match uuid {
            "aabbccdd-1234-5678-9abc-def012345678" => Some(info_firefox.to_string()),
            "11223344-5566-7788-99aa-bbccddeeff00" => Some(info_slack.to_string()),
            _ => None,
        };

        let map = discover_apps_with(match_fn, info_fn);

        assert_eq!(
            map.get("firefox"),
            Some(&"application://org.mozilla.firefox.desktop".to_string())
        );
        assert_eq!(
            map.get("org.mozilla.firefox"),
            Some(&"application://org.mozilla.firefox.desktop".to_string())
        );
        assert_eq!(
            map.get("slack"),
            Some(&"application://com.slack.Slack.desktop".to_string())
        );
        assert_eq!(
            map.get("com.slack.slack"),
            Some(&"application://com.slack.Slack.desktop".to_string())
        );
    }

    #[test]
    fn test_discover_apps_empty_match() {
        let map = discover_apps_with(|| None, |_| None);
        assert!(map.is_empty());
    }

    #[test]
    fn test_discover_apps_no_uuids() {
        let map = discover_apps_with(|| Some("([],)".to_string()), |_| None);
        assert!(map.is_empty());
    }

    #[test]
    fn test_discover_apps_empty_desktop_file() {
        let match_output =
            r#"([('0_{aabbccdd-1234-5678-9abc-def012345678}', 'Window', 'icon', 100, 0.8, {})],)"#;
        let info_output = r#"({'desktopFile': <''>, 'caption': <'Window'>},)"#;

        let map = discover_apps_with(
            || Some(match_output.to_string()),
            |_| Some(info_output.to_string()),
        );
        assert!(map.is_empty());
    }

    #[test]
    fn test_discover_apps_window_info_fails() {
        let match_output =
            r#"([('0_{aabbccdd-1234-5678-9abc-def012345678}', 'Window', 'icon', 100, 0.8, {})],)"#;

        let map = discover_apps_with(|| Some(match_output.to_string()), |_| None);
        assert!(map.is_empty());
    }

    #[test]
    fn test_discover_apps_deduplicates_same_app() {
        // Two windows from the same app
        let match_output = r#"([('0_{aaaaaaaa-1234-5678-9abc-def012345678}', 'Tab 1 — Firefox', 'firefox', 100, 0.8, {}), ('0_{bbbbbbbb-1234-5678-9abc-def012345678}', 'Tab 2 — Firefox', 'firefox', 100, 0.8, {})],)"#;
        let info_output = r#"({'desktopFile': <'org.mozilla.firefox'>, 'caption': <'Firefox'>},)"#;

        let map = discover_apps_with(
            || Some(match_output.to_string()),
            |_| Some(info_output.to_string()),
        );
        // Should have exactly 2 entries (short name + full name), not duplicated
        assert_eq!(map.len(), 2);
        assert!(map.contains_key("firefox"));
        assert!(map.contains_key("org.mozilla.firefox"));
    }
}
