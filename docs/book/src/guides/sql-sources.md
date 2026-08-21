---
title: SQL Sources
description: Index rows from SQL databases as searchable chunks
tags: [guides, sql, sources]
status: draft
category: guides
related: [guides/archive.md, config/reference.md]
---

# SQL Sources

Bobbin can index rows from any MySQL-protocol database (MySQL, MariaDB,
Dolt) as searchable chunks. Each configured source runs a query at index
time; every row becomes one chunk with a stable identity, so re-index runs
only re-embed rows whose content changed, and rows that disappear from the
query results are removed from the index.

## Configuration

```toml
[sql]
enabled = true

[[sql.sources]]
name = "tickets"
url_env = "BOBBIN_SQL_TICKETS_URL"
query = "SELECT id, title, body, status FROM tickets"
id_column = "id"
text_columns = ["title", "body"]
tag_columns = ["status"]
```

| Field | Required | Description |
|-------|----------|-------------|
| `name` | yes | Label for the source. Rows index under repo key `sql-{name}` with chunk paths `sql:{name}:{id}`. |
| `url_env` | yes | Environment variable holding the `mysql://user:pass@host:port/db` connection URL. Credentials never live in config. |
| `query` | yes | Query producing the rows to index. Must include `id_column`. |
| `id_column` | yes | Column holding each row's stable primary key. |
| `text_columns` | no | Columns embedded as searchable content. Empty means all columns. |
| `tag_columns` | no | Columns rendered as `column:value` tags for search filtering. |

## How rows become chunks

Each row is assembled into a small document — `{name} #{id}` followed by
`column: value` lines for the text columns — and embedded like any other
chunk. Tags from `tag_columns` work with the same `tag`/`exclude_tag`
search filters as file chunks.

Incremental behavior follows the beads pattern: a content hash per row is
stored after a successful insert, unchanged rows are skipped on the next
run, and vanished rows are swept. `bobbin index --force` re-embeds
everything.
