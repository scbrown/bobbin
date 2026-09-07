#!/usr/bin/env bash
# Refuse to publish anything that is not the version this run means to ship.
#
# Called by .github/actions/crates-publish before `cargo publish` (aegis-g1kdsx).
# It lives in a script rather than inline in the action for one reason: the
# refusals are the safety property, and a refusal that has never been observed
# is a claim. `--selftest` observes all of them.
#
# THE REFUSALS HOLD WITH TRUSTED PUBLISHING FULLY WORKING. It would be easy to
# call this lane safe today because crates.io TP is unconfigured, so a stray
# publish cannot succeed. That is not safety — it is an untested procedure
# standing behind someone else's outage, and it expires silently the moment TP
# is configured. Nothing below consults TP.
#
#   usage: crates-publish-guard.sh <expected-version> <actual-version> <ref-name>
#          crates-publish-guard.sh --selftest
#
#   exit 0  publish may proceed
#   exit 1  refused (the reason is on stderr)
#   exit 2  called wrong

set -uo pipefail

guard() {
    local expected="$1" actual="$2" ref="$3"

    if ! [[ "$expected" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
        echo "REFUSED: expected version must be a stable numeric version." >&2
        return 1
    fi

    # `cargo publish` publishes whatever is in Cargo.toml regardless of which
    # tag invoked it, so "which version are we publishing" is not knowable from
    # the tag and must be asserted rather than assumed.
    if [ "$actual" != "$expected" ]; then
        echo "REFUSED: the crate is at ${actual} but this run intends to publish ${expected}." >&2
        echo "         cargo publish would ship ${actual} under a tag nobody meant." >&2
        return 1
    fi

    # Checked on the VERSION, not on a GitHub release's `prerelease` flag: the
    # release lane calls this from a push, where there is no release object to
    # read, so a flag-based check would silently not apply on the path that
    # matters most.
    case "$actual" in
        *-*) echo "REFUSED: ${actual} is a prerelease version." >&2; return 1 ;;
    esac

    # docs/RELEASING.md tells you to rehearse by pushing a throwaway tag. Doing
    # exactly that on 2026-09-05 fired the publish workflow for real (run
    # 33959092337); it failed only because TP was unconfigured. Named here so
    # the documented rehearsal stays harmless after that stops being true.
    case "$ref" in
        rehearsal-*|*-rehearsal|test-*|*/rehearsal-*|*/test-*)
            echo "REFUSED: ref ${ref} looks like a rehearsal." >&2; return 1 ;;
    esac

    echo "ok: publishing ${actual} from ${ref}"
    return 0
}

selftest() {
    local failed=0
    check() {  # check <description> <want-rc> <expected> <actual> <ref>
        local desc="$1" want="$2"; shift 2
        local out rc
        out="$(guard "$@" 2>&1)"; rc=$?
        if [ "$rc" -eq "$want" ]; then
            printf '  PASS  %s (rc=%s)\n' "$desc" "$rc"
        else
            printf '  FAIL  %s — wanted rc=%s, got %s: %s\n' "$desc" "$want" "$rc" "$out"
            failed=1
        fi
    }

    # The CONTROL comes first. Every arm below asserts a refusal, and a guard
    # that refuses everything would pass all of them — including a guard broken
    # by a typo. Without this line the suite cannot tell "correctly strict" from
    # "uniformly broken".
    check "a real release publishes"                  0 "0.3.34" "0.3.34" "main"

    check "a version mismatch is refused"             1 "0.3.34" "0.3.35" "main"
    check "publishing an OLDER crate is refused"      1 "0.3.34" "0.3.27" "main"
    check "a prerelease version is refused"           1 "0.3.34-rc.1" "0.3.34-rc.1" "main"
    check "a prerelease build tag is refused"         1 "0.4.0-alpha" "0.4.0-alpha" "main"
    check "a rehearsal ref is refused"                1 "0.3.34" "0.3.34" "rehearsal-wasm-20260905-0951"
    check "a trailing -rehearsal ref is refused"      1 "0.3.34" "0.3.34" "v1-rehearsal"
    check "a test-* ref is refused"                   1 "0.3.34" "0.3.34" "test-publish"
    check "an empty expected version is refused"      1 "" "0.3.34" "main"
    check "an empty actual version is refused"        1 "0.3.34" "" "main"

    # The rehearsal arm must refuse even when everything else is perfect —
    # otherwise it is the version check doing the work and the rehearsal guard
    # has never actually fired.
    check "rehearsal refused on a VALID version"      1 "0.3.34" "0.3.34" "rehearsal-anything"

    check "two empty versions are refused" 1 "" "" "main"
    check "nested rehearsal ref is refused" 1 "0.3.34" "0.3.34" "fix/test-publish"

    echo "selftest: $([ $failed -eq 0 ] && echo 'ALL PASS' || echo 'FAILED')"
    return $failed
}

case "${1:-}" in
    --selftest) selftest; exit $? ;;
    "") echo "usage: $0 <expected-version> <actual-version> <ref-name> | --selftest" >&2; exit 2 ;;
esac

if [ $# -ne 3 ]; then
    echo "usage: $0 <expected-version> <actual-version> <ref-name> | --selftest" >&2
    exit 2
fi
guard "$1" "$2" "$3"
