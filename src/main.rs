use log::{error, info, warn};
use plasma_task_manager_notifications::{
    AppMap, BadgeEmitter, DbusMessage, DbusParser, FloorpResolver,
};
use std::io::BufRead;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use zbus::interface;

/// How often (seconds) to re-scan open windows and rebuild the app map.
const DISCOVERY_INTERVAL: u64 = 30;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let app_map = Arc::new(Mutex::new(AppMap::new()));
    let floorp = FloorpResolver::discover();

    install_signal_handler(&app_map);
    run_initial_discovery(&app_map);
    spawn_discovery_thread(&app_map);
    spawn_focus_dbus_thread(&app_map);
    run_monitor_loop(&app_map, floorp.as_ref());
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
    let new_map = plasma_task_manager_notifications::discover_apps();
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
        let new_map = plasma_task_manager_notifications::discover_apps();
        let mut map = app_map_disc.lock().unwrap();
        if map.update_patterns(new_map) {
            log_discovered(&map);
        }
    });
}

struct FocusService {
    app_map: Arc<Mutex<AppMap>>,
}

#[interface(name = "org.kde.PlasmaTaskManagerNotifications")]
impl FocusService {
    /// Called by the KWin helper whenever an application window gains focus.
    fn focus_app(&self, desktop_file: &str) {
        let desktop_file = desktop_file.trim().trim_end_matches(".desktop");

        if desktop_file.is_empty() {
            return;
        }

        let desktop_id = format!("application://{desktop_file}.desktop");

        let mut map = self.app_map.lock().unwrap();

        if map.clear_app(&desktop_id) {
            BadgeEmitter::emit(&desktop_id, 0);
            info!("focus -> cleared {desktop_id}");
        }
    }
}

fn spawn_focus_dbus_thread(app_map: &Arc<Mutex<AppMap>>) {
    let app_map_focus = Arc::clone(app_map);

    thread::spawn(move || {
        let connection = match zbus::blocking::Connection::session() {
            Ok(connection) => connection,
            Err(e) => {
                error!("failed to connect focus service to session D-Bus: {e}");
                return;
            }
        };

        if let Err(e) = connection.request_name("org.kde.PlasmaTaskManagerNotifications") {
            error!("failed to register focus D-Bus service: {e}");
            return;
        }

        let service = FocusService {
            app_map: app_map_focus,
        };

        if let Err(e) = connection
            .object_server()
            .at("/org/kde/PlasmaTaskManagerNotifications", service)
        {
            error!("failed to expose focus D-Bus interface: {e}");
            return;
        }

        info!("focus D-Bus service ready");

        // Keep the connection and exported object alive.
        loop {
            thread::park();
        }
    });
}

fn run_monitor_loop(app_map: &Arc<Mutex<AppMap>>, floorp: Option<&FloorpResolver>) {
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
            handle_message(msg, app_map, floorp);
        }
    }

    if let Some(msg) = parser.flush() {
        handle_message(msg, app_map, floorp);
    }

    error!("dbus-monitor exited unexpectedly");
    let map = app_map.lock().unwrap();
    BadgeEmitter::clear_all(&map.all_desktop_ids());
}

fn handle_message(msg: DbusMessage, app_map: &Arc<Mutex<AppMap>>, floorp: Option<&FloorpResolver>) {
    let mut map = app_map.lock().unwrap();

    match msg {
        DbusMessage::Notify {
            serial,
            app_name,
            replaces_id,
            desktop_entry_hint,
            summary,
            body,
        } => {
            let hint_ref = desktop_entry_hint.as_deref().unwrap_or("");
            let is_floorp =
                app_name.eq_ignore_ascii_case("firefox") && hint_ref.eq_ignore_ascii_case("floorp");
            let desktop_id = if is_floorp {
                floorp.and_then(|resolver| resolver.resolve(&summary, &body))
            } else {
                map.match_app(&[&app_name, hint_ref])
            };
            if let Some(desktop_id) = desktop_id {
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
            // Intentionally keep the badge when Plasma closes/expires
            // the popup. It will be cleared when the application's
            // window receives focus.
            info!("notification [{notification_id}] closed; keeping badge until focus");
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
