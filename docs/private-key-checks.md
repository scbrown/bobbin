# Private-key checks

The pre-commit configuration includes the pinned `detect-private-key` hook.
Install pre-commit for development and run `pre-commit install` in your clone
if you want the check before local commits. CI runs it regardless of local hook
installation.

Run `python3 scripts/check-private-keys.py` to exercise a benign control and
three synthetic private-key refusals, then scan every tracked file. The tests
use a temporary repository and never generate, stage or commit a real key.
`--selftest` runs only the controls. A failure exits nonzero and reports filenames,
not key contents.

This check recognizes private-key format markers, including PEM and OpenSSH.
It is not a general credential scanner: arbitrary API tokens, raw binary DER,
encrypted archives and repository history are outside its coverage. Keep private
signing material outside tracked files; an ignored path is useful but does not
replace this check when a key arrives under a different filename.
