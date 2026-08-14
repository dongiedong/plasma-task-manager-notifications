PREFIX ?= $(HOME)/.local
KWIN_SCRIPT_ID := plasma-task-manager-notifications-focus

.PHONY: build install uninstall test clean

build:
	cargo build --release

test:
	cargo test

install: build
	install -Dm755 target/release/plasma-task-manager-notifications $(PREFIX)/bin/plasma-task-manager-notifications
	install -Dm644 plasma-task-manager-notifications.service $(HOME)/.config/systemd/user/plasma-task-manager-notifications.service
	kpackagetool6 --type=KWin/Script -u kwin-script >/dev/null 2>&1 || \
		kpackagetool6 --type=KWin/Script -i kwin-script
	kwriteconfig6 --file kwinrc --group Plugins --key $(KWIN_SCRIPT_ID)Enabled true
	-qdbus6 org.kde.KWin /KWin reconfigure 2>/dev/null

uninstall:
	-systemctl --user stop plasma-task-manager-notifications.service 2>/dev/null
	-systemctl --user disable plasma-task-manager-notifications.service 2>/dev/null
	-kwriteconfig6 --file kwinrc --group Plugins --key $(KWIN_SCRIPT_ID)Enabled false
	-qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript $(KWIN_SCRIPT_ID) 2>/dev/null
	-kpackagetool6 --type=KWin/Script -r $(KWIN_SCRIPT_ID) 2>/dev/null
	rm -f $(PREFIX)/bin/plasma-task-manager-notifications
	rm -f $(HOME)/.config/systemd/user/plasma-task-manager-notifications.service

clean:
	cargo clean
