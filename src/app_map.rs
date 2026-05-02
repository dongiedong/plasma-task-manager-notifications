use std::collections::{HashMap, HashSet};

/// Maps notification app names / desktop-entry hints to Unity desktop IDs,
/// and tracks active notification IDs per app.
#[derive(Debug, Default)]
pub struct AppMap {
    /// pattern (lowercase) -> "application://<id>.desktop"
    patterns: HashMap<String, String>,
    /// desktop_id -> set of active notification IDs
    active: HashMap<String, HashSet<u32>>,
    /// notification_id -> desktop_id
    notif_to_app: HashMap<u32, String>,
    /// pending Notify calls: serial -> desktop_id
    pending: HashMap<u64, String>,
}

/// Normalize separators (hyphens, underscores, spaces) to a single form
/// so that "google-chrome", "google chrome", and "google_chrome" all match.
fn normalize_separators(s: &str) -> String {
    s.chars()
        .map(|c| if matches!(c, '-' | '_' | ' ') { '-' } else { c })
        .collect()
}

impl AppMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the pattern map with a new set of discovered apps.
    /// Returns true if the map changed.
    pub fn update_patterns(&mut self, new_map: HashMap<String, String>) -> bool {
        if new_map == self.patterns {
            return false;
        }
        self.patterns = new_map;
        true
    }

    /// Return the desktop_id if any candidate matches a known app pattern.
    pub fn match_app(&self, candidates: &[&str]) -> Option<String> {
        candidates
            .iter()
            .filter(|c| !c.is_empty())
            .find_map(|candidate| self.find_pattern(&candidate.to_lowercase()))
    }

    fn find_pattern(&self, lower: &str) -> Option<String> {
        let normalized = normalize_separators(lower);
        self.patterns
            .iter()
            .find(|(pattern, _)| normalized.contains(&normalize_separators(pattern)))
            .map(|(_, desktop_id)| desktop_id.clone())
    }

    /// Record a pending Notify call (serial -> desktop_id) after matching.
    /// Handles replaces_id: if the notification replaces an existing one,
    /// the old one is removed from tracking first.
    pub fn record_notify(&mut self, serial: u64, replaces_id: u32, desktop_id: &str) {
        if replaces_id > 0 {
            if let Some(old_desktop) = self.notif_to_app.remove(&replaces_id) {
                if let Some(set) = self.active.get_mut(&old_desktop) {
                    set.remove(&replaces_id);
                }
            }
        }
        self.pending.insert(serial, desktop_id.to_string());
    }

    /// Resolve a method_return for a Notify call. Returns (desktop_id, new_count) if matched.
    pub fn resolve_reply(&mut self, reply_serial: u64, nid: u32) -> Option<(String, usize)> {
        let desktop_id = self.pending.remove(&reply_serial)?;
        self.notif_to_app.insert(nid, desktop_id.clone());
        let set = self.active.entry(desktop_id.clone()).or_default();
        set.insert(nid);
        Some((desktop_id, set.len()))
    }

    /// Record that a notification was closed. Returns (desktop_id, new_count) if tracked.
    pub fn notification_closed(&mut self, nid: u32) -> Option<(String, usize)> {
        let desktop_id = self.notif_to_app.remove(&nid)?;
        let set = self.active.entry(desktop_id.clone()).or_default();
        set.remove(&nid);
        Some((desktop_id, set.len()))
    }

    /// Return the count of active notifications for a desktop_id.
    pub fn count(&self, desktop_id: &str) -> usize {
        self.active.get(desktop_id).map_or(0, |s| s.len())
    }

    /// Return all unique desktop_ids that have been registered (from patterns).
    pub fn all_desktop_ids(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut ids = Vec::new();
        for id in self.patterns.values() {
            if seen.insert(id.clone()) {
                ids.push(id.clone());
            }
        }
        ids
    }

    /// Clear all active notifications for every app, returning the desktop_ids that were cleared.
    pub fn clear_all(&mut self) -> Vec<String> {
        let ids = self.all_desktop_ids();
        for id in &ids {
            if let Some(set) = self.active.get_mut(id) {
                for nid in set.drain() {
                    self.notif_to_app.remove(&nid);
                }
            }
        }
        ids
    }

    /// Return the current pattern map for logging.
    pub fn patterns(&self) -> &HashMap<String, String> {
        &self.patterns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_map() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert(
            "firefox".into(),
            "application://org.mozilla.firefox.desktop".into(),
        );
        m.insert(
            "org.mozilla.firefox".into(),
            "application://org.mozilla.firefox.desktop".into(),
        );
        m.insert(
            "slack".into(),
            "application://com.slack.Slack.desktop".into(),
        );
        m.insert(
            "com.slack.slack".into(),
            "application://com.slack.Slack.desktop".into(),
        );
        m
    }

    #[test]
    fn test_match_app_by_short_name() {
        let mut am = AppMap::new();
        am.update_patterns(sample_map());

        let ff = Some("application://org.mozilla.firefox.desktop".to_string());
        let sl = Some("application://com.slack.Slack.desktop".to_string());

        assert_eq!(am.match_app(&["firefox"]), ff);
        assert_eq!(am.match_app(&["FIREFOX"]), ff);
        assert_eq!(am.match_app(&["FireFox"]), ff);
        assert_eq!(am.match_app(&["Slack"]), sl);
    }

    #[test]
    fn test_match_app_by_desktop_entry() {
        let mut am = AppMap::new();
        am.update_patterns(sample_map());

        assert_eq!(
            am.match_app(&["", "com.slack.Slack"]),
            Some("application://com.slack.Slack.desktop".into())
        );
    }

    #[test]
    fn test_match_app_no_match() {
        let mut am = AppMap::new();
        am.update_patterns(sample_map());

        assert_eq!(am.match_app(&["unknown-app"]), None);
        assert_eq!(am.match_app(&[""]), None);
        assert_eq!(am.match_app(&[]), None);
    }

    #[test]
    fn test_notify_reply_closed_lifecycle() {
        let mut am = AppMap::new();
        am.update_patterns(sample_map());

        let desktop = "application://org.mozilla.firefox.desktop";

        // Notify call with serial=100, replaces_id=0
        am.record_notify(100, 0, desktop);

        // Reply with notification ID 42
        let result = am.resolve_reply(100, 42);
        assert_eq!(result, Some((desktop.into(), 1)));
        assert_eq!(am.count(desktop), 1);

        // Second notification
        am.record_notify(101, 0, desktop);
        let result = am.resolve_reply(101, 43);
        assert_eq!(result, Some((desktop.into(), 2)));
        assert_eq!(am.count(desktop), 2);

        // Close first
        let result = am.notification_closed(42);
        assert_eq!(result, Some((desktop.into(), 1)));
        assert_eq!(am.count(desktop), 1);

        // Close second
        let result = am.notification_closed(43);
        assert_eq!(result, Some((desktop.into(), 0)));
        assert_eq!(am.count(desktop), 0);
    }

    #[test]
    fn test_replacement_notification() {
        let mut am = AppMap::new();
        am.update_patterns(sample_map());

        let desktop = "application://org.mozilla.firefox.desktop";

        // First notification
        am.record_notify(100, 0, desktop);
        am.resolve_reply(100, 42);
        assert_eq!(am.count(desktop), 1);

        // Replacement notification (replaces_id=42)
        am.record_notify(101, 42, desktop);
        assert_eq!(am.count(desktop), 0); // old one removed during record
        am.resolve_reply(101, 43);
        assert_eq!(am.count(desktop), 1); // new one added
    }

    #[test]
    fn test_close_unknown_notification() {
        let mut am = AppMap::new();
        assert_eq!(am.notification_closed(999), None);
    }

    #[test]
    fn test_resolve_unknown_reply() {
        let mut am = AppMap::new();
        assert_eq!(am.resolve_reply(999, 42), None);
    }

    #[test]
    fn test_update_patterns_returns_changed() {
        let mut am = AppMap::new();
        assert!(am.update_patterns(sample_map()));
        assert!(!am.update_patterns(sample_map()));

        let mut different = HashMap::new();
        different.insert("new".into(), "application://new.desktop".into());
        assert!(am.update_patterns(different));
    }

    #[test]
    fn test_clear_all() {
        let mut am = AppMap::new();
        am.update_patterns(sample_map());

        let desktop = "application://org.mozilla.firefox.desktop";
        am.record_notify(100, 0, desktop);
        am.resolve_reply(100, 42);
        am.record_notify(101, 0, desktop);
        am.resolve_reply(101, 43);

        let cleared = am.clear_all();
        assert!(cleared.contains(&desktop.to_string()));
        assert_eq!(am.count(desktop), 0);
    }

    #[test]
    fn test_all_desktop_ids_deduplicates() {
        let mut am = AppMap::new();
        am.update_patterns(sample_map());
        let ids = am.all_desktop_ids();
        // sample_map has 4 entries mapping to 2 unique desktop IDs
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"application://org.mozilla.firefox.desktop".to_string()));
        assert!(ids.contains(&"application://com.slack.Slack.desktop".to_string()));
    }

    #[test]
    fn test_match_app_normalizes_separators() {
        let mut am = AppMap::new();
        let mut m = HashMap::new();
        // KWin reports "google-chrome" as the desktopFile
        m.insert(
            "google-chrome".into(),
            "application://google-chrome.desktop".into(),
        );
        am.update_patterns(m);

        // Chrome sends app_name "Google Chrome" (space, not hyphen)
        assert_eq!(
            am.match_app(&["Google Chrome"]),
            Some("application://google-chrome.desktop".into())
        );
        // Also match with underscore or exact hyphen
        assert_eq!(
            am.match_app(&["google_chrome"]),
            Some("application://google-chrome.desktop".into())
        );
        assert_eq!(
            am.match_app(&["google-chrome"]),
            Some("application://google-chrome.desktop".into())
        );
    }

    #[test]
    fn test_patterns_accessor() {
        let mut am = AppMap::new();
        assert!(am.patterns().is_empty());
        am.update_patterns(sample_map());
        assert_eq!(am.patterns().len(), 4);
    }
}
