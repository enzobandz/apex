//! SQLite-backed audit log of every system change, plus benchmark history.
//!
//! The ledger is written *before* a change is attempted (status `pending`) so that
//! if the app crashes mid-operation, `Executor::recover` can determine what happened.

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;
use uuid::Uuid;

use crate::actions::{Action, Setting, SettingValue};

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
}

pub type LedgerResult<T> = Result<T, LedgerError>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ChangeStatus {
    /// Recorded, write not yet confirmed. Only seen after a crash.
    Pending,
    Applied,
    /// The target value was already in place; nothing written.
    Skipped,
    Failed,
    /// Applied, then automatically restored because a later step in the batch failed.
    RolledBack,
    /// Applied, then undone by the user.
    Reverted,
    /// Crash recovery found a value that is neither the original nor the target.
    NeedsAttention,
}

impl ChangeStatus {
    fn as_str(self) -> &'static str {
        match self {
            ChangeStatus::Pending => "pending",
            ChangeStatus::Applied => "applied",
            ChangeStatus::Skipped => "skipped",
            ChangeStatus::Failed => "failed",
            ChangeStatus::RolledBack => "rolledBack",
            ChangeStatus::Reverted => "reverted",
            ChangeStatus::NeedsAttention => "needsAttention",
        }
    }
    fn parse(s: &str) -> ChangeStatus {
        match s {
            "pending" => ChangeStatus::Pending,
            "applied" => ChangeStatus::Applied,
            "skipped" => ChangeStatus::Skipped,
            "failed" => ChangeStatus::Failed,
            "rolledBack" => ChangeStatus::RolledBack,
            "reverted" => ChangeStatus::Reverted,
            _ => ChangeStatus::NeedsAttention,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BatchStatus {
    InProgress,
    Applied,
    PartiallyApplied,
    RolledBack,
    Reverted,
    Failed,
}

impl BatchStatus {
    fn as_str(self) -> &'static str {
        match self {
            BatchStatus::InProgress => "inProgress",
            BatchStatus::Applied => "applied",
            BatchStatus::PartiallyApplied => "partiallyApplied",
            BatchStatus::RolledBack => "rolledBack",
            BatchStatus::Reverted => "reverted",
            BatchStatus::Failed => "failed",
        }
    }
    fn parse(s: &str) -> BatchStatus {
        match s {
            "inProgress" => BatchStatus::InProgress,
            "applied" => BatchStatus::Applied,
            "partiallyApplied" => BatchStatus::PartiallyApplied,
            "rolledBack" => BatchStatus::RolledBack,
            "reverted" => BatchStatus::Reverted,
            _ => BatchStatus::Failed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRecord {
    pub id: String,
    pub batch_id: String,
    pub seq: i64,
    pub created_at: DateTime<Utc>,
    pub action: Action,
    pub setting: Option<Setting>,
    pub before: Option<SettingValue>,
    pub after: Option<SettingValue>,
    pub status: ChangeStatus,
    pub reversible: bool,
    pub error: Option<String>,
    pub detail: Option<String>,
    pub reverted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRecord {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub label: String,
    pub status: BatchStatus,
    pub changes: Vec<ChangeRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkRecord {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub kind: String,
    pub label: String,
    pub unit: String,
    pub higher_is_better: bool,
    pub samples: Vec<f64>,
    pub context: serde_json::Value,
}

pub struct Ledger {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS batches (
    id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    label TEXT NOT NULL,
    status TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS changes (
    id TEXT PRIMARY KEY,
    batch_id TEXT NOT NULL REFERENCES batches(id),
    seq INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    action_json TEXT NOT NULL,
    setting_json TEXT,
    before_json TEXT,
    after_json TEXT,
    status TEXT NOT NULL,
    reversible INTEGER NOT NULL,
    error TEXT,
    detail TEXT,
    reverted_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_changes_batch ON changes(batch_id, seq);
CREATE TABLE IF NOT EXISTS benchmarks (
    id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    kind TEXT NOT NULL,
    label TEXT NOT NULL,
    unit TEXT NOT NULL,
    higher_is_better INTEGER NOT NULL,
    samples_json TEXT NOT NULL,
    context_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS kv (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
PRAGMA user_version = 1;
"#;

fn opt_json<T: Serialize>(v: &Option<T>) -> LedgerResult<Option<String>> {
    Ok(match v {
        Some(x) => Some(serde_json::to_string(x)?),
        None => None,
    })
}

fn parse_opt<T: for<'de> Deserialize<'de>>(s: Option<String>) -> LedgerResult<Option<T>> {
    Ok(match s {
        Some(x) => Some(serde_json::from_str(&x)?),
        None => None,
    })
}

fn parse_time(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

impl Ledger {
    pub fn open(path: &Path) -> LedgerResult<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> LedgerResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn create_batch(&self, label: &str) -> LedgerResult<String> {
        let id = Uuid::new_v4().to_string();
        self.conn.lock().execute(
            "INSERT INTO batches (id, created_at, label, status) VALUES (?1, ?2, ?3, ?4)",
            params![
                id,
                Utc::now().to_rfc3339(),
                label,
                BatchStatus::InProgress.as_str()
            ],
        )?;
        Ok(id)
    }

    pub fn set_batch_status(&self, batch_id: &str, status: BatchStatus) -> LedgerResult<()> {
        self.conn.lock().execute(
            "UPDATE batches SET status = ?1 WHERE id = ?2",
            params![status.as_str(), batch_id],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_change(
        &self,
        batch_id: &str,
        seq: i64,
        action: &Action,
        setting: Option<&Setting>,
        before: Option<&SettingValue>,
        after: Option<&SettingValue>,
        status: ChangeStatus,
    ) -> LedgerResult<String> {
        let id = Uuid::new_v4().to_string();
        self.conn.lock().execute(
            "INSERT INTO changes (id, batch_id, seq, created_at, action_json, setting_json, before_json, after_json, status, reversible)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                batch_id,
                seq,
                Utc::now().to_rfc3339(),
                serde_json::to_string(action)?,
                opt_json(&setting.cloned())?,
                opt_json(&before.cloned())?,
                opt_json(&after.cloned())?,
                status.as_str(),
                action.is_reversible() as i64,
            ],
        )?;
        Ok(id)
    }

    pub fn update_change(
        &self,
        change_id: &str,
        status: ChangeStatus,
        error: Option<&str>,
        detail: Option<&str>,
    ) -> LedgerResult<()> {
        let reverted_at = matches!(status, ChangeStatus::Reverted | ChangeStatus::RolledBack)
            .then(|| Utc::now().to_rfc3339());
        let n = self.conn.lock().execute(
            "UPDATE changes SET status = ?1,
                 error = COALESCE(?2, error),
                 detail = COALESCE(?3, detail),
                 reverted_at = COALESCE(?4, reverted_at)
             WHERE id = ?5",
            params![status.as_str(), error, detail, reverted_at, change_id],
        )?;
        if n == 0 {
            return Err(LedgerError::NotFound(change_id.into()));
        }
        Ok(())
    }

    fn row_to_change(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawChange> {
        Ok(RawChange {
            id: row.get(0)?,
            batch_id: row.get(1)?,
            seq: row.get(2)?,
            created_at: row.get(3)?,
            action_json: row.get(4)?,
            setting_json: row.get(5)?,
            before_json: row.get(6)?,
            after_json: row.get(7)?,
            status: row.get(8)?,
            reversible: row.get(9)?,
            error: row.get(10)?,
            detail: row.get(11)?,
            reverted_at: row.get(12)?,
        })
    }

    const CHANGE_COLS: &'static str = "id, batch_id, seq, created_at, action_json, setting_json, before_json, after_json, status, reversible, error, detail, reverted_at";

    pub fn get_change(&self, change_id: &str) -> LedgerResult<ChangeRecord> {
        let raw = self
            .conn
            .lock()
            .query_row(
                &format!("SELECT {} FROM changes WHERE id = ?1", Self::CHANGE_COLS),
                params![change_id],
                Self::row_to_change,
            )
            .optional()?
            .ok_or_else(|| LedgerError::NotFound(change_id.into()))?;
        raw.into_record()
    }

    pub fn changes_for_batch(&self, batch_id: &str) -> LedgerResult<Vec<ChangeRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM changes WHERE batch_id = ?1 ORDER BY seq",
            Self::CHANGE_COLS
        ))?;
        let raws = stmt
            .query_map(params![batch_id], Self::row_to_change)?
            .collect::<Result<Vec<_>, _>>()?;
        raws.into_iter().map(RawChange::into_record).collect()
    }

    pub fn changes_with_status(&self, status: ChangeStatus) -> LedgerResult<Vec<ChangeRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM changes WHERE status = ?1 ORDER BY created_at",
            Self::CHANGE_COLS
        ))?;
        let raws = stmt
            .query_map(params![status.as_str()], Self::row_to_change)?
            .collect::<Result<Vec<_>, _>>()?;
        raws.into_iter().map(RawChange::into_record).collect()
    }

    pub fn get_batch(&self, batch_id: &str) -> LedgerResult<BatchRecord> {
        let (id, created_at, label, status): (String, String, String, String) = self
            .conn
            .lock()
            .query_row(
                "SELECT id, created_at, label, status FROM batches WHERE id = ?1",
                params![batch_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
            .ok_or_else(|| LedgerError::NotFound(batch_id.into()))?;
        Ok(BatchRecord {
            changes: self.changes_for_batch(&id)?,
            id,
            created_at: parse_time(&created_at),
            label,
            status: BatchStatus::parse(&status),
        })
    }

    /// Most recent batches first.
    pub fn history(&self, limit: usize) -> LedgerResult<Vec<BatchRecord>> {
        let ids: Vec<String> = {
            let conn = self.conn.lock();
            let mut stmt =
                conn.prepare("SELECT id FROM batches ORDER BY created_at DESC LIMIT ?1")?;
            let rows = stmt.query_map(params![limit as i64], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        ids.iter().map(|id| self.get_batch(id)).collect()
    }

    pub fn save_benchmark(&self, rec: &BenchmarkRecord) -> LedgerResult<()> {
        self.conn.lock().execute(
            "INSERT INTO benchmarks (id, created_at, kind, label, unit, higher_is_better, samples_json, context_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                rec.id,
                rec.created_at.to_rfc3339(),
                rec.kind,
                rec.label,
                rec.unit,
                rec.higher_is_better as i64,
                serde_json::to_string(&rec.samples)?,
                serde_json::to_string(&rec.context)?,
            ],
        )?;
        Ok(())
    }

    pub fn benchmarks(
        &self,
        kind: Option<&str>,
        limit: usize,
    ) -> LedgerResult<Vec<BenchmarkRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, created_at, kind, label, unit, higher_is_better, samples_json, context_json
             FROM benchmarks WHERE (?1 IS NULL OR kind = ?1) ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![kind, limit as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, created, kind, label, unit, hib, samples, ctx) = row?;
            out.push(BenchmarkRecord {
                id,
                created_at: parse_time(&created),
                kind,
                label,
                unit,
                higher_is_better: hib != 0,
                samples: serde_json::from_str(&samples)?,
                context: serde_json::from_str(&ctx)?,
            });
        }
        Ok(out)
    }

    pub fn get_benchmark(&self, id: &str) -> LedgerResult<BenchmarkRecord> {
        self.benchmarks(None, 10_000)?
            .into_iter()
            .find(|b| b.id == id)
            .ok_or_else(|| LedgerError::NotFound(id.into()))
    }

    pub fn kv_get(&self, key: &str) -> LedgerResult<Option<String>> {
        Ok(self
            .conn
            .lock()
            .query_row("SELECT value FROM kv WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn kv_set(&self, key: &str, value: &str) -> LedgerResult<()> {
        self.conn.lock().execute(
            "INSERT INTO kv (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

struct RawChange {
    id: String,
    batch_id: String,
    seq: i64,
    created_at: String,
    action_json: String,
    setting_json: Option<String>,
    before_json: Option<String>,
    after_json: Option<String>,
    status: String,
    reversible: i64,
    error: Option<String>,
    detail: Option<String>,
    reverted_at: Option<String>,
}

impl RawChange {
    fn into_record(self) -> LedgerResult<ChangeRecord> {
        Ok(ChangeRecord {
            id: self.id,
            batch_id: self.batch_id,
            seq: self.seq,
            created_at: parse_time(&self.created_at),
            action: serde_json::from_str(&self.action_json)?,
            setting: parse_opt(self.setting_json)?,
            before: parse_opt(self.before_json)?,
            after: parse_opt(self.after_json)?,
            status: ChangeStatus::parse(&self.status),
            reversible: self.reversible != 0,
            error: self.error,
            detail: self.detail,
            reverted_at: self.reverted_at.as_deref().map(parse_time),
        })
    }
}
