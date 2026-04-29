PREFIX ?= $(HOME)/.local

.PHONY: build install uninstall test clean

build:
	cargo build --release

test:
	cargo test

install: build
	install -Dm755 target/release/plasma-task-manager-notifications $(PREFIX)/bin/plasma-task-manager-notifications
	install -Dm644 plasma-task-manager-notifications.service $(HOME)/.config/systemd/user/plasma-task-manager-notifications.service

uninstall:
	rm -f $(PREFIX)/bin/plasma-task-manager-notifications
	rm -f $(HOME)/.config/systemd/user/plasma-task-manager-notifications.service
	-systemctl --user stop plasma-task-manager-notifications.service 2>/dev/null
	-systemctl --user disable plasma-task-manager-notifications.service 2>/dev/null

clean:
	cargo clean
