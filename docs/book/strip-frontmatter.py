#!/usr/bin/env python3
"""mdbook preprocessor that strips YAML frontmatter from chapters."""
import json
import sys
import re

FRONTMATTER_RE = re.compile(r'\A---\n.*?\n---\n', re.DOTALL)

def strip_frontmatter(content):
    return FRONTMATTER_RE.sub('', content, count=1)

def process_chapter(chapter):
    chapter['content'] = strip_frontmatter(chapter['content'])
    for sub in chapter.get('sub_items', []):
        if 'Chapter' in sub:
            process_chapter(sub['Chapter'])

if len(sys.argv) > 1 and sys.argv[1] == 'supports':
    sys.exit(0)

data = json.load(sys.stdin)
for section in data[1]['sections']:
    if 'Chapter' in section:
        process_chapter(section['Chapter'])

json.dump(data[1], sys.stdout)
