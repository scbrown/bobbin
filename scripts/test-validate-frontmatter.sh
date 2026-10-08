#!/usr/bin/env bash
# arming: ci Docs workflow; isolates metadata boundaries from page-body examples.
set -euo pipefail
validator="$(cd "$(dirname "$0")" && pwd)/validate-frontmatter.sh"
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/docs"
cat > "$fixture/header" <<'HEADER'
---
title: Boundary control
description: Fixture for the first metadata block
status: published
tags: [test]
category: guide
---
HEADER
# Appending body text, including complete YAML examples, cannot change the
# metadata verdict. This discriminates the old sed range's extra blocks.
for body in plain fields yaml repeated; do
  cp "$fixture/header" "$fixture/docs/page.md"
  case "$body" in
    plain) printf '# Body\n' >> "$fixture/docs/page.md" ;;
    fields) printf 'status: invalid\nsource_files: [missing.rs]\n' >> "$fixture/docs/page.md" ;;
    yaml) printf '\n```yaml\n---\nstatus: invalid\nsource_files: [missing.rs]\n---\n```\n' >> "$fixture/docs/page.md" ;;
    repeated) printf '\n---\nstatus: draft\n---\n---\nstatus: invalid\n---\n' >> "$fixture/docs/page.md" ;;
  esac
  bash "$validator" "$fixture/docs" > "$fixture/log" 2>&1
  echo "PASS body=$body leaves metadata verdict unchanged"
done
sed 's/status: published/status: invalid/' "$fixture/header" > "$fixture/docs/page.md"
if bash "$validator" "$fixture/docs" > "$fixture/log" 2>&1; then
  echo 'FAIL invalid header accepted' >&2; exit 1
fi
/usr/bin/grep -q 'Invalid status' "$fixture/log"
printf '# No metadata\n' > "$fixture/docs/page.md"
if bash "$validator" "$fixture/docs" > "$fixture/log" 2>&1; then
  echo 'FAIL missing header accepted' >&2; exit 1
fi
/usr/bin/grep -q 'Missing frontmatter' "$fixture/log"
cp "$fixture/header" "$fixture/docs/page.md"
sed -i '/category: guide/a source_files: [missing-file.rs]' "$fixture/docs/page.md"
if bash "$validator" "$fixture/docs" > "$fixture/log" 2>&1; then
  echo 'FAIL missing source accepted' >&2; exit 1
fi
/usr/bin/grep -q 'Source file not found' "$fixture/log"
head -n -1 "$fixture/header" > "$fixture/docs/page.md"
if bash "$validator" "$fixture/docs" > "$fixture/log" 2>&1; then
  echo 'FAIL unterminated metadata accepted' >&2; exit 1
fi
/usr/bin/grep -q 'Unterminated frontmatter' "$fixture/log"
echo 'PASS invalid status, absent metadata, missing source and unterminated header refuse' 
