#!/usr/bin/env bash
# Exercise the aegis-egqrv4 latest-decision.
#
# `decide` below is the same comparison release.yml performs before choosing
# between `--latest` and `--latest=false`. The static contract in
# test-release-workflow-contract.py asserts the workflow still SPELLS it this
# way (sort -V, --exclude-drafts, both branches present); this asserts the
# comparison is CORRECT.
#
# The load-bearing case is 0.9.0 against 0.10.0: a lexical sort ranks 0.9.0
# higher and inverts the guard in BOTH directions, which is why `sort -V` is not
# a stylistic choice. Substituting plain `sort` fails exactly those two cases.
decide() {
  local mine_raw="$1"; shift
  local highest; highest="$(printf '%s\n' "$@" | sed 's/^v//' | sort -V | tail -1)"
  local mine="${mine_raw#v}"
  case "$mine" in *-*) echo "not-latest"; return ;; esac
  if [ -z "$highest" ] || [ "$(printf '%s\n%s\n' "$highest" "$mine" | sort -V | tail -1)" = "$mine" ]; then
    echo "latest"
  else
    echo "not-latest"
  fi
}
fails=0
check() { # expected, label, mine, published...
  local want="$1" label="$2" mine="$3"; shift 3
  local got; got="$(decide "$mine" "$@")"
  if [ "$got" = "$want" ]; then printf '  PASS  %-46s -> %s\n' "$label" "$got"
  else printf '  FAIL  %-46s -> %s (wanted %s)\n' "$label" "$got" "$want"; fails=1; fi
}
echo "=== aegis-egqrv4 latest-gate ==="
check latest      "newer than everything published"   v0.16.0 v0.15.0 v0.14.0
check not-latest  "OLDER than a published release"    v0.15.0 v0.16.0 v0.14.0
check latest      "equal to highest (re-publish)"     v0.16.0 v0.16.0 v0.15.0
check latest      "no published releases at all"      v0.1.0
check not-latest  "0.9.0 while 0.10.0 published"      v0.9.0  v0.10.0
check latest      "0.10.0 while 0.9.0 published"      v0.10.0 v0.9.0
check latest      "1.0.0 over 0.99.0"                 v1.0.0  v0.99.0
check not-latest  "0.16.0 while 1.0.0 published"      v0.16.0 v1.0.0
check not-latest  "PRERELEASE never claims latest"      v0.17.0-rc1 v0.16.0
check not-latest  "prerelease below its own final"     v0.17.0-rc1 v0.17.0
check latest      "the final release after an rc"      v0.17.0     v0.16.0
echo
[ $fails -eq 0 ] && echo "ALL PASS" || echo "FAILURES"
exit $fails
