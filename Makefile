DIST_DIR := dist

build: format lint build-linux-musl build-windows-static

lint:
	cargo fmt --all -- --check

format:
	cargo fmt --all

prepare-dist:
	mkdir -p $(DIST_DIR)

build-linux-glibc: prepare-dist
	cargo build --release
	cp target/release/janq $(DIST_DIR)/janq-glibc

build-linux-musl: prepare-dist
	cargo build --release --target x86_64-unknown-linux-musl
	cp target/x86_64-unknown-linux-musl/release/janq $(DIST_DIR)/janq

build-linux: build-linux-musl build-linux-glibc

build-windows-nonstatic: prepare-dist
	cargo build --release --target x86_64-pc-windows-gnu
	cp target/x86_64-pc-windows-gnu/release/janq.exe $(DIST_DIR)/janq-nonstatic.exe

build-windows-static: prepare-dist
	RUSTFLAGS="-C link-arg=-static" cargo build --release --target x86_64-pc-windows-gnu
	cp target/x86_64-pc-windows-gnu/release/janq.exe $(DIST_DIR)/janq.exe

build-windows: build-windows-static build-windows-nonstatic

build-all: build-linux build-windows

check:
	cargo check --target x86_64-unknown-linux-gnu
	cargo check --target x86_64-pc-windows-gnu

install:
	cargo install --path .

clean:
	cargo clean
	if [ -d $(DIST_DIR) ]; then find $(DIST_DIR) -maxdepth 1 -type f ! -name "*.toml" -delete; fi
