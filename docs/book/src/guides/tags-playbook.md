# Tags Deployment Playbook

A practical guide to deploying and iterating on bobbin's tag taxonomy. Based on
real deployment experience across 14 repos with 80,000+ tag assignments.

## The Iteration Loop

Tag tuning is empirical. You can't predict which docs will pollute results until
you test real queries. The loop is:

1. **Run test queries** — cover different query types (how-to, architecture, debugging)
2. **Identify noise** — which irrelevant docs keep surfacing?
3. **Add rules + effects** — tag the noisy content, configure demotion
4. **Deploy + reindex** — restart server, reindex affected repos
5. **Verify** — re-run the same test queries
6. **Repeat**

Each iteration takes 10-15 minutes. Plan for 3-5 iterations to get a good baseline.

## Starting From Scratch

### Step 1: Assess your index

```bash
# What repos are indexed?
curl -s http://search.svc/repos | jq '.repos[].name'

# How many chunks per repo?
curl -s http://search.svc/repos | jq '.repos[] | "\(.name): \(.chunk_count) chunks"'

# What tags exist already? (auto-tags are applied by default)
curl -s http://search.svc/tags?repo=<name> | jq '.tags[] | "\(.count)\t\(.tag)"'
```

### Step 2: Run a query battery

Test 8-10 queries that represent real agent workflows:

```bash
queries=(
  "how do I restart a service"
  "ansible playbook for deploying"
  "prometheus alert rules"
  "message router architecture"
  "backup and restore procedures"
)

for q in "${queries[@]}"; do
  echo "=== $q ==="
  curl -s "http://search.svc/context?q=$(python3 -c \
    "import urllib.parse; print(urllib.parse.quote('$q'))")&repo=aegis&limit=3" \
    | jq '.files[] | "\(.score)\t\(.path | split("/") | last)"'
  echo
done
```

### Step 3: Identify patterns

Common noise sources by priority:

| Pattern | Why it's noisy | Typical effect |
|---------|---------------|----------------|
| Changelogs | Match every keyword ever mentioned | `boost = -0.6` |
| Test files | Match function names but aren't useful as context | `boost = -0.4` |
| Historical records | Pensieve/HLA/audit logs match queries semantically | `boost = -0.4` |
| Agent instruction files | Already in the agent's system prompt | `boost = -0.3` |
| Upstream digests | Release notes match keyword queries | `boost = -0.5` |
| Eval artifacts | Task definitions match code queries | `boost = -0.5` |

### Step 4: Write your initial tags.toml

Start with the high-impact rules:

```toml
# --- Always exclude ---
[effects."auto:init"]
exclude = true

[effects."type:node-modules"]
exclude = true

# --- Heavy demotion ---
[effects."type:changelog"]
boost = -0.6

[effects."auto:test"]
boost = -0.4

# --- Light demotion ---
[effects."auto:docs"]
boost = -0.1

[effects."role:claude-md"]
boost = -0.3

# --- Boost valuable content ---
[effects."type:runbook"]
boost = 0.25

[effects."type:design"]
boost = 0.2

# --- Rules ---
[[rules]]
pattern = "**/CHANGELOG.md"
tags = ["type:changelog"]

[[rules]]
pattern = "**/node_modules/**"
tags = ["type:node-modules"]
```

## Advanced Patterns

### Virtual path tagging

When content from other systems is indexed into a repo with virtual paths
(e.g., `pensieve:2026/03/file.md` or `hla:2026/03/file.md`), standard
glob patterns like `**/*.md` won't match because the prefix contains a colon.

Use the virtual prefix directly:

```toml
# Tag pensieve records embedded in aegis index
[[rules]]
pattern = "pensieve:*"
tags = ["type:record"]
repo = "aegis"

# Tag HLA records embedded in aegis index
[[rules]]
pattern = "hla:*"
tags = ["type:hla-record"]
repo = "aegis"
```

This works because bobbin uses `glob::Pattern::matches()` with default options,
where `*` matches `/` (unlike `matches_path()` which treats `/` as a separator).

### Domain tagging

Group related docs under domain tags for scoped boosting:

```toml
# Tag comms-related docs
[[rules]]
pattern = "**/docs/designs/comms-*.md"
tags = ["domain:comms"]
repo = "aegis"

[[rules]]
pattern = "**/deploy/aegis-irc/**"
tags = ["domain:comms"]
repo = "aegis"

# Boost comms docs for comms-focused agents
[[effects_scoped]]
tag = "domain:comms"
role = "*/polecats/*"
boost = 0.15
```

### Catch-all directory rules

When a directory contains many files of the same type, use a single broad rule
instead of multiple specific patterns:

```toml
# Instead of 7 specific probe subdirectory rules:
[[rules]]
pattern = "**/docs/probes/**"
tags = ["type:probe"]
repo = "aegis"

# Instead of 2 specific eval directory rules:
[[rules]]
pattern = "**/eval/**"
tags = ["type:eval"]
repo = "bobbin"
```

### Role-scoped effects

Demote globally, boost for specific roles:

```toml
# Globally demote lifecycle docs
[effects."domain:lifecycle"]
boost = -0.3

# But boost for witness (manages polecat lifecycle)
[[effects_scoped]]
tag = "domain:lifecycle"
role = "*/witness"
boost = 0.2
```

## Deployment Checklist

After editing `tags.toml`:

1. **Validate syntax**: `python3 -c "import tomllib; tomllib.load(open('tags.toml','rb'))"`
2. **Deploy to server**: `scp tags.toml user@server:/path/.bobbin/tags.toml`
3. **Restart bobbin**: `sudo systemctl restart bobbin` (tags loaded at startup)
4. **Reindex affected repos**:
   ```bash
   bobbin index /data --repo <name> --source /path/to/repo --force
   ```
5. **Verify via `/context` endpoint** (not `/search` — tag effects only apply to context)
6. **Check tag counts**: `curl -s http://search.svc/tags?repo=<name> | jq`

## Key Gotchas

- **`/search` vs `/context`**: Tag effects (boost/demote/exclude) only apply via
  the `/context` endpoint. The `/search` endpoint returns raw LanceDB scores.
  Always test with `/context`.

- **Server restart required**: Tags config is loaded at server startup. Editing
  the file without restarting has no effect.

- **Reindex required for new rules**: Tag rules are applied during indexing.
  New rules won't tag existing chunks until you reindex.

- **Rule ordering**: When multiple rules match a file, all their tags are applied
  (union). More specific rules don't override broader ones — they add to them.

- **Repo scope**: Rules with `repo = "name"` only apply when indexing that specific
  repo. Omit `repo` for rules that should apply everywhere.

## Metrics to Track

After each iteration, check:

```bash
# Tag coverage (from reindex output)
# ✓ Indexed 471 files (8378 chunks)
#   Tags: 46347 tagged, 22941 untagged chunks

# Tag distribution
curl -s http://search.svc/tags?repo=aegis | jq '.tags | length'
# → Unique tag count

# Total assignments
curl -s http://search.svc/tags?repo=aegis | jq '[.tags[].count] | add'
# → Total tag assignments
```

Target: 60-70% tagged chunks is healthy. 100% is unnecessary — not every chunk
needs tags. Focus on tagging content that causes noise or deserves boosting.
