use log::{error, info, warn};
use notification_badge::{AppMap, BadgeEmitter, DbusMessage, DbusParser};
use std::io::BufRead;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// How often (seconds) to re-scan open windows and rebuild the app map.
const DISCOVERY_INTERVAL: u64 = 30;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let app_map = Arc::new(Mutex::new(AppMap::new()));

    install_signal_handler(&app_map);
    run_initial_discovery(&app_map);
    spawn_discovery_thread(&app_map);
    run_monitor_loop(&app_map);
}

fn install_signal_handler(app_map: &Arc<Mutex<AppMap>>) {
    let app_map_sig = Arc::clone(app_map);
    ctrlc::handle(move || {
        info!("shutting down, clearing badges...");
        let map = app_map_sig.lock().unwrap();
        BadgeEmitter::clear_all(&map.all_desktop_ids());
        std::process::exit(0);
    });
}

fn run_initial_discovery(app_map: &Arc<Mutex<AppMap>>) {
    let new_map = notification_badge::discover_apps();
    let mut map = app_map.lock().unwrap();
    if map.update_patterns(new_map) {
        log_discovered(&map);
    } else {
        warn!("no apps discovered, will retry...");
    }
}

fn spawn_discovery_thread(app_map: &Arc<Mutex<AppMap>>) {
    let app_map_disc = Arc::clone(app_map);
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(DISCOVERY_INTERVAL));
        let new_map = notification_badge::discover_apps();
        let mut map = app_map_disc.lock().unwrap();
        if map.update_patterns(new_map) {
            log_discovered(&map);
        }
    });
}

fn run_monitor_loop(app_map: &Arc<Mutex<AppMap>>) {
    let mut monitor = Command::new("dbus-monitor")
        .args(["--session"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start dbus-monitor");

    let stdout = monitor
        .stdout
        .take()
        .expect("failed to capture dbus-monitor stdout");

    let reader = std::io::BufReader::new(stdout);
    let mut parser = DbusParser::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                error!("read error: {e}");
                break;
            }
        };

        if let Some(msg) = parser.feed_line(&line) {
            handle_message(msg, app_map);
        }
    }

    if let Some(msg) = parser.flush() {
        handle_message(msg, app_map);
    }

    error!("dbus-monitor exited unexpectedly");
    let map = app_map.lock().unwrap();
    BadgeEmitter::clear_all(&map.all_desktop_ids());
}

fn handle_message(msg: DbusMessage, app_map: &Arc<Mutex<AppMap>>) {
    let mut map = app_map.lock().unwrap();

    match msg {
        DbusMessage::Notify {
            serial,
            app_name,
            replaces_id,
            desktop_entry_hint,
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
                BadgeEmitter::emit(&desktop_id, count);
                info!("+ [{notification_id}] {desktop_id} (count: {count})");
            }
        }
        DbusMessage::NotificationClosed { notification_id } => {
            if let Some((desktop_id, count)) = map.notification_closed(notification_id) {
                BadgeEmitter::emit(&desktop_id, count);
                info!("- [{notification_id}] {desktop_id} (count: {count})");
            }
        }
    }
}

fn log_discovered(map: &AppMap) {
    let mut apps: Vec<_> = map.patterns().values().collect();
    apps.sort();
    apps.dedup();
    info!(
        "discovered apps: [{}]",
        apps.iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

/// Minimal signal handler using libc.
mod ctrlc {
    pub fn handle<F: Fn() + Send + 'static>(handler: F) {
        unsafe {
            libc::signal(
                libc::SIGINT,
                signal_handler as *const () as libc::sighandler_t,
            );
            libc::signal(
                libc::SIGTERM,
                signal_handler as *const () as libc::sighandler_t,
            );
        }

        HANDLER.lock().unwrap().replace(Box::new(move || handler()));
    }

    use std::sync::Mutex;
    static HANDLER: Mutex<Option<Box<dyn Fn() + Send>>> = Mutex::new(None);

    extern "C" fn signal_handler(_sig: libc::c_int) {
        if let Ok(guard) = HANDLER.lock() {
            if let Some(ref handler) = *guard {
                handler();
            }
        }
    }
}
