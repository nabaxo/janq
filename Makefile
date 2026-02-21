# Optimization: Setting MALLOC_ARENA_MAX=1 reduces glibc allocator overhead in janq.
RUN_CMD := MALLOC_ARENA_MAX=1

DIST_DIR := dist

# Flags for the two optimization levels
OPT_Z := RUSTFLAGS="-C opt-level=z"
OPT_S := RUSTFLAGS="-C opt-level=s"

# Nightly build-std with immediate-abort panics (smallest binary + lowest RSS)
NIGHTLY_FLAGS := RUSTFLAGS="-Zunstable-options -Cpanic=immediate-abort"
NIGHTLY_ARGS := +nightly -Zbuild-std=std,panic_abort

build: format lint build-linux-nightly build-windows-static

lint:
	cargo fmt --all -- --check

format:
	cargo fmt --all

prepare-dist:
	mkdir -p $(DIST_DIR)

build-linux-glibc: prepare-dist
	$(OPT_Z) cargo build --release
	cp target/release/janq $(DIST_DIR)/janq-glibc

build-linux-nightly: prepare-dist
	$(NIGHTLY_FLAGS) cargo $(NIGHTLY_ARGS) build --release --target x86_64-unknown-linux-musl
	cp target/x86_64-unknown-linux-musl/release/janq $(DIST_DIR)/janq

build-linux-musl: prepare-dist
	$(OPT_Z) cargo build --release --target x86_64-unknown-linux-musl
	cp target/x86_64-unknown-linux-musl/release/janq $(DIST_DIR)/janq-stable

build-linux: build-linux-nightly build-linux-glibc

build-windows-nonstatic: prepare-dist
	$(OPT_Z) cargo build --release --target x86_64-pc-windows-gnu
	cp target/x86_64-pc-windows-gnu/release/janq.exe $(DIST_DIR)/janq-nonstatic.exe

build-windows-static: prepare-dist
	RUSTFLAGS="-C opt-level=z -C link-arg=-static" cargo build --release --target x86_64-pc-windows-gnu
	cp target/x86_64-pc-windows-gnu/release/janq.exe $(DIST_DIR)/janq.exe

build-windows: build-windows-static build-windows-nonstatic

build-all: format lint build-linux build-linux-musl build-windows

build-all-s: format lint prepare-dist
	# Build opt-level s versions of everything
	$(OPT_S) cargo build --release
	cp target/release/janq $(DIST_DIR)/janq-glibc-s
	$(OPT_S) cargo build --release --target x86_64-unknown-linux-musl
	cp target/x86_64-unknown-linux-musl/release/janq $(DIST_DIR)/janq-stable-s
	$(OPT_S) cargo build --release --target x86_64-pc-windows-gnu
	cp target/x86_64-pc-windows-gnu/release/janq.exe $(DIST_DIR)/janq-nonstatic-s.exe
	# Build s version (janq-s.exe)
	RUSTFLAGS="-C opt-level=s -C link-arg=-static" cargo build --release --target x86_64-pc-windows-gnu
	cp target/x86_64-pc-windows-gnu/release/janq.exe $(DIST_DIR)/janq-s.exe

build-all-all: build-all build-all-s

check:
	cargo check --target x86_64-unknown-linux-gnu
	cargo check --target x86_64-pc-windows-gnu

install:
	cargo install --path .

size-compare:
	@echo "--- Binary Size Comparison ---"
	@ls -lh $(DIST_DIR)/janq $(DIST_DIR)/janq-s || true
	@ls -lh $(DIST_DIR)/janq.exe $(DIST_DIR)/janq-s.exe || true

clean:
	cargo clean
	if [ -d $(DIST_DIR) ]; then find $(DIST_DIR) -maxdepth 1 -type f ! -name "*.toml" -delete; fi
