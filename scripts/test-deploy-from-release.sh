#!/usr/bin/env bash
# arming: ci .github/workflows/ci.yml
# Exercise the actual pull-deploy script without network or a serving host.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/bin" "$scratch/release" "$scratch/payload"
cat > "$scratch/payload/bobbin" <<'BIN'
#!/usr/bin/env bash
echo 'bobbin 0.20.1'
BIN
chmod +x "$scratch/payload/bobbin"
asset=bobbin-v0.20.1-x86_64-unknown-linux-gnu.tar.gz
tar czf "$scratch/release/$asset" -C "$scratch/payload" bobbin
sum="$(sha256sum "$scratch/release/$asset")"; sum="${sum%% *}"
cat > "$scratch/bin/gh" <<'GH'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1 $2" == 'release download' ]]
while (($#)); do
  if [[ "$1" == -D ]]; then dest="$2"; shift; fi
  shift
done
cp "$FIXTURE_RELEASE/"* "$dest/"
GH
chmod +x "$scratch/bin/gh"
for mode in plain siblings missing corrupt; do
  manifest="$scratch/release/SHA256SUMS.txt"
  printf '%s  %s.sigstore.json\n%s  %s.sbom.sigstore.json\n' "$sum" "$asset" "$sum" "$asset" > "$manifest"
  case "$mode" in
    plain) printf '%s  %s\n' "$sum" "$asset" > "$manifest" ;;
    siblings) printf '%s  %s\n' "$sum" "$asset" >> "$manifest" ;;
    missing) : ;;
    corrupt) printf '%064d  %s\n' 0 "$asset" >> "$manifest" ;;
  esac
  out="$scratch/verified-$mode"
  rc=0
  env PATH="$scratch/bin:/usr/bin:/bin" FIXTURE_RELEASE="$scratch/release" \
    DEPLOY_HOST=serving.test DRY_RUN=1 DRY_RUN_OUT="$out" \
    bash "$root/scripts/deploy-from-release.sh" v0.20.1 > "$scratch/log" 2>&1 || rc=$?
  case "$mode" in
    plain|siblings)
      if [[ "$rc" != 0 ]] || ! cmp -s "$out" "$scratch/payload/bobbin"; then
        cat "$scratch/log"; echo "FAIL: $mode"; exit 1
      fi ;;
    missing|corrupt)
      if [[ "$rc" == 0 || -e "$out" ]]; then
        cat "$scratch/log"; echo "FAIL: $mode was accepted"; exit 1
      fi
      grep -q 'REFUSED:' "$scratch/log"
      if [[ "$mode" == missing ]]; then grep -q 'has no entry' "$scratch/log"; fi ;;
  esac
  echo "PASS: $mode"
done
