//! SQLite persistence for completed calls (#3) plus retention / rollup (#10).
//!
//! One row per completed call, written in its own transaction after the stream ends,
//! so a crash mid-stream never leaves a partial row. Rows are redacted (#9) before they
//! reach this layer.

use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

use crate::types::{CallOutcome, LLMMetrics};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS calls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts TEXT NOT NULL,
    model TEXT NOT NULL,
    prompt TEXT NOT NULL,
    prompt_tokens INTEGER,
    completion_tokens INTEGER,
    latency_ms INTEGER NOT NULL,
    ttft_ms INTEGER,
    status INTEGER NOT NULL,
    outcome TEXT NOT NULL,
    estimated_cost_usd REAL,
    trace_id TEXT,
    request_headers TEXT
);
CREATE INDEX IF NOT EXISTS idx_calls_ts ON calls(ts);
CREATE TABLE IF NOT EXISTS daily_rollup (
    day TEXT NOT NULL,
    model TEXT NOT NULL,
    calls INTEGER NOT NULL,
    errors INTEGER NOT NULL,
    prompt_tokens INTEGER NOT NULL,
    completion_tokens INTEGER NOT NULL,
    estimated_cost_usd REAL NOT NULL,
    PRIMARY KEY (day, model)
);
"#;

/// A row as stored (and as shown on the dashboard).
#[derive(Clone, Debug, PartialEq)]
pub struct CallRow {
    pub ts: String,
    pub model: String,
    pub prompt: String,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub latency_ms: u64,
    pub ttft_ms: Option<u64>,
    pub status: u16,
    pub outcome: String,
    pub estimated_cost_usd: Option<f64>,
    pub trace_id: Option<String>,
}

impl CallRow {
    pub fn from_metrics(m: &LLMMetrics) -> Self {
        Self {
            ts: m.timestamp.clone(),
            model: m.model.clone(),
            prompt: m.prompt.clone(),
            prompt_tokens: m.prompt_tokens,
            completion_tokens: m.completion_tokens,
            latency_ms: m.latency_ms,
            ttft_ms: m.ttft_ms,
            status: m.status,
            outcome: outcome_str(&m.outcome).to_string(),
            estimated_cost_usd: m.estimated_cost_usd,
            trace_id: m.trace_id.clone(),
        }
    }
}

pub fn outcome_str(o: &CallOutcome) -> &'static str {
    match o {
        CallOutcome::Ok => "ok",
        CallOutcome::UpstreamError => "upstream_error",
        CallOutcome::ClientAborted => "client_aborted",
        CallOutcome::UpstreamStreamError => "upstream_stream_error",
    }
}

/// Totals used to check that rollup preserves aggregate metrics.
#[derive(Debug, Default, PartialEq)]
pub struct Totals {
    pub calls: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}

