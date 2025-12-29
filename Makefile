DIST_DIR := dist

build: build-linux build-windows

prepare-dist:
	mkdir -p $(DIST_DIR)

build-linux: prepare-dist
	cargo build --release --target x86_64-unknown-linux-gnu
	cp target/x86_64-unknown-linux-gnu/release/ruake $(DIST_DIR)/ruake

build-windows: prepare-dist
	cargo build --release --target x86_64-pc-windows-gnu
	cp target/x86_64-pc-windows-gnu/release/ruake.exe $(DIST_DIR)/ruake.exe

build-all: build-linux build-windows

check:
	cargo check --target x86_64-unknown-linux-gnu
	cargo check --target x86_64-pc-windows-gnu

install:
	cargo install --path .

clean:
	cargo clean
	rm -rf $(DIST_DIR)
