# Common tasks. Install `just` (https://just.systems) and run `just` to list
# them. Everything here is plain cargo; CI runs the same commands.

default:
    @just --list

# Everything CI checks that runs in a few seconds to minutes.
check: fmt-check clippy test typos

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

clippy:
    cargo clippy --all-targets --all-features --locked -- -D warnings

test:
    cargo test --locked --workspace

# Review changed insta snapshots interactively.
snapshots:
    cargo insta test --review --workspace

docs:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --locked

coverage:
    cargo llvm-cov --locked --fail-under-lines 80
    cargo llvm-cov --locked -p prunecss-core --fail-under-lines 95

# Build with the minimum supported Rust version.
msrv:
    RUSTUP_TOOLCHAIN=1.88 cargo check --workspace --all-targets --locked

deny:
    cargo deny --locked check

typos:
    typos

# Check that both crates would publish.
package:
    cargo package --workspace --locked

# Fuzz one target for a while, e.g. `just fuzz prune_no_panic 300`.
fuzz target="prune_no_panic" seconds="60":
    cd fuzz && cargo +nightly fuzz run {{target}} -s none -- -max_total_time={{seconds}} -timeout=1 -dict=dict/css.dict

# Criterion micro-benchmarks.
bench-micro:
    cargo bench -p prunecss-core

# The full comparison with PurgeCSS (see bench/README.md).
bench:
    make -C bench

# Everything CI runs, slow checks included.
ci: check docs msrv coverage deny package
