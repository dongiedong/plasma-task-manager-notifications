use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Debug, Clone)]
pub struct FloorpResolver {
    profile: PathBuf,
}

impl FloorpResolver {
    pub fn discover() -> Option<Self> {
        let home = std::env::var_os("HOME")?;
        Self::discover_in(&PathBuf::from(home).join(".floorp"))
    }

    pub fn discover_in(floorp_dir: &Path) -> Option<Self> {
        fs::read_dir(floorp_dir)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.join("notificationstore.json").is_file() && path.join("ssb/ssb.json").is_file()
            })
            .max_by_key(|path| {
                fs::metadata(path.join("notificationstore.json"))
                    .and_then(|metadata| metadata.modified())
                    .ok()
            })
            .map(|profile| Self { profile })
    }

    pub fn from_profile(profile: impl Into<PathBuf>) -> Self {
        Self {
            profile: profile.into(),
        }
    }

    /// Resolve a Floorp web notification to its SSB launcher. Both files are
    /// read for every notification because Floorp updates them while running.
    pub fn resolve(&self, title: &str, body: &str) -> Option<String> {
        let store = read_json(&self.profile.join("notificationstore.json"))?;
        let notification_origin = find_notification_origin(&store, title, body)?;
        let ssbs = read_json(&self.profile.join("ssb/ssb.json"))?;
        resolve_origin_to_desktop_id(&notification_origin, &ssbs)
    }
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn find_notification_origin(store: &Value, title: &str, body: &str) -> Option<String> {
    store
        .as_object()?
        .iter()
        .flat_map(|(origin, notifications)| {
            notifications
                .as_object()
                .into_iter()
                .flat_map(move |entries| entries.values().map(move |entry| (origin, entry)))
        })
        .filter(|(_, entry)| {
            entry.get("title").and_then(Value::as_str) == Some(title)
                && entry.get("body").and_then(Value::as_str) == Some(body)
        })
        .max_by_key(|(_, entry)| entry.get("timestamp").and_then(Value::as_u64).unwrap_or(0))
        .map(|(origin, entry)| {
            entry
                .get("serviceWorkerRegistrationScope")
                .and_then(Value::as_str)
                .unwrap_or(origin)
                .to_string()
        })
}

fn url_origin(value: &str) -> Option<(String, String, Option<u16>)> {
    let url = Url::parse(value).ok()?;
    Some((
        url.scheme().to_ascii_lowercase(),
        url.host_str()?.to_ascii_lowercase(),
        url.port_or_known_default(),
    ))
}

fn resolve_origin_to_desktop_id(origin: &str, ssbs: &Value) -> Option<String> {
    let wanted = url_origin(origin)?;
    ssbs.as_object()?.values().find_map(|ssb| {
        let scope = ssb.get("scope").and_then(Value::as_str)?;
        if url_origin(scope)? != wanted {
            return None;
        }
        let uuid = ssb.get("id").and_then(Value::as_str)?;
        Some(format!(
            "application://org.mozilla.firefox.webapp-{uuid}.desktop"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn messenger_deep_scope_matches_by_origin() {
        let ssbs = json!({"messenger": {
            "scope": "https://www.messenger.com/t/938421545493992/",
            "id": "{478c45c7-e9e6-472a-a6cf-cfb82ed21216}"
        }});
        assert_eq!(
            resolve_origin_to_desktop_id("https://www.messenger.com/", &ssbs),
            Some("application://org.mozilla.firefox.webapp-{478c45c7-e9e6-472a-a6cf-cfb82ed21216}.desktop".into())
        );
    }

    #[test]
    fn origin_includes_scheme_and_effective_port() {
        let ssbs = json!({"app": {"scope": "https://example.com/deep", "id": "{one}"}});
        assert!(resolve_origin_to_desktop_id("http://example.com", &ssbs).is_none());
        assert!(resolve_origin_to_desktop_id("https://example.com:444", &ssbs).is_none());
        assert!(resolve_origin_to_desktop_id("https://example.com:443", &ssbs).is_some());
    }

    #[test]
    fn newest_exact_title_and_body_selects_origin() {
        let store = json!({
            "https://old.example": {"1": {"title": "Message", "body": "Hello", "timestamp": 1}},
            "https://new.example": {"2": {"title": "Message", "body": "Hello", "timestamp": 2,
                "serviceWorkerRegistrationScope": "https://new.example/sw/"}},
            "https://wrong.example": {"3": {"title": "Message", "body": "Other", "timestamp": 3}}
        });
        assert_eq!(
            find_notification_origin(&store, "Message", "Hello").as_deref(),
            Some("https://new.example/sw/")
        );
    }
}
