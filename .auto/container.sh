#!/usr/bin/env bash
# Frozen builder/test environment. Measured executables run on the HOST.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMMON="$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)"
IMAGE='docker.io/library/rust@sha256:f31fa9eaac4e417505e10009786ec0c636558e02dff3fa662cc92c10894ba065'
NAME="matchbox-interpreter-${PPID}-$$"
mkdir -p "$ROOT/target/build" "$ROOT/.auto/artifacts"
# Native embedded runner is unused by this campaign. Freeze its existing bytes,
# avoiding irrelevant recursive rebuilds; never patch generated tracked sources.
RUNNER="$ROOT/stubs/runner_stub_x86_64-unknown-linux-gnu"
printf '%s  %s\n' 'b2e75b0324fe0cf451f26f02c2a63d39d588b8c2ea653ec074071c180d269688' "$RUNNER" | sha256sum --check --status
if ! cmp -s "$RUNNER.features" <(printf '%s' 'bif-io,bif-http,bif-zip,bif-crypto,bif-cli,jit'); then
  printf '%s' 'bif-io,bif-http,bif-zip,bif-crypto,bif-cli,jit' > "$RUNNER.features"
fi
if find "$ROOT/crates/matchbox-runner/src/main.rs" "$ROOT/crates/matchbox-runner/Cargo.toml" \
    "$ROOT/crates/matchbox-vm/src" "$ROOT/crates/matchbox-vm/Cargo.toml" \
    "$ROOT/crates/matchbox-plugin-sdk" "$ROOT/crates/matchbox-parser" "$ROOT/build.rs" \
    -newer "$RUNNER" -print -quit | grep -q .; then
  touch "$RUNNER"
fi
BACKUP="$(mktemp "$ROOT/target/generated-stubs.XXXXXX")"
cp -p "$ROOT/src/stubs.rs" "$BACKUP"
cleanup() {
  podman rm --force "$NAME" >/dev/null 2>&1 || true
  if ! cmp -s "$BACKUP" "$ROOT/src/stubs.rs"; then cp -p "$BACKUP" "$ROOT/src/stubs.rs"; fi
  rm -f "$BACKUP"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
# No rootless cgroup delegation here: per-process 8GiB virtual-address limit,
# two Cargo jobs, and a hard 600s wall timeout. No network or toolchain installs.
timeout --kill-after=10s 600s podman run --rm --pull=never --name "$NAME" \
  --network none --userns keep-id --user "$(id -u):$(id -g)" \
  --security-opt label=disable -v "$ROOT:/work" -v "$COMMON:$COMMON:ro" -w /work \
  -e CARGO_HOME=/work/target/cargo-home -e CARGO_TARGET_DIR=/work/target/build \
  -e CARGO_BUILD_JOBS=2 -e CARGO_NET_OFFLINE=true -e CARGO_TERM_COLOR=never \
  "$IMAGE" bash -c 'ulimit -v 8388608; exec "$@"' bash "$@"
