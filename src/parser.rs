use regex::Regex;

/// Parsed message types from the dbus-monitor stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbusMessage {
    /// A Notify method call was detected.
    /// Fields: serial, app_name, replaces_id, desktop_entry_hint
    Notify {
        serial: u64,
        app_name: String,
        replaces_id: u32,
        desktop_entry_hint: Option<String>,
        summary: String,
        body: String,
    },
    /// A method return matching a pending Notify serial.
    /// Fields: reply_serial, notification_id
    NotifyReply {
        reply_serial: u64,
        notification_id: u32,
    },
    /// A NotificationClosed signal.
    NotificationClosed { notification_id: u32 },
}

/// Streaming parser for dbus-monitor text output.
///
/// Feed it lines one at a time via `feed_line`. When a complete message
/// is recognized, it is returned from `feed_line` or `flush`.
pub struct DbusParser {
    header_re: Regex,
    reply_serial_re: Regex,
    member_re: Regex,
    string_re: Regex,
    uint32_re: Regex,
    variant_string_re: Regex,

    // Current message block state
    msg_type: Option<MsgType>,
    serial: u64,
    reply_serial: Option<u64>,
    strings: Vec<String>,
    uint32s: Vec<u32>,
    last_dict_key: Option<String>,
    desktop_entry_hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MsgType {
    Notify,
    CloseCall,
    ClosedSignal,
    Reply,
}

fn classify_header(kind: &str, member: &str, has_reply_serial: bool) -> Option<MsgType> {
    match (kind, member) {
        ("method call", "Notify") => Some(MsgType::Notify),
        ("method call", "CloseNotification") => Some(MsgType::CloseCall),
        ("signal", "NotificationClosed") => Some(MsgType::ClosedSignal),
        ("method return", _) if has_reply_serial => Some(MsgType::Reply),
        _ => None,
    }
}

impl DbusParser {
    pub fn new() -> Self {
        Self {
            header_re: Regex::new(r"^(method call|method return|signal)\s+.*?serial=(\d+)")
                .unwrap(),
            reply_serial_re: Regex::new(r"reply_serial=(\d+)").unwrap(),
            member_re: Regex::new(r"member=(\w+)").unwrap(),
            string_re: Regex::new(r#"^\s+string\s+"(.*)""#).unwrap(),
            uint32_re: Regex::new(r"^\s+uint32\s+(\d+)").unwrap(),
            variant_string_re: Regex::new(r#"^\s+variant\s+string\s+"(.*)""#).unwrap(),
            msg_type: None,
            serial: 0,
            reply_serial: None,
            strings: Vec::new(),
            uint32s: Vec::new(),
            last_dict_key: None,
            desktop_entry_hint: None,
        }
    }

    /// Feed a single line from dbus-monitor output. Returns a parsed message
    /// if the previous message block is now complete (triggered by seeing
    /// the next header line).
    pub fn feed_line(&mut self, line: &str) -> Option<DbusMessage> {
        let line = line.trim_end_matches('\n');

        if let Some(caps) = self.header_re.captures(line) {
            return self.handle_header(line, caps);
        }

        self.parse_body_line(line);
        None
    }

    fn handle_header(&mut self, line: &str, caps: regex::Captures) -> Option<DbusMessage> {
        let result = self.finalize();

        let kind = caps.get(1).unwrap().as_str();
        self.serial = caps.get(2).unwrap().as_str().parse().unwrap_or(0);
        self.strings.clear();
        self.uint32s.clear();
        self.last_dict_key = None;
        self.desktop_entry_hint = None;

        self.reply_serial = self
            .reply_serial_re
            .captures(line)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse().ok());

        let member = self
            .member_re
            .captures(line)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();

        self.msg_type = classify_header(kind, &member, self.reply_serial.is_some());

        result
    }

    fn parse_body_line(&mut self, line: &str) {
        if let Some(caps) = self.string_re.captures(line) {
            self.collect_string(&caps);
        } else if let Some(caps) = self.variant_string_re.captures(line) {
            self.try_capture_desktop_entry(&caps);
        } else if let Some(caps) = self.uint32_re.captures(line) {
            self.collect_uint32(&caps);
        }
    }

    fn collect_string(&mut self, caps: &regex::Captures) {
        let val = caps.get(1).unwrap().as_str().to_string();
        self.strings.push(val.clone());
        if self.msg_type == Some(MsgType::Notify) {
            self.last_dict_key = Some(val);
        }
    }

    fn collect_uint32(&mut self, caps: &regex::Captures) {
        if let Ok(v) = caps.get(1).unwrap().as_str().parse() {
            self.uint32s.push(v);
        }
    }

    fn try_capture_desktop_entry(&mut self, caps: &regex::Captures) {
        if self.msg_type == Some(MsgType::Notify)
            && self.last_dict_key.as_deref() == Some("desktop-entry")
        {
            self.desktop_entry_hint = Some(caps.get(1).unwrap().as_str().to_string());
            self.last_dict_key = None;
        }
    }

    /// Flush the current message block (e.g. at end of stream).
    pub fn flush(&mut self) -> Option<DbusMessage> {
        self.finalize()
    }

    fn finalize(&mut self) -> Option<DbusMessage> {
        let result = match self.msg_type {
            Some(MsgType::Notify) if !self.strings.is_empty() => {
                let app_name = self.strings[0].clone();
                let replaces_id = self.uint32s.first().copied().unwrap_or(0);
                let summary = self.strings.get(2).cloned().unwrap_or_default();
                let body = self.strings.get(3).cloned().unwrap_or_default();
                Some(DbusMessage::Notify {
                    serial: self.serial,
                    app_name,
                    replaces_id,
                    desktop_entry_hint: self.desktop_entry_hint.clone(),
                    summary,
                    body,
                })
            }
            Some(MsgType::Reply) => {
                let reply_serial = self.reply_serial?;
                let notification_id = *self.uint32s.first()?;
                Some(DbusMessage::NotifyReply {
                    reply_serial,
                    notification_id,
                })
            }
            Some(MsgType::ClosedSignal) => {
                let notification_id = *self.uint32s.first()?;
                Some(DbusMessage::NotificationClosed { notification_id })
            }
            Some(MsgType::Notify) => None,    // No strings collected
            Some(MsgType::CloseCall) => None, // Signal will follow
            None => None,
        };

        self.msg_type = None;
        result
    }
}

impl Default for DbusParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_block(lines: &str) -> Vec<DbusMessage> {
        let mut parser = DbusParser::new();
        let mut results = Vec::new();
        for line in lines.lines() {
            if let Some(msg) = parser.feed_line(line) {
                results.push(msg);
            }
        }
        if let Some(msg) = parser.flush() {
            results.push(msg);
        }
        results
    }

