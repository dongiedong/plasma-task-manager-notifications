use log::warn;

/// Emits com.canonical.Unity.LauncherEntry.Update signals via gdbus.
pub struct BadgeEmitter;

impl BadgeEmitter {
    /// Emit a badge update for the given desktop_id with the specified count.
    pub fn emit(desktop_id: &str, count: usize) {
        Self::emit_with(desktop_id, count, crate::dbus_glue::emit_badge);
    }

    /// Testable version that accepts a function for the actual D-Bus emission.
    pub fn emit_with<F>(desktop_id: &str, count: usize, emit_fn: F)
    where
        F: FnOnce(&str, &str) -> Result<(), String>,
    {
        let visible = if count > 0 { "true" } else { "false" };
        let urgent = visible;
        let properties = format!(
            "{{'count': <int64 {count}>, 'count-visible': <{visible}>, 'urgent': <{urgent}>}}"
        );

        if let Err(e) = emit_fn(desktop_id, &properties) {
            warn!("badge error: {e}");
        }
    }

    /// Clear badges for a list of desktop_ids (used on shutdown).
    pub fn clear_all(desktop_ids: &[String]) {
        for id in desktop_ids {
            Self::emit(id, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn test_emit_with_count() {
        let captured = RefCell::new(None);

        BadgeEmitter::emit_with("application://firefox.desktop", 3, |id, props| {
            *captured.borrow_mut() = Some((id.to_string(), props.to_string()));
            Ok(())
        });

        let (id, props) = captured.borrow().clone().unwrap();
        assert_eq!(id, "application://firefox.desktop");
        assert!(props.contains("count': <int64 3>"));
        assert!(props.contains("count-visible': <true>"));
        assert!(props.contains("urgent': <true>"));
    }

    #[test]
    fn test_emit_with_zero_count() {
        let captured = RefCell::new(None);

        BadgeEmitter::emit_with("application://firefox.desktop", 0, |id, props| {
            *captured.borrow_mut() = Some((id.to_string(), props.to_string()));
            Ok(())
        });

        let (_, props) = captured.borrow().clone().unwrap();
        assert!(props.contains("count': <int64 0>"));
        assert!(props.contains("count-visible': <false>"));
        assert!(props.contains("urgent': <false>"));
    }

    #[test]
    fn test_emit_error_is_logged_not_panicked() {
        // Should not panic on error
        BadgeEmitter::emit_with("application://firefox.desktop", 1, |_, _| {
            Err("dbus error".into())
        });
    }
}
