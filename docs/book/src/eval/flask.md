# Flask (Python)

## flask-001 <span class="eval-medium">medium</span>

**Commit**: [8646edca6f](https://github.com/pallets/flask/commit/8646edca6f47e2cd57464081b3911218d4734f8d)

<details>
<summary>Task prompt</summary>

> Fix the Vary: Cookie header to be set consistently when the session is
accessed, modified, or refreshed. Previously, the header was only set when
the session was modified but not when it was merely accessed or when a
session cookie was being deleted. Move the Vary header logic so it covers
all code paths in save\_session, including cookie deletion and session
refresh without modification.

</details>

| Approach | Tests Pass | Precision | Recall | F1 | Duration |
|----------|:----------:|:---------:|:------:|:--:|:--------:|
| no-bobbin | 0.0% | 100.0% | 33.3% | 50.0% | 48s |
| with-bobbin | 0.0% | 100.0% | 33.3% | 50.0% | 1.5m |

<div class="eval-chart">

![flask-001_duration.svg](./charts/flask-001_duration.svg)

</div>

**Ground truth files**: `CHANGES.rst`, `src/flask/sessions.py`, `tests/test_basic.py`

**Files touched (no-bobbin)**: `src/flask/sessions.py`
**Files touched (with-bobbin)**: `src/flask/sessions.py`

---

## flask-002 <span class="eval-medium">medium</span>

**Commit**: [1232d69860](https://github.com/pallets/flask/commit/1232d698600e11dcb83bb5dc349ca785eae02d2f)

<details>
<summary>Task prompt</summary>

> Refactor the CLI module to inline conditional imports (dotenv, ssl,
importlib.metadata) at their point of use instead of importing them at
module level. This avoids import errors and unnecessary imports when the
optional dependencies are not installed, and moves version-conditional
importlib.metadata handling into the method that actually uses it.

</details>

| Approach | Tests Pass | Precision | Recall | F1 | Duration |
|----------|:----------:|:---------:|:------:|:--:|:--------:|
| no-bobbin | 0.0% | 100.0% | 66.7% | 80.0% | 2.9m |
| with-bobbin | 0.0% | 100.0% | 66.7% | 80.0% | 2.7m |

<div class="eval-chart">

![flask-002_duration.svg](./charts/flask-002_duration.svg)

</div>

**Ground truth files**: `setup.cfg`, `src/flask/cli.py`, `tests/test_cli.py`

**Files touched (no-bobbin)**: `src/flask/cli.py`, `tests/test_cli.py`
**Files touched (with-bobbin)**: `src/flask/cli.py`, `tests/test_cli.py`

---

## flask-003 <span class="eval-medium">medium</span>

**Commit**: [fdab801fbb](https://github.com/pallets/flask/commit/fdab801fbbd9de5adbdb3320ca4a1cb116c892f5)

<details>
<summary>Task prompt</summary>

> Add a redirect method to the Flask app object and a new flask.redirect
helper function that delegates to it. This allows applications to customize
redirect behavior by overriding app.redirect. The flask.redirect function
checks for current\_app and calls app.redirect if available, otherwise
falls back to werkzeug.utils.redirect. Update flask.\_\_init\_\_ to export
redirect from helpers instead of werkzeug.

</details>

| Approach | Tests Pass | Precision | Recall | F1 | Duration |
|----------|:----------:|:---------:|:------:|:--:|:--------:|
| no-bobbin | 0.0% | 100.0% | 60.0% | 75.0% | 2.4m |
| with-bobbin | 0.0% | 100.0% | 60.0% | 75.0% | 2.0m |

<div class="eval-chart">

![flask-003_duration.svg](./charts/flask-003_duration.svg)

</div>

**Ground truth files**: `CHANGES.rst`, `src/flask/__init__.py`, `src/flask/app.py`, `src/flask/helpers.py`, `tests/test_helpers.py`

**Files touched (no-bobbin)**: `src/flask/__init__.py`, `src/flask/app.py`, `src/flask/helpers.py`
**Files touched (with-bobbin)**: `src/flask/__init__.py`, `src/flask/app.py`, `src/flask/helpers.py`

---

## flask-004 <span class="eval-medium">medium</span>

**Commit**: [eb5dd9f5ef](https://github.com/pallets/flask/commit/eb5dd9f5ef255c578cbbe13c1cb4dd11389d5519)

<details>
<summary>Task prompt</summary>

> Add an aborter\_class attribute and aborter instance to the Flask app object,
along with a make\_aborter factory method. Create a new flask.abort helper
function that delegates to app.aborter when current\_app is available,
otherwise falls back to werkzeug.exceptions.abort. This allows applications
to customize abort behavior, including registering custom HTTP error codes.
Update flask.\_\_init\_\_ to export abort from helpers instead of werkzeug.

</details>

| Approach | Tests Pass | Precision | Recall | F1 | Duration |
|----------|:----------:|:---------:|:------:|:--:|:--------:|
| no-bobbin | 0.0% | 100.0% | 60.0% | 75.0% | 2.5m |
| with-bobbin | 0.0% | 100.0% | 60.0% | 75.0% | 3.0m |

<div class="eval-chart">

![flask-004_duration.svg](./charts/flask-004_duration.svg)

</div>

**Ground truth files**: `CHANGES.rst`, `src/flask/__init__.py`, `src/flask/app.py`, `src/flask/helpers.py`, `tests/test_helpers.py`

**Files touched (no-bobbin)**: `src/flask/__init__.py`, `src/flask/app.py`, `src/flask/helpers.py`
**Files touched (with-bobbin)**: `src/flask/__init__.py`, `src/flask/app.py`, `src/flask/helpers.py`

---

## flask-005 <span class="eval-easy">easy</span>

**Commit**: [1e5dd43022](https://github.com/pallets/flask/commit/1e5dd430223369d13ea94ffffe22bca53a98e730)

<details>
<summary>Task prompt</summary>

> Refactor error checking in register\_error\_handler and
\_get\_exc\_class\_and\_code to consolidate validation logic. Move error code
lookup validation into \_get\_exc\_class\_and\_code, replace the assertion for
non-Exception subclasses with a proper ValueError, add a TypeError for
when an exception instance is passed instead of a class, and rewrite
error messages to be more consistent and descriptive.

</details>

| Approach | Tests Pass | Precision | Recall | F1 | Duration |
|----------|:----------:|:---------:|:------:|:--:|:--------:|
| no-bobbin | 0.0% | 100.0% | 75.0% | 85.7% | 2.0m |
| with-bobbin | 0.0% | 100.0% | 50.0% | 66.7% | 2.3m |

<div class="eval-chart">

![flask-005_duration.svg](./charts/flask-005_duration.svg)

</div>

**Ground truth files**: `CHANGES.rst`, `src/flask/scaffold.py`, `tests/test_basic.py`, `tests/test_user_error_handler.py`

**Files touched (no-bobbin)**: `src/flask/scaffold.py`, `tests/test_basic.py`, `tests/test_user_error_handler.py`
**Files touched (with-bobbin)**: `src/flask/scaffold.py`, `tests/test_user_error_handler.py`

---
