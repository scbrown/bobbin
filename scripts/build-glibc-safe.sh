#!/usr/bin/env bash
# build-glibc-safe.sh — build a bobbin binary that will actually RUN on the
# serving host, from a clean checkout, in a container.
#
# WHY THIS EXISTS (aegis-6iqv5). During the aegis-ks9cl outage the code fix took
# minutes and getting it onto the serving host is what turned a fix into an
# outage that outlived it. Three mundane things, each of which cost real time:
#
#   1. There is no cargo on the serving host — you cannot build where it runs.
#   2. glibc skew. A build host newer than the serving host produces a binary
#      that fails at EXEC time, not build time, which is the worst place to find
#      out. Measured during that incident: builder 2.43, serving host 2.42, and
#      the resulting binary required GLIBC_2.43.
#   3. The documented container build omitted protobuf-compiler, so it died on
#      `protoc` several minutes in, AFTER compiling ~1900 crates. A recipe that
#      does not build from clean is one that has only ever been run by somebody
#      who already had the dependency.
#
# This script is that recipe, made repeatable. It was previously an emergency
# procedure rediscovered under pressure by the one person who knew to try it.
#
# It builds in bullseye (glibc 2.31) so the artifact runs anywhere newer, from a
# FRESH CLONE so build.rs can stamp the real commit sha (a binary that reports
# `git_sha: unknown` cannot be checked against what you meant to deploy), and it
# verifies the artifact ON the target before you trust it.
#
#   BUILD_HOST=<host with docker>  TARGET_HOST=<serving host>  ./build-glibc-safe.sh
#
# Then hand the printed path to scripts/deploy-cutover.sh, which gates and
# rolls back. This script deliberately does NOT deploy: build and cutover are
# separate so a bad build cannot reach the live binary.
set -euo pipefail

BUILD_HOST="${BUILD_HOST:?set BUILD_HOST (a host with docker; must NOT have a newer glibc than the target)}"
TARGET_HOST="${TARGET_HOST:-$BUILD_HOST}"
REPO_URL="${REPO_URL:-https://github.com/scbrown/bobbin}"
REF="${REF:-main}"
SRC="${SRC:-/tmp/bobbin-src}"
SSH="${BUILD_SSH:-ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new}"
IMAGE="${BUILD_IMAGE:-rust:1-bullseye}"
JOBS="${BUILD_JOBS:-6}"          # capped: an all-core build on a shared host is a thermal event
NICE="${BUILD_NICE:-19}"

on_build()  { $SSH "root@$BUILD_HOST"  "$@"; }
on_target() { $SSH "root@$TARGET_HOST" "$@"; }

echo "==> glibc check (build must be <= target, or the binary will not exec)"
bg=$(on_build  'ldd --version | head -1 | grep -o "[0-9]\+\.[0-9]\+$"')
tg=$(on_target 'ldd --version | head -1 | grep -o "[0-9]\+\.[0-9]\+$"')
echo "    build host glibc=$bg   target glibc=$tg   (container is $IMAGE, older than both)"

echo "==> fresh clone (NOT a tarball: build.rs needs .git to stamp the sha)"
on_build "rm -rf '$SRC' && git clone --quiet '$REPO_URL' '$SRC' && cd '$SRC' && git checkout --quiet '$REF' && git rev-parse --short HEAD"

echo "==> build in $IMAGE (protobuf-compiler is REQUIRED; lance-encoding shells out to protoc)"
on_build "cd '$SRC' && nice -n $NICE docker run --rm \
  -v \"\$PWD\":/src -w /src \
  -v bobbin-cargo-registry:/usr/local/cargo/registry \
  $IMAGE \
  bash -euc 'apt-get update -qq && apt-get install -y -qq protobuf-compiler >/dev/null && cargo build --release --features knowledge --jobs $JOBS'"

echo "==> verify the artifact ON THE TARGET before trusting it"
on_build "test -x '$SRC/target/release/bobbin'"
maxglibc=$(on_build "objdump -T '$SRC/target/release/bobbin' | grep -o 'GLIBC_[0-9.]*' | sort -u -V | tail -1")
echo "    highest GLIBC symbol required: $maxglibc (target provides $tg)"
if [ "$BUILD_HOST" = "$TARGET_HOST" ]; then
  on_target "'$SRC/target/release/bobbin' --version"
else
  echo "    (build host != target: deploy-cutover.sh runs the --version gate on the target)"
fi

echo
echo "built: $SRC/target/release/bobbin"
echo "next:  DEPLOY_HOST=$TARGET_HOST NEW_ON_HOST=1 bash scripts/deploy-cutover.sh $SRC/target/release/bobbin"
