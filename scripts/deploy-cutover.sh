#!/usr/bin/env bash
# deploy-cutover.sh — cut a freshly-built bobbin binary into service on the serving
# host SAFELY, with the on-serving-host `--version` gate as the last-line ABI safety
# net (bobbin-nimn0).
#
# WHY THIS EXISTS. The old deploy `mv`d the new binary over the live one and
# restarted, THEN smoke-tested — so a binary built for the wrong glibc (the classic
# "built on a host whose glibc is newer than the serving host" landmine) took the
# service DOWN before anything checked it, with no copy to roll back to. bobbin's
# only build-time ABI constraint is glibc (ort/onnxruntime are dlopen'd at runtime,
# `load-dynamic`), so `--version` run ON THE SERVING HOST is exactly the probe that
# catches a glibc mismatch: a too-new-glibc binary dies with "GLIBC_x.yz not found"
# at `--version`, before it can replace anything.
#
# THE CONTRACT:
#   1. stage the new binary at $STAGE on the serving host (never over the live one)
#   2. GATE: run `$STAGE --version` on the serving host. FAIL -> refuse, remove the
#      stage, leave the live binary untouched, exit 1. Nothing was cut over.
#   3. only on a passing gate: snapshot live -> $PREV, atomic mv stage -> live,
#      restart, smoke-test; on ANY smoke failure ROLL BACK to $PREV and restart.
#
# Usage:  deploy-cutover.sh <new-binary>
# Env:
#   DEPLOY_HOST   serving host (required)
#   NEW_ON_HOST   "1" if <new-binary> is ALREADY a path on the serving host (built
#                 there); otherwise it is scp'd up first.
#   BOBBIN_SSH / BOBBIN_SCP   the ssh/scp command prefixes (defaults use the deploy
#                 key). Overridden in tests by a mock that simulates the host.
#   SMOKE_WAIT    seconds to wait after restart before smoke (default 3).
#   HEALTH_URL    health endpoint on the serving host (default :3000/status).
set -euo pipefail

NEW="${1:?usage: deploy-cutover.sh <new-binary>}"
HOST="${DEPLOY_HOST:?set DEPLOY_HOST (the serving host)}"
LIVE="${LIVE_BIN:-/usr/local/bin/bobbin}"
STAGE="${STAGE_BIN:-/usr/local/bin/bobbin.new}"
PREV="${PREV_BIN:-/opt/bobbin/bobbin.prev}"
HEALTH_URL="${HEALTH_URL:-http://127.0.0.1:3000/status}"
BOBBIN_SSH="${BOBBIN_SSH:-ssh -i $HOME/.ssh/deploy_key -o StrictHostKeyChecking=accept-new}"
BOBBIN_SCP="${BOBBIN_SCP:-scp -i $HOME/.ssh/deploy_key -o StrictHostKeyChecking=accept-new}"

on_host() { $BOBBIN_SSH "root@$HOST" "$@"; }

# --- 0. stage the new binary (never onto the live path) ---------------------
if [ "${NEW_ON_HOST:-0}" = 1 ]; then
  on_host "cp -f '$NEW' '$STAGE' && chmod +x '$STAGE'"
else
  $BOBBIN_SCP "$NEW" "root@$HOST:$STAGE"
  on_host "chmod +x '$STAGE'"
fi

# --- 1. THE GATE: --version on the STAGED binary, on the serving host --------
# A glibc-mismatched or otherwise-broken binary is caught HERE, before it can
# touch the live one. No cutover happens unless this passes.
if ! ver="$(on_host "'$STAGE' --version" 2>&1)" || ! printf '%s' "$ver" | grep -qi bobbin; then
  echo "::error::REFUSED cutover — staged binary failed --version on $HOST (likely a glibc/ABI mismatch): ${ver}" >&2
  on_host "rm -f '$STAGE'" || true
  exit 1
fi
echo "gate PASS on $HOST: $ver"

# --- 2. snapshot the live binary, then atomic swap + restart ----------------
on_host "mkdir -p \"\$(dirname '$PREV')\" && cp -f '$LIVE' '$PREV' && mv -f '$STAGE' '$LIVE' && cp -f '$LIVE' /opt/bobbin/bobbin && systemctl restart bobbin"
sleep "${SMOKE_WAIT:-3}"

# --- 3. post-cutover smoke; roll back to $PREV on ANY failure ---------------
fail=0
on_host "systemctl is-active --quiet bobbin" || fail=1
[ "$(on_host "curl -sf -o /dev/null -w '%{http_code}' '$HEALTH_URL'" 2>/dev/null || echo 000)" = "200" ] || fail=1
on_host "'$LIVE' --version" 2>/dev/null | grep -qi bobbin || fail=1
if [ "$fail" -ne 0 ]; then
  echo "::error::smoke FAILED after cutover — rolling back to $PREV" >&2
  on_host "cp -f '$PREV' '$LIVE' && cp -f '$PREV' /opt/bobbin/bobbin && systemctl restart bobbin"
  exit 1
fi
echo "cutover OK on $HOST: $ver"