    fn expect_single_notify(input: &str) -> DbusMessage {
        let msgs = parse_block(input);
        assert_eq!(msgs.len(), 1);
        assert!(matches!(msgs[0], DbusMessage::Notify { .. }));
        msgs.into_iter().next().unwrap()
    }

    #[test]
    fn test_parse_notify_calls() {
        // Case 1: plain notification
        let plain = r#"method call time=1234.0 sender=:1.100 -> destination=:1.5 serial=500 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify
   string "firefox"
   uint32 0
   string "firefox-icon"
   string "New message"
   string "You have a new message"
   array [
   ]
   array [
   ]
   int32 -1
"#;
        let msg = expect_single_notify(plain);
        assert_eq!(
            msg,
            DbusMessage::Notify {
                serial: 500,
                app_name: "firefox".into(),
                replaces_id: 0,
                desktop_entry_hint: None,
                summary: "New message".into(),
                body: "You have a new message".into(),
            }
        );

        // Case 2: with desktop-entry hint
        let with_hint = r#"method call time=1234.0 sender=:1.100 -> destination=:1.5 serial=501 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify
   string "System Notifications"
   uint32 0
   string ""
   string "New message"
   string "body"
   array [
   ]
   dict entry(
      string "desktop-entry"
      variant string "com.slack.Slack"
   )
   int32 -1
"#;
        let msg = expect_single_notify(with_hint);
        assert_eq!(
            msg,
            DbusMessage::Notify {
                serial: 501,
                app_name: "System Notifications".into(),
                replaces_id: 0,
                desktop_entry_hint: Some("com.slack.Slack".into()),
                summary: "New message".into(),
                body: "body".into(),
            }
        );
    }

    #[test]
    fn test_parse_method_return() {
        let input = r#"method return time=1234.0 sender=:1.5 -> destination=:1.100 serial=600 reply_serial=500
   uint32 42
"#;
        let msgs = parse_block(input);
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            msgs[0],
            DbusMessage::NotifyReply {
                reply_serial: 500,
                notification_id: 42,
            }
        );
    }

