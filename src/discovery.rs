use regex::Regex;
use std::collections::HashMap;
use std::env;
use std::path::Path;

/// Query KWin for open windows and return a pattern map:
/// pattern (lowercase) -> "application://<desktop_file>.desktop"
///
/// Normally KWin gives us desktopFile directly.
///
/// Some applications (notably Floorp's normal browser window) expose an empty
/// desktopFile but do expose a resourceClass matching StartupWMClass.
/// In that case we fall back to resourceClass if a matching .desktop launcher
/// actually exists.
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
    let desktop_re = Regex::new(r"'desktopFile':\s*<'([^']*)'>").unwrap();
    let resource_class_re = Regex::new(r"'resourceClass':\s*<'([^']*)'>").unwrap();

    let mut map = HashMap::new();

    for caps in uuid_re.captures_iter(&output) {
        let uid = &caps[1];

        if let Some(entries) =
            extract_desktop_entries(uid, &info_fn, &desktop_re, &resource_class_re)
        {
            map.extend(entries);
        }
    }

    map
}

/// Return true if a desktop launcher exists in the usual user/system paths.
fn desktop_file_exists(desktop_file: &str) -> bool {
    let filename = format!("{desktop_file}.desktop");

    if let Ok(home) = env::var("HOME") {
        if Path::new(&home)
            .join(".local/share/applications")
            .join(&filename)
            .exists()
        {
            return true;
        }
    }

    Path::new("/usr/share/applications")
        .join(&filename)
        .exists()
}

fn extract_desktop_entries<G>(
    uid: &str,
    info_fn: &G,
    desktop_re: &Regex,
    resource_class_re: &Regex,
) -> Option<Vec<(String, String)>>
where
    G: Fn(&str) -> Option<String>,
{
    let info_output = info_fn(uid)?;

    // Preferred source: KWin desktopFile.
    let desktop_file = desktop_re
        .captures(&info_output)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str())
        .filter(|s| !s.is_empty());

    // Fallback: resourceClass, but only if a real matching launcher exists.
    let desktop_file = match desktop_file {
        Some(value) => value.to_string(),
        None => {
            let resource_class = resource_class_re
                .captures(&info_output)
                .and_then(|caps| caps.get(1))
                .map(|m| m.as_str())
                .filter(|s| !s.is_empty())?;

            if !desktop_file_exists(resource_class) {
                return None;
            }

            resource_class.to_string()
        }
    };

    let key = desktop_file
        .rsplit('.')
        .next()
        .unwrap_or(&desktop_file)
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
    fn test_discover_apps_window_info_fails() {
        let match_output =
            r#"([('0_{aaaaaaaa-1234-5678-9abc-def012345678}', 'Window', 'icon', 100, 0.8, {})],)"#;

        let map = discover_apps_with(|| Some(match_output.to_string()), |_| None);
        assert!(map.is_empty());
    }
}
