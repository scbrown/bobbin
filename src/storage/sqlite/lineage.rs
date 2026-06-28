use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::MetadataStore;

impl MetadataStore {
    /// Record a bead→commit lineage entry (GH#9 Layer 1 logging).
    ///
    /// Returns the row id of the inserted record. `touched_files` is stored as
    /// a JSON array so later layers can aggregate over changesets.
    pub fn record_bead_lineage(&self, rec: &NewBeadLineage) -> Result<i64> {
        let touched_files_json = serde_json::to_string(&rec.touched_files)
            .unwrap_or_else(|_| "[]".to_string());
        let touched_symbols_json = serde_json::to_string(&rec.touched_symbols)
            .unwrap_or_else(|_| "[]".to_string());
        self.conn.execute(
            r#"INSERT INTO bead_lineage
                   (bead_id, bead_type, commit_sha, bundle_slugs, touched_files, action_type,
                    feature_id, lines_added, lines_deleted, touched_symbols)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#,
            rusqlite::params![
                rec.bead_id,
                rec.bead_type,
                rec.commit_sha,
                rec.bundle_slugs,
                touched_files_json,
                rec.action_type,
                rec.feature_id,
                rec.lines_added,
                rec.lines_deleted,
                touched_symbols_json,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// List bead lineage records, optionally filtered by bead id and/or commit.
    /// Most recent first.
    pub fn list_bead_lineage(
        &self,
        bead_id: Option<&str>,
        commit_sha: Option<&str>,
        limit: usize,
    ) -> Result<Vec<BeadLineageRecord>> {
        let mut sql = String::from(
            "SELECT id, created_at, bead_id, bead_type, commit_sha, bundle_slugs, touched_files, action_type,
                    feature_id, lines_added, lines_deleted, touched_symbols
             FROM bead_lineage",
        );
        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(b) = bead_id {
            conditions.push(format!("bead_id = ?{}", params.len() + 1));
            params.push(Box::new(b.to_string()));
        }
        if let Some(c) = commit_sha {
            conditions.push(format!("commit_sha = ?{}", params.len() + 1));
            params.push(Box::new(c.to_string()));
        }
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        sql.push_str(&format!(" ORDER BY id DESC LIMIT ?{}", params.len() + 1));
        params.push(Box::new(limit as i64));

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                let touched_json: Option<String> = row.get(6)?;
                let touched_files = touched_json
                    .and_then(|j| serde_json::from_str::<Vec<String>>(&j).ok())
                    .unwrap_or_default();
                let symbols_json: Option<String> = row.get(11)?;
                let touched_symbols = symbols_json
                    .and_then(|j| serde_json::from_str::<Vec<TouchedSymbol>>(&j).ok())
                    .unwrap_or_default();
                Ok(BeadLineageRecord {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    bead_id: row.get(2)?,
                    bead_type: row.get(3)?,
                    commit_sha: row.get(4)?,
                    bundle_slugs: row.get(5)?,
                    touched_files,
                    action_type: row.get(7)?,
                    feature_id: row.get(8)?,
                    lines_added: row.get(9)?,
                    lines_deleted: row.get(10)?,
                    touched_symbols,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Distinct (bead_id, bead_type) pairs present in bead_lineage. `bead_type`
    /// is the most-recent non-null type recorded for that bead (may be None).
    /// Used by the causality job to discover candidate bug beads (bo-s1kb).
    pub fn distinct_lineage_bead_ids(&self) -> Result<Vec<(String, Option<String>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT bead_id, MAX(bead_type) FROM bead_lineage GROUP BY bead_id ORDER BY bead_id",
        )?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Prior commit lineage rows that touched any of `files` strictly before
    /// `before` (ISO timestamp). Returns one (bead_id, commit_sha, file,
    /// created_at) tuple per matching (row, file), most-recent first. Used by
    /// bug-causality reconstruction (bo-s1kb) to find candidate culprit commits.
    /// `files` empty → empty result. Uses json_each over `touched_files`.
    pub fn prior_lineage_touching_files(
        &self,
        files: &[String],
        before: &str,
    ) -> Result<Vec<PriorTouch>> {
        if files.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = (0..files.len())
            .map(|i| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let before_idx = files.len() + 1;
        let sql = format!(
            "SELECT bl.bead_id, bl.commit_sha, je.value, bl.created_at
             FROM bead_lineage bl, json_each(bl.touched_files) je
             WHERE bl.commit_sha IS NOT NULL
               AND bl.created_at < ?{before_idx}
               AND je.value IN ({placeholders})
             ORDER BY bl.created_at DESC, bl.id DESC"
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        for f in files {
            params.push(Box::new(f.clone()));
        }
        params.push(Box::new(before.to_string()));
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(PriorTouch {
                    bead_id: row.get(0)?,
                    commit_sha: row.get(1)?,
                    file: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Upsert a bug-causality row (bo-s1kb). Idempotent on (bug_id, culprit_sha,
    /// file): a re-run refreshes confidence / culprit_bead_id rather than
    /// duplicating. Returns the row id.
    pub fn record_bug_causality(&self, rec: &NewBugCausality) -> Result<i64> {
        self.conn.execute(
            r#"INSERT INTO bug_causality (bug_id, culprit_sha, culprit_bead_id, file, confidence)
               VALUES (?1, ?2, ?3, ?4, ?5)
               ON CONFLICT(bug_id, culprit_sha, file) DO UPDATE SET
                   culprit_bead_id = excluded.culprit_bead_id,
                   confidence = excluded.confidence,
                   created_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')"#,
            rusqlite::params![
                rec.bug_id,
                rec.culprit_sha,
                rec.culprit_bead_id,
                rec.file,
                rec.confidence,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// List bug-causality rows, optionally filtered by bug id. Highest
    /// confidence first.
    pub fn list_bug_causality(
        &self,
        bug_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<BugCausalityRecord>> {
        let mut sql = String::from(
            "SELECT id, created_at, bug_id, culprit_sha, culprit_bead_id, file, confidence
             FROM bug_causality",
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(b) = bug_id {
            sql.push_str(" WHERE bug_id = ?1");
            params.push(Box::new(b.to_string()));
        }
        sql.push_str(&format!(
            " ORDER BY confidence DESC, id DESC LIMIT ?{}",
            params.len() + 1
        ));
        params.push(Box::new(limit as i64));
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(BugCausalityRecord {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    bug_id: row.get(2)?,
                    culprit_sha: row.get(3)?,
                    culprit_bead_id: row.get(4)?,
                    file: row.get(5)?,
                    confidence: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

/// One (commit, file) pair from a prior lineage row that touched a file of
/// interest (bug-causality reconstruction, bo-s1kb).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorTouch {
    pub bead_id: String,
    pub commit_sha: Option<String>,
    pub file: String,
    pub created_at: String,
}

/// Input for upserting a bug-causality row (bo-s1kb).
#[derive(Debug, Clone, Default)]
pub struct NewBugCausality {
    pub bug_id: String,
    pub culprit_sha: Option<String>,
    pub culprit_bead_id: Option<String>,
    pub file: Option<String>,
    pub confidence: Option<f64>,
}

/// A stored bug-causality row (bo-s1kb).
#[derive(Debug, Clone)]
pub struct BugCausalityRecord {
    pub id: i64,
    pub created_at: String,
    pub bug_id: String,
    pub culprit_sha: Option<String>,
    pub culprit_bead_id: Option<String>,
    pub file: Option<String>,
    pub confidence: Option<f64>,
}

/// A symbol touched by a commit's changeset (telemetry Phase 0, bo-xrsy).
/// Carries file attribution so the reconcile/predict loops (bo-mu4m/bo-6i55) can
/// map a bead to the named entities it changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TouchedSymbol {
    pub file: String,
    pub symbol: String,
    /// Chunk kind (function | method | struct | ...), from the parser.
    pub kind: String,
}

/// Input for recording a bead lineage entry. `id`/`created_at` are assigned by
/// the database.
#[derive(Debug, Clone, Default)]
pub struct NewBeadLineage {
    pub bead_id: String,
    pub bead_type: Option<String>,
    pub commit_sha: Option<String>,
    pub bundle_slugs: Option<String>,
    pub touched_files: Vec<String>,
    pub action_type: Option<String>,
    /// Feature ancestor resolved via bd dep-walk (edge E1 'implements').
    pub feature_id: Option<String>,
    /// Aggregate lines added across the changeset (numstat).
    pub lines_added: Option<i64>,
    /// Aggregate lines deleted across the changeset (numstat).
    pub lines_deleted: Option<i64>,
    /// Named symbols touched by the changeset (best-effort parse).
    pub touched_symbols: Vec<TouchedSymbol>,
}

/// A stored bead→commit lineage record.
#[derive(Debug, Clone)]
pub struct BeadLineageRecord {
    pub id: i64,
    pub created_at: String,
    pub bead_id: String,
    pub bead_type: Option<String>,
    pub commit_sha: Option<String>,
    pub bundle_slugs: Option<String>,
    pub touched_files: Vec<String>,
    pub action_type: Option<String>,
    pub feature_id: Option<String>,
    pub lines_added: Option<i64>,
    pub lines_deleted: Option<i64>,
    pub touched_symbols: Vec<TouchedSymbol>,
}