    #[test]
    fn test_parse_notification_closed() {
        let input = r#"signal time=1234.0 sender=:1.5 -> destination=(null destination) serial=700 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=NotificationClosed
   uint32 42
   uint32 2
"#;
        let msgs = parse_block(input);
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            msgs[0],
            DbusMessage::NotificationClosed {
                notification_id: 42
            }
        );
    }

    #[test]
    fn test_parse_close_call_ignored() {
        let input = r#"method call time=1234.0 sender=:1.100 -> destination=:1.5 serial=800 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=CloseNotification
   uint32 42
"#;
        let msgs = parse_block(input);
        assert_eq!(msgs.len(), 0); // CloseNotification calls are ignored (signal follows)
    }

    #[test]
    fn test_parse_unrelated_messages_ignored() {
        let input = r#"signal time=1234.0 sender=:1.5 -> destination=(null destination) serial=900 path=/org/freedesktop/DBus; interface=org.freedesktop.DBus; member=NameOwnerChanged
   string ":1.200"
   string ""
   string ":1.200"
method call time=1234.0 sender=:1.100 -> destination=:1.5 serial=901 path=/org/kde/StatusNotifierWatcher; interface=org.kde.StatusNotifierWatcher; member=RegisterStatusNotifierItem
   string "/StatusNotifierItem"
"#;
        let msgs = parse_block(input);
        assert_eq!(msgs.len(), 0);
    }

    #[test]
    fn test_parse_notify_with_replaces_id() {
        let input = r#"method call time=1234.0 sender=:1.100 -> destination=:1.5 serial=502 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify
   string "firefox"
   uint32 42
   string "firefox-icon"
   string "Updated message"
   string "Updated body"
   array [
   ]
   array [
   ]
   int32 -1
"#;
        let msgs = parse_block(input);
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            DbusMessage::Notify { replaces_id, .. } => {
                assert_eq!(*replaces_id, 42);
            }
            other => panic!("Expected Notify, got {:?}", other),
        }
    }

    #[test]
    fn test_multi_message_stream() {
        let input = r#"method call time=1234.0 sender=:1.100 -> destination=:1.5 serial=500 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify
   string "firefox"
   uint32 0
   string "icon"
   string "summary"
   string "body"
method return time=1234.1 sender=:1.5 -> destination=:1.100 serial=600 reply_serial=500
   uint32 42
signal time=1234.2 sender=:1.5 -> destination=(null destination) serial=700 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=NotificationClosed
   uint32 42
   uint32 2
"#;
        let msgs = parse_block(input);
        assert_eq!(msgs.len(), 3);
        assert!(matches!(msgs[0], DbusMessage::Notify { .. }));
        assert!(matches!(msgs[1], DbusMessage::NotifyReply { .. }));
        assert!(matches!(msgs[2], DbusMessage::NotificationClosed { .. }));
    }

    #[test]
    fn test_method_return_without_uint32_ignored() {
        let input = r#"method return time=1234.0 sender=:1.5 -> destination=:1.100 serial=600 reply_serial=999
   string "some string"
"#;
        let msgs = parse_block(input);
        assert_eq!(msgs.len(), 0); // No uint32 means not a Notify reply
    }

    #[test]
    fn test_empty_input() {
        let msgs = parse_block("");
        assert_eq!(msgs.len(), 0);
    }

    #[test]
    fn test_default_parser() {
        let mut parser = DbusParser::default();
        assert_eq!(parser.flush(), None);
    }

    #[test]
    fn test_notify_without_strings_produces_nothing() {
        // Notify header followed immediately by another header — no strings collected
        let input = "method call time=1234.0 sender=:1.100 -> destination=:1.5 serial=500 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify\nmethod call time=1234.0 sender=:1.100 -> destination=:1.5 serial=501 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify\n";
        let msgs = parse_block(input);
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_desktop_entry_hint_only_from_notify() {
        // A non-Notify message with "desktop-entry" should not be captured
        let input = r#"signal time=1234.0 sender=:1.5 -> destination=(null destination) serial=700 path=/some/path; interface=some.Interface; member=SomeSignal
   string "desktop-entry"
   variant string "com.slack.Slack"
   uint32 99
"#;
        let msgs = parse_block(input);
        assert_eq!(msgs.len(), 0); // Not a recognized message type
    }
}