#[derive(Debug, Default, PartialEq)]
pub struct GcReport {
    pub deleted: usize,
    pub rolled_up_groups: usize,
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Insert one completed call in its own transaction.
    pub fn insert(&self, row: &CallRow, request_headers_json: &str) -> rusqlite::Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO calls (ts, model, prompt, prompt_tokens, completion_tokens, latency_ms,
                ttft_ms, status, outcome, estimated_cost_usd, trace_id, request_headers)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                row.ts,
                row.model,
                row.prompt,
                row.prompt_tokens,
                row.completion_tokens,
                row.latency_ms as i64,
                row.ttft_ms.map(|v| v as i64),
                row.status,
                row.outcome,
                row.estimated_cost_usd,
                row.trace_id,
                request_headers_json,
            ],
        )?;
        tx.commit()
    }

    pub fn count(&self) -> rusqlite::Result<i64> {
        self.lock()
            .query_row("SELECT count(*) FROM calls", [], |r| r.get(0))
    }

    /// Most recent `limit` calls, newest first.
    pub fn recent(&self, limit: usize, model: Option<&str>) -> rusqlite::Result<Vec<CallRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT ts, model, prompt, prompt_tokens, completion_tokens, latency_ms, ttft_ms,
                    status, outcome, estimated_cost_usd, trace_id
             FROM calls WHERE (?1 IS NULL OR model = ?1) ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![model, limit as i64], |r| {
            Ok(CallRow {
                ts: r.get(0)?,
                model: r.get(1)?,
                prompt: r.get(2)?,
                prompt_tokens: r.get(3)?,
                completion_tokens: r.get(4)?,
                latency_ms: r.get::<_, i64>(5)? as u64,
                ttft_ms: r.get::<_, Option<i64>>(6)?.map(|v| v as u64),
                status: r.get(7)?,
                outcome: r.get(8)?,
                estimated_cost_usd: r.get(9)?,
                trace_id: r.get(10)?,
            })
        })?;
        rows.collect()
    }

    /// Stored request headers JSON for the newest row (tests / debugging).
    pub fn latest_headers(&self) -> rusqlite::Result<Option<String>> {
        self.lock()
            .query_row(
                "SELECT request_headers FROM calls ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()
            .map(|o| o.flatten())
    }

    /// Calls + rolled-up totals; unchanged by `gc(.., rollup = true)`.
    pub fn totals(&self) -> rusqlite::Result<Totals> {
        let conn = self.lock();
        let live: (i64, i64, i64) = conn.query_row(
            "SELECT count(*), coalesce(sum(prompt_tokens),0), coalesce(sum(completion_tokens),0) FROM calls",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let rolled: (i64, i64, i64) = conn.query_row(
            "SELECT coalesce(sum(calls),0), coalesce(sum(prompt_tokens),0), coalesce(sum(completion_tokens),0) FROM daily_rollup",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        Ok(Totals {
            calls: live.0 + rolled.0,
            prompt_tokens: live.1 + rolled.1,
            completion_tokens: live.2 + rolled.2,
        })
    }

    /// Delete calls older than `cutoff_rfc3339`, optionally folding them into
    /// `daily_rollup` first (same transaction, so totals stay consistent).
    pub fn gc_before(&self, cutoff_rfc3339: &str, rollup: bool) -> rusqlite::Result<GcReport> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let mut report = GcReport::default();
        if rollup {
            report.rolled_up_groups = tx.execute(
                "INSERT INTO daily_rollup (day, model, calls, errors, prompt_tokens, completion_tokens, estimated_cost_usd)
                 SELECT substr(ts, 1, 10), model, count(*),
                        sum(CASE WHEN outcome = 'ok' THEN 0 ELSE 1 END),
                        coalesce(sum(prompt_tokens), 0), coalesce(sum(completion_tokens), 0),
                        coalesce(sum(estimated_cost_usd), 0.0)
                 FROM calls WHERE ts < ?1 GROUP BY substr(ts, 1, 10), model
                 ON CONFLICT(day, model) DO UPDATE SET
                    calls = calls + excluded.calls,
                    errors = errors + excluded.errors,
                    prompt_tokens = prompt_tokens + excluded.prompt_tokens,
                    completion_tokens = completion_tokens + excluded.completion_tokens,
                    estimated_cost_usd = estimated_cost_usd + excluded.estimated_cost_usd",
                params![cutoff_rfc3339],
            )?;
        }
        report.deleted = tx.execute("DELETE FROM calls WHERE ts < ?1", params![cutoff_rfc3339])?;
        tx.commit()?;
        Ok(report)
    }

    /// `gc --days N`: everything older than N days.
    pub fn gc_days(&self, days: u32, rollup: bool) -> rusqlite::Result<GcReport> {
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339();
        self.gc_before(&cutoff, rollup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(ts: &str, model: &str, p: u32, c: u32) -> CallRow {
        CallRow {
            ts: ts.into(),
            model: model.into(),
            prompt: "hi".into(),
            prompt_tokens: Some(p),
            completion_tokens: Some(c),
            latency_ms: 10,
            ttft_ms: Some(2),
            status: 200,
            outcome: "ok".into(),
            estimated_cost_usd: Some(0.001),
            trace_id: Some("t".into()),
        }
    }

    #[test]
    fn n_inserts_give_n_rows_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.db");
        {
            let s = Store::open(&path).unwrap();
            for i in 0..5 {
                s.insert(
                    &row(&format!("2026-10-0{}T00:00:00+00:00", i + 1), "m", 1, 1),
                    "[]",
                )
                .unwrap();
            }
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.count().unwrap(), 5);
        assert_eq!(
            s.recent(2, None).unwrap()[0].ts,
            "2026-10-05T00:00:00+00:00"
        );
    }

    #[test]
    fn gc_rollup_keeps_totals() {
        let s = Store::open_in_memory().unwrap();
        s.insert(&row("2026-01-01T01:00:00+00:00", "a", 10, 5), "[]")
            .unwrap();
        s.insert(&row("2026-01-01T02:00:00+00:00", "a", 20, 5), "[]")
            .unwrap();
        s.insert(&row("2026-01-02T02:00:00+00:00", "b", 1, 1), "[]")
            .unwrap();
        s.insert(&row("2026-09-01T00:00:00+00:00", "a", 3, 3), "[]")
            .unwrap();
        let before = s.totals().unwrap();
        let rep = s.gc_before("2026-06-01T00:00:00+00:00", true).unwrap();
        assert_eq!(
            rep,
            GcReport {
                deleted: 3,
                rolled_up_groups: 2
            }
        );
        assert_eq!(s.count().unwrap(), 1);
        assert_eq!(s.totals().unwrap(), before);
    }

    #[test]
    fn gc_without_rollup_deletes() {
        let s = Store::open_in_memory().unwrap();
        s.insert(&row("2026-01-01T01:00:00+00:00", "a", 10, 5), "[]")
            .unwrap();
        let rep = s.gc_before("2026-06-01T00:00:00+00:00", false).unwrap();
        assert_eq!(rep.deleted, 1);
        assert_eq!(s.totals().unwrap(), Totals::default());
    }
}
