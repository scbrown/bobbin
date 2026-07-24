#!/usr/bin/env bash
# test-deploy-cutover.sh — prove deploy-cutover.sh's safety contract WITHOUT a real
# serving host (bobbin-nimn0 acceptance #1/#2). The serving host is mocked via
# BOBBIN_SSH: the mock simulates `--version` on the staged binary (good or a
# glibc-mismatch failure) and the smoke probes, and LOGS every command so we can
# assert whether a cutover / rollback actually happened.
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CUTOVER="$HERE/deploy-cutover.sh"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
MOCK="$TMP/mock-host.sh"
LOG="$TMP/cmds.log"

cat > "$MOCK" <<'MOCKEOF'
#!/usr/bin/env bash
# $1 = root@HOST ; $2 = the command string. Logs $2, simulates responses.
printf '%s\n' "$2" >> "$MOCK_LOG"
cmd="$2"
case "$cmd" in
  *bobbin.new*--version*)   # THE GATE — the staged binary's --version
    if [ "${MOCK_STAGE:-good}" = bad ]; then
      echo "/usr/local/bin/bobbin.new: /lib/x86_64-linux-gnu/libc.so.6: version \`GLIBC_2.43' not found" >&2
      exit 1
    fi
    echo "bobbin 0.6.0"; exit 0 ;;
  *--version*)              # the LIVE binary's --version (smoke)
    [ "${MOCK_SMOKE_VER:-good}" = bad ] && exit 1
    echo "bobbin 0.6.0"; exit 0 ;;
  *grep*Knowledge\ graph\ tools*bobbin.new*)  # THE FEATURE GATE — sentinel probe on the staged binary
    # grep exits 0 when the featureless sentinel is FOUND. Default: a knowledge
    # build (sentinel absent) -> exit 1. MOCK_STAGE_FEATURE=featureless -> found -> 0.
    [ "${MOCK_STAGE_FEATURE:-knowledge}" = featureless ] && exit 0 ; exit 1 ;;
  *is-active*)
    [ "${MOCK_SMOKE_ACTIVE:-good}" = bad ] && exit 1 ; exit 0 ;;
  *curl*)
    [ "${MOCK_SMOKE_HTTP:-good}" = bad ] && { echo 000; exit 0; } ; echo 200; exit 0 ;;
  *) exit 0 ;;             # cp / mv / mkdir / restart / chmod / rm all "succeed"
esac
MOCKEOF
chmod +x "$MOCK"

run_cutover() {  # runs the cutover with the mock; returns its exit code
  : > "$LOG"
  env DEPLOY_HOST=serving.test NEW_ON_HOST=1 SMOKE_WAIT=0 \
      BOBBIN_SSH="$MOCK" MOCK_LOG="$LOG" \
      "$@" bash "$CUTOVER" /some/new/bobbin >/dev/null 2>&1
}

pass=0; fail=0
check() { # name  expected(0/nonzero)  actual  extra-assert-file-grep...
  local name="$1" want="$2" got="$3"; shift 3
  local ok=1
  { [ "$want" = 0 ] && [ "$got" -eq 0 ]; } || { [ "$want" != 0 ] && [ "$got" -ne 0 ]; } || ok=0
  for pat in "$@"; do
    case "$pat" in
      "!"*) grep -qF -- "${pat#!}" "$LOG" && ok=0 ;;   # must NOT appear
      *)    grep -qF -- "$pat"     "$LOG" || ok=0 ;;   # must appear
    esac
  done
  if [ "$ok" = 1 ]; then echo "  PASS  $name"; pass=$((pass+1))
  else echo "  FAIL  $name (exit=$got)"; fail=$((fail+1)); fi
}

# 1. A bad (glibc-mismatched) staged binary MUST be refused: exit!=0, and the live
#    binary is never touched — no atomic mv over it, no restart. (Patterns match the
#    single-quoted paths as they appear in the logged host commands.)
run_cutover MOCK_STAGE=bad
check "bad binary is REFUSED, no cutover" 1 $? \
  "!mv -f '/usr/local/bin/bobbin.new' '/usr/local/bin/bobbin'" \
  "!systemctl restart bobbin"

# 2. A good binary cuts over: exit 0, a rollback snapshot (live -> prev) is made,
#    then the atomic mv + restart.
run_cutover MOCK_STAGE=good
check "good binary deploys + prev snapshot exists" 0 $? \
  "cp -f '/usr/local/bin/bobbin' '/opt/bobbin/bobbin.prev'" \
  "mv -f '/usr/local/bin/bobbin.new' '/usr/local/bin/bobbin'" \
  "systemctl restart bobbin"

# 3. Gate passes but post-cutover smoke fails -> ROLL BACK to prev (never leave the
#    service on a bad binary). The rollback copies prev -> live (opposite direction
#    to the step-2 snapshot).
run_cutover MOCK_STAGE=good MOCK_SMOKE_HTTP=bad
check "smoke failure rolls back to prev" 1 $? \
  "cp -f '/opt/bobbin/bobbin.prev' '/usr/local/bin/bobbin'"

# 4. FEATURE GATE (deploy-feature regression): a featureless staged binary passes --version but MUST
#    be refused by the knowledge probe — no cutover, live binary untouched.
run_cutover MOCK_STAGE=good MOCK_STAGE_FEATURE=featureless
check "featureless binary is REFUSED, no cutover" 1 $? \
  "!mv -f '/usr/local/bin/bobbin.new' '/usr/local/bin/bobbin'" \
  "!systemctl restart bobbin"

# 5. A knowledge-enabled binary passes the feature gate and deploys normally.
run_cutover MOCK_STAGE=good MOCK_STAGE_FEATURE=knowledge
check "knowledge binary passes the feature gate + deploys" 0 $? \
  "mv -f '/usr/local/bin/bobbin.new' '/usr/local/bin/bobbin'"

# 6. The escape hatch: REQUIRE_KNOWLEDGE=0 lets a featureless binary through
#    deliberately (never silently — the operator typed it).
run_cutover MOCK_STAGE=good MOCK_STAGE_FEATURE=featureless REQUIRE_KNOWLEDGE=0
check "REQUIRE_KNOWLEDGE=0 allows a deliberate featureless deploy" 0 $? \
  "mv -f '/usr/local/bin/bobbin.new' '/usr/local/bin/bobbin'"

echo "cutover selftest: ${pass} passed, ${fail} failed"
[ "$fail" -eq 0 ]
