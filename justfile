run *args:
    cargo run --bin youtube-captions-dl -- {{args}}

run-ai *args:
    AGENT=1 cargo run --bin youtube-captions-dl -- {{args}}

build:
    cargo build --locked --release --bin youtube-captions-dl
    mkdir -p bin
    cp target/release/youtube-captions-dl bin/youtube-captions-dl

test:
    cargo test --locked --all-targets

lint:
    rustfmt --check --edition 2024 --config skip_children=true src/lib.rs src/main.rs src/cache.rs src/cli.rs src/transcript.rs tests/cache.rs tests/captions.rs tests/runtime.rs
    cargo clippy --locked --all-targets -- -D warnings

coverage:
    cargo llvm-cov --locked --all-targets --fail-under-lines 90

check: lint test coverage
