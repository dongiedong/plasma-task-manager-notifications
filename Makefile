PREFIX ?= $(HOME)/.local

.PHONY: build install uninstall test clean

build:
	cargo build --release

test:
	cargo test

install: build
	install -Dm755 target/release/notification-badge $(PREFIX)/bin/notification-badge
	install -Dm644 notification-badge.service $(HOME)/.config/systemd/user/notification-badge.service

uninstall:
	rm -f $(PREFIX)/bin/notification-badge
	rm -f $(HOME)/.config/systemd/user/notification-badge.service
	-systemctl --user stop notification-badge.service 2>/dev/null
	-systemctl --user disable notification-badge.service 2>/dev/null

clean:
	cargo clean
