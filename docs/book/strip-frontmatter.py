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

def process_book(book):
    for section in book.get('sections', []):
        if 'Chapter' in section:
            process_chapter(section['Chapter'])

if len(sys.argv) > 1 and sys.argv[1] == 'supports':
    sys.exit(0)

data = json.load(sys.stdin)

# mdbook passes [context, book] as a tuple
if isinstance(data, list) and len(data) >= 2:
    context, book = data[0], data[1]
    # Newer mdbook may nest book under a 'book' key
    if 'sections' in book:
        process_book(book)
    elif isinstance(book, dict):
        # Log structure for debugging
        print(f"strip-frontmatter: book keys = {list(book.keys())}", file=sys.stderr)
        for key, val in book.items():
            if isinstance(val, dict) and 'sections' in val:
                process_book(val)
                break
            elif isinstance(val, list):
                # Try treating the list as sections directly
                for item in val:
                    if isinstance(item, dict) and 'Chapter' in item:
                        process_chapter(item['Chapter'])
    json.dump(book, sys.stdout)
elif isinstance(data, dict):
    if 'sections' in data:
        process_book(data)
    json.dump(data, sys.stdout)
else:
    json.dump(data, sys.stdout)
