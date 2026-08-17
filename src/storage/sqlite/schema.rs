//! Schema creation and in-place migrations.
//!
//! Split out of `src/storage/sqlite/mod.rs` (bobbin-aoz). Rust lets one
//! type's inherent methods live in several `impl` blocks across modules, so
//! this is a real split rather than a re-export shim.

use anyhow::Result;

use super::MetadataStore;

impl MetadataStore {
    /// Initialize the database schema
    pub(super) fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            -- Temporal coupling (git co-change relationships)
            CREATE TABLE IF NOT EXISTS coupling (
                file_a TEXT NOT NULL,
                file_b TEXT NOT NULL,
                score REAL,
                co_changes INTEGER,
                last_co_change INTEGER,
                PRIMARY KEY (file_a, file_b)
            );

            -- Cross-repo coupling (bo-oqny). Co-change inferred across repos via
            -- shared bead references; both sides carry their repo because paths
            -- are repo-relative and collide across repos. Additive: leaves the
            -- per-repo `coupling` table above untouched. Canonicalized so
            -- (repo_a, path_a) <= (repo_b, path_b).
            CREATE TABLE IF NOT EXISTS cross_repo_coupling (
                repo_a TEXT NOT NULL,
                path_a TEXT NOT NULL,
                repo_b TEXT NOT NULL,
                path_b TEXT NOT NULL,
                score REAL,
                co_changes INTEGER,
                last_co_change INTEGER,
                PRIMARY KEY (repo_a, path_a, repo_b, path_b)
            );

            -- Global metadata
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT
            );

            -- File hash tracking for incremental indexing. Keyed by
            -- (repo, file_path): the bare path is NOT unique across a
            -- multi-repo index (19 repos each have a README.md), and a
            -- path-only key let the first writer own the row while every
            -- other repo's copy was judged against the wrong hash and
            -- silently skipped forever.
            CREATE TABLE IF NOT EXISTS file_hashes (
                repo TEXT NOT NULL,
                file_path TEXT NOT NULL,
                hash TEXT NOT NULL,
                PRIMARY KEY (repo, file_path)
            );

            -- Bead → bundle → commit workflow telemetry (GH#9, Layer 1: logging).
            -- Each row records that a bead was linked to a commit / changeset, so
            -- later layers can mine which files matter for which kinds of work.
            CREATE TABLE IF NOT EXISTS bead_lineage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                bead_id TEXT NOT NULL,
                bead_type TEXT,
                commit_sha TEXT,
                bundle_slugs TEXT,
                touched_files TEXT,
                action_type TEXT
            );

            -- Bug causality (GH#9 telemetry Phase 0, bo-s1kb). The supervised
            -- signal for "risky change": reconstructs which prior commit most
            -- likely introduced the bug a later bead fixed, per file. One row per
            -- (bug, culprit_sha, file); UNIQUE makes the reconstruction job
            -- idempotent so periodic re-runs upsert rather than duplicate.
            CREATE TABLE IF NOT EXISTS bug_causality (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                bug_id TEXT NOT NULL,
                culprit_sha TEXT,
                culprit_bead_id TEXT,
                file TEXT,
                confidence REAL,
                UNIQUE(bug_id, culprit_sha, file)
            );

            -- Indexes
            CREATE INDEX IF NOT EXISTS idx_coupling_score ON coupling(score DESC);
            CREATE INDEX IF NOT EXISTS idx_xrepo_coupling_a ON cross_repo_coupling(repo_a, path_a);
            CREATE INDEX IF NOT EXISTS idx_xrepo_coupling_b ON cross_repo_coupling(repo_b, path_b);
            CREATE INDEX IF NOT EXISTS idx_bead_lineage_bead ON bead_lineage(bead_id);
            CREATE INDEX IF NOT EXISTS idx_bead_lineage_commit ON bead_lineage(commit_sha);
            CREATE INDEX IF NOT EXISTS idx_bug_causality_bug ON bug_causality(bug_id);
        "#,
        )?;

        self.migrate_bead_lineage()?;
        self.migrate_repo_scoped_state()?;

        Ok(())
    }

    /// One-time migration to repo-scoped incremental state.
    ///
    /// The pre-migration `file_hashes` rows are keyed by bare path, so in a
    /// multi-repo index each row belongs to whichever repo wrote it first and
    /// there is no way to attribute it after the fact — re-keying would just
    /// bless the collided data. The rows are dropped instead; the next index
    /// run per repo re-hashes (and re-embeds changed files) from scratch, which
    /// is the one-time rebuild this fix requires.
    ///
    /// The legacy GLOBAL watermark keys (`last_indexed_commit`,
    /// `last_coupling_commit`, `coupling_depth`) are deleted for the same
    /// reason: one SHA shared by every repo means each repo was asked for
    /// "commits since" a commit it does not contain. Their per-repo
    /// replacements (`last_indexed_commit:<repo>`, …) are written fresh by the
    /// next index run; a missing watermark falls back to the full-depth scan a
    /// fresh index already does.
    pub(super) fn migrate_repo_scoped_state(&self) -> Result<()> {
        // Old schema is detected by the absence of the `repo` column. The DROP
        // runs at most once: the recreate below uses the new schema.
        let mut has_repo = false;
        let mut has_table = false;
        {
            let mut stmt = self.conn.prepare("PRAGMA table_info(file_hashes)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            for r in rows {
                has_table = true;
                if r? == "repo" {
                    has_repo = true;
                }
            }
        }
        if has_table && !has_repo {
            self.conn.execute_batch(
                r#"
                DROP TABLE file_hashes;
                CREATE TABLE file_hashes (
                    repo TEXT NOT NULL,
                    file_path TEXT NOT NULL,
                    hash TEXT NOT NULL,
                    PRIMARY KEY (repo, file_path)
                );
                "#,
            )?;
        }
        // Idempotent; a no-op once the keys are gone.
        self.conn.execute(
            "DELETE FROM meta WHERE key IN ('last_indexed_commit', 'last_coupling_commit', 'coupling_depth')",
            [],
        )?;
        Ok(())
    }

    /// Idempotently add columns introduced after the initial bead_lineage schema
    /// (telemetry Phase 0, bo-xrsy). SQLite has no `ADD COLUMN IF NOT EXISTS`, so
    /// we inspect `PRAGMA table_info` and only ALTER for genuinely-missing
    /// columns. Errors other than the additions themselves propagate — we do not
    /// blind-try-and-ignore. `bundle_slugs` already exists in the base schema and
    /// is intentionally absent here (this migration only adds new columns).
    pub(super) fn migrate_bead_lineage(&self) -> Result<()> {
        let mut existing = std::collections::HashSet::new();
        {
            let mut stmt = self.conn.prepare("PRAGMA table_info(bead_lineage)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            for r in rows {
                existing.insert(r?);
            }
        }
        // (column, SQL type) — additive only.
        let additions = [
            ("feature_id", "TEXT"),
            ("lines_added", "INTEGER"),
            ("lines_deleted", "INTEGER"),
            ("touched_symbols", "TEXT"),
        ];
        for (col, ty) in additions {
            if !existing.contains(col) {
                self.conn.execute(
                    &format!("ALTER TABLE bead_lineage ADD COLUMN {} {}", col, ty),
                    [],
                )?;
            }
        }
        Ok(())
    }
}
