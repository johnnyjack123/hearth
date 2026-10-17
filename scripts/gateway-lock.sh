#!/usr/bin/env bash
# Regenerates images/gateway/Cargo.lock, the lock file the gateway image is
# built with (`cargo build --locked` inside the VM).
#
# It is the workspace's Cargo.lock pruned to what harness-gateway needs: the
# crate is copied alone into a temporary directory together with the
# workspace lock, and cargo drops every package the crate does not use while
# keeping the versions and checksums of the rest. Run it after changing the
# gateway's dependencies or after `cargo update`; the test
# `gateway_lock_matches_the_workspace` (harness-core, image.rs) fails until
# then.
#
# Needs no network if the workspace has been built once (the registry index
# is cached); otherwise drop --offline below.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

cp "$REPO/crates/harness-gateway/Cargo.toml" "$WORK/"
cp -r "$REPO/crates/harness-gateway/src" "$WORK/"
cp "$REPO/Cargo.lock" "$WORK/"
# Resolving against the copied lock rewrites it: unused packages go, the
# versions of the others stay as they are.
cargo metadata --offline --format-version 1 --manifest-path "$WORK/Cargo.toml" > /dev/null
cp "$WORK/Cargo.lock" "$REPO/images/gateway/Cargo.lock"
echo "images/gateway/Cargo.lock: $(grep -c '^name = ' "$REPO/images/gateway/Cargo.lock") Pakete"
