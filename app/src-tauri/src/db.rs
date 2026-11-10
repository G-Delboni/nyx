//! SQLite local da Nyx (`<data_dir>/nyx.db`). Migrações por `PRAGMA user_version`.
//! Timestamps: texto RFC3339 (para a UI) + inteiro em ms (para filtros por intervalo).

use crate::model::{
    EventKind, NyxEvent, Priority, ProjectContext, SessionStatus, TokenCounts,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use nyx_claude::UsageRecord;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

const MIGRATIONS: &[&str] = &[
    // v1
    r#"
    CREATE TABLE events (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        kind        TEXT    NOT NULL,
        priority    TEXT    NOT NULL,
        session_id  TEXT,
        context_id  TEXT,
        title       TEXT    NOT NULL,
        detail      TEXT,
        created_at  TEXT    NOT NULL,
        created_ms  INTEGER NOT NULL,
        read        INTEGER NOT NULL DEFAULT 0,
        raw_hook    TEXT
    );
    CREATE INDEX idx_events_created ON events(created_ms);

    CREATE TABLE usage (
        message_id     TEXT PRIMARY KEY,
        session_id     TEXT    NOT NULL,
        cwd            TEXT,
        model          TEXT    NOT NULL,
        ts             TEXT    NOT NULL,
        ts_ms          INTEGER NOT NULL,
        input          INTEGER NOT NULL,
        output         INTEGER NOT NULL,
        cache_write    INTEGER NOT NULL,
        cache_read     INTEGER NOT NULL,
        cost_usd       REAL    NOT NULL,
        parser_version INTEGER NOT NULL
    );
    CREATE INDEX idx_usage_ts ON usage(ts_ms);
    CREATE INDEX idx_usage_session ON usage(session_id);

    CREATE TABLE transcript_offsets (
        path   TEXT PRIMARY KEY,
        offset INTEGER NOT NULL,
        mtime  INTEGER NOT NULL,
        size   INTEGER NOT NULL
    );

    CREATE TABLE sessions (
        session_id      TEXT PRIMARY KEY,
        agent           TEXT    NOT NULL,
        cwd             TEXT,
        status          TEXT    NOT NULL,
        last_message    TEXT,
        transcript_path TEXT,
        started_at      TEXT    NOT NULL,
        updated_at      TEXT    NOT NULL,
        updated_ms      INTEGER NOT NULL
    );
    CREATE INDEX idx_sessions_updated ON sessions(updated_ms);

    CREATE TABLE contexts (
        id           TEXT PRIMARY KEY,
        name         TEXT NOT NULL,
        root_path    TEXT NOT NULL,
        instructions TEXT NOT NULL DEFAULT '',
        color        TEXT,
        created_at   TEXT NOT NULL
    );

    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    "#,
    // v2: início do trecho contínuo em `running` (para o toast de tarefa longa)
    r#"
    ALTER TABLE sessions ADD COLUMN running_since TEXT;
    "#,
    // v3 (Noite 2): delegação, chat, custo da IA da Nyx, campos novos do contexto.
    // Migração nova (e não somada à v2) porque bancos na v2 já existem.
    r#"
    ALTER TABLE contexts ADD COLUMN allowed_tools TEXT NOT NULL DEFAULT '[]';
    ALTER TABLE contexts ADD COLUMN refs TEXT NOT NULL DEFAULT '[]';
    ALTER TABLE contexts ADD COLUMN ai_enabled INTEGER NOT NULL DEFAULT 0;

    CREATE TABLE delegations (
        id                TEXT PRIMARY KEY,
        context_id        TEXT    NOT NULL,
        title             TEXT    NOT NULL,
        status            TEXT    NOT NULL,
        current_step      INTEGER NOT NULL DEFAULT 0,
        worktree_path     TEXT    NOT NULL DEFAULT '',
        branch            TEXT    NOT NULL DEFAULT '',
        base_branch       TEXT    NOT NULL DEFAULT '',
        claude_session_id TEXT,
        handoffs          INTEGER NOT NULL DEFAULT 0,
        pending_handoff   INTEGER NOT NULL DEFAULT 0,
        blocked_reason    TEXT,
        use_ai            INTEGER NOT NULL DEFAULT 0,
        model             TEXT,
        finalized         INTEGER NOT NULL DEFAULT 0,
        created_at        TEXT    NOT NULL,
        updated_at        TEXT    NOT NULL
    );
    CREATE INDEX idx_delegations_ctx ON delegations(context_id, updated_at);

    CREATE TABLE delegation_steps (
        delegation_id TEXT    NOT NULL,
        idx           INTEGER NOT NULL,
        prompt        TEXT    NOT NULL,
        status        TEXT    NOT NULL,
        attempts      INTEGER NOT NULL DEFAULT 0,
        report        TEXT,
        ai_summary    TEXT,
        feedback      TEXT    NOT NULL DEFAULT '[]',
        commit_hash   TEXT,
        cost_usd      REAL    NOT NULL DEFAULT 0,
        context_fill  REAL,
        started_at    TEXT,
        finished_at   TEXT,
        PRIMARY KEY (delegation_id, idx)
    );

    CREATE TABLE chat_messages (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        context_id TEXT,
        role       TEXT    NOT NULL,
        text       TEXT    NOT NULL,
        actions    TEXT    NOT NULL DEFAULT '[]',
        cost_usd   REAL    NOT NULL DEFAULT 0,
        created_at TEXT    NOT NULL
    );
    CREATE INDEX idx_chat_ctx ON chat_messages(context_id, id);

    CREATE TABLE ai_costs (
        id       INTEGER PRIMARY KEY AUTOINCREMENT,
        ts_ms    INTEGER NOT NULL,
        kind     TEXT    NOT NULL,
        cost_usd REAL    NOT NULL
    );
    CREATE INDEX idx_ai_costs_ts ON ai_costs(ts_ms);
    "#,
];

pub struct Db {
    conn: Connection,
}

/// Evento a gravar (o id e o `read` vêm do banco).
#[derive(Debug, Clone)]
pub struct NewEvent {
    pub kind: EventKind,
    pub priority: Priority,
    pub session_id: Option<String>,
    pub context_id: Option<String>,
    pub title: String,
    pub detail: Option<String>,
    pub created_at: DateTime<Utc>,
    pub raw_hook: Option<String>,
}

/// Linha de uso já gravada.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageRow {
    pub message_id: String,
    pub session_id: String,
    pub cwd: Option<String>,
    pub model: String,
    pub ts: DateTime<Utc>,
    pub tokens: TokenCounts,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    pub session_id: String,
    pub agent: String,
    pub cwd: Option<String>,
    pub status: SessionStatus,
    pub last_message: Option<String>,
    pub transcript_path: Option<String>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Desde quando a sessão está `running` sem interrupção (None fora de `running`).
    pub running_since: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileOffset {
    pub offset: u64,
    pub mtime: i64,
    pub size: u64,
}

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_default()
}

fn rfc(ts: &DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let conn = Connection::open(path).with_context(|| format!("abrindo {}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=3000; PRAGMA cache_size=-1024;",
        )?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        let mut db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&mut self) -> Result<()> {
        let current: usize = self.conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql).with_context(|| format!("migração v{}", i + 1))?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    // ---------- events ----------

    pub fn insert_event(&self, e: &NewEvent) -> Result<NyxEvent> {
        self.conn.execute(
            "INSERT INTO events (kind, priority, session_id, context_id, title, detail, created_at, created_ms, raw_hook)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                e.kind.as_str(),
                e.priority.as_str(),
                e.session_id,
                e.context_id,
                e.title,
                e.detail,
                rfc(&e.created_at),
                e.created_at.timestamp_millis(),
                e.raw_hook,
            ],
        )?;
        Ok(NyxEvent {
            id: self.conn.last_insert_rowid(),
            kind: e.kind,
            priority: e.priority,
            session_id: e.session_id.clone(),
            context_id: e.context_id.clone(),
            title: e.title.clone(),
            detail: e.detail.clone(),
            created_at: rfc(&e.created_at),
            read: false,
        })
    }

    pub fn list_events(&self, limit: u32, context_id: Option<&str>) -> Result<Vec<NyxEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, priority, session_id, context_id, title, detail, created_at, read
             FROM events WHERE (?1 IS NULL OR context_id = ?1)
             ORDER BY created_ms DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![context_id, limit], |r| {
            let kind: String = r.get(1)?;
            let prio: String = r.get(2)?;
            Ok(NyxEvent {
                id: r.get(0)?,
                kind: EventKind::parse(&kind).unwrap_or(EventKind::Error),
                priority: Priority::parse(&prio),
                session_id: r.get(3)?,
                context_id: r.get(4)?,
                title: r.get(5)?,
                detail: r.get(6)?,
                created_at: r.get(7)?,
                read: r.get::<_, i64>(8)? != 0,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn mark_event_read(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE events SET read = 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn unread_count(&self) -> Result<u64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM events WHERE read = 0", [], |r| r.get::<_, i64>(0))? as u64)
    }

    // ---------- usage ----------

    /// Grava em uma transação, com upsert pela PK `message_id`. Durante o streaming a mesma
    /// mensagem aparece várias vezes e a versão final pode chegar depois da parcial, então
    /// cada contador fica com o maior valor visto. Devolve quantas linhas entraram ou cresceram.
    pub fn insert_usage(&mut self, records: &[(UsageRecord, f64)]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut n = 0;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO usage
                 (message_id, session_id, cwd, model, ts, ts_ms, input, output, cache_write, cache_read, cost_usd, parser_version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(message_id) DO UPDATE SET
                    input       = max(usage.input, excluded.input),
                    output      = max(usage.output, excluded.output),
                    cache_write = max(usage.cache_write, excluded.cache_write),
                    cache_read  = max(usage.cache_read, excluded.cache_read),
                    cost_usd    = max(usage.cost_usd, excluded.cost_usd)
                 WHERE excluded.input > usage.input OR excluded.output > usage.output
                    OR excluded.cache_write > usage.cache_write OR excluded.cache_read > usage.cache_read",
            )?;
            for (r, cost) in records {
                n += stmt.execute(params![
                    r.message_id,
                    r.session_id,
                    r.cwd,
                    r.model,
                    rfc(&r.ts),
                    r.ts.timestamp_millis(),
                    r.input as i64,
                    r.output as i64,
                    r.cache_write as i64,
                    r.cache_read as i64,
                    cost,
                    nyx_claude::PARSER_VERSION,
                ])?;
            }
        }
        tx.commit()?;
        Ok(n)
    }

    fn usage_query(&self, sql: &str, p: impl rusqlite::Params) -> Result<Vec<UsageRow>> {
        let mut stmt = self.conn.prepare_cached(sql)?;
        let rows = stmt.query_map(p, |r| {
            let ts: String = r.get(4)?;
            Ok(UsageRow {
                message_id: r.get(0)?,
                session_id: r.get(1)?,
                cwd: r.get(2)?,
                model: r.get(3)?,
                ts: parse_ts(&ts),
                tokens: TokenCounts {
                    input: r.get::<_, i64>(5)? as u64,
                    output: r.get::<_, i64>(6)? as u64,
                    cache_write: r.get::<_, i64>(7)? as u64,
                    cache_read: r.get::<_, i64>(8)? as u64,
                },
                cost_usd: r.get(9)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Linhas com `from <= ts < to`.
    pub fn usage_between(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<UsageRow>> {
        self.usage_query(
            "SELECT message_id, session_id, cwd, model, ts, input, output, cache_write, cache_read, cost_usd
             FROM usage WHERE ts_ms >= ?1 AND ts_ms < ?2 ORDER BY ts_ms",
            params![from.timestamp_millis(), to.timestamp_millis()],
        )
    }

    pub fn usage_for_session(&self, session_id: &str) -> Result<Vec<UsageRow>> {
        self.usage_query(
            "SELECT message_id, session_id, cwd, model, ts, input, output, cache_write, cache_read, cost_usd
             FROM usage WHERE session_id = ?1 ORDER BY ts_ms",
            params![session_id],
        )
    }

    /// Soma de tokens e custo de uma sessão.
    pub fn session_totals(&self, session_id: &str) -> Result<(TokenCounts, f64)> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(input),0), COALESCE(SUM(output),0), COALESCE(SUM(cache_write),0),
                    COALESCE(SUM(cache_read),0), COALESCE(SUM(cost_usd),0)
             FROM usage WHERE session_id = ?1",
            [session_id],
            |r| {
                Ok((
                    TokenCounts {
                        input: r.get::<_, i64>(0)? as u64,
                        output: r.get::<_, i64>(1)? as u64,
                        cache_write: r.get::<_, i64>(2)? as u64,
                        cache_read: r.get::<_, i64>(3)? as u64,
                    },
                    r.get(4)?,
                ))
            },
        )?)
    }

    /// Sessão com a mensagem de uso mais recente (fallback da visão "sessão atual").
    pub fn latest_usage_session(&self) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT session_id FROM usage ORDER BY ts_ms DESC LIMIT 1", [], |r| r.get(0))
            .optional()?)
    }

    // ---------- transcript offsets ----------

    pub fn get_offset(&self, path: &str) -> Result<Option<FileOffset>> {
        Ok(self
            .conn
            .query_row(
                "SELECT offset, mtime, size FROM transcript_offsets WHERE path = ?1",
                [path],
                |r| {
                    Ok(FileOffset {
                        offset: r.get::<_, i64>(0)? as u64,
                        mtime: r.get(1)?,
                        size: r.get::<_, i64>(2)? as u64,
                    })
                },
            )
            .optional()?)
    }

    pub fn set_offset(&self, path: &str, o: FileOffset) -> Result<()> {
        self.conn.execute(
            "INSERT INTO transcript_offsets (path, offset, mtime, size) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET offset = excluded.offset, mtime = excluded.mtime, size = excluded.size",
            params![path, o.offset as i64, o.mtime, o.size as i64],
        )?;
        Ok(())
    }

    // ---------- sessions ----------

    pub fn get_session(&self, session_id: &str) -> Result<Option<SessionRow>> {
        Ok(self
            .conn
            .query_row(
                "SELECT session_id, agent, cwd, status, last_message, transcript_path, started_at, updated_at, running_since
                 FROM sessions WHERE session_id = ?1",
                [session_id],
                Self::map_session,
            )
            .optional()?)
    }

    fn map_session(r: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRow> {
        let status: String = r.get(3)?;
        let started: String = r.get(6)?;
        let updated: String = r.get(7)?;
        let running: Option<String> = r.get(8)?;
        Ok(SessionRow {
            session_id: r.get(0)?,
            agent: r.get(1)?,
            cwd: r.get(2)?,
            status: SessionStatus::parse(&status),
            last_message: r.get(4)?,
            transcript_path: r.get(5)?,
            started_at: parse_ts(&started),
            updated_at: parse_ts(&updated),
            running_since: running.as_deref().map(parse_ts),
        })
    }

    /// Sessões mais recentes primeiro.
    pub fn list_sessions(&self, limit: u32) -> Result<Vec<SessionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_id, agent, cwd, status, last_message, transcript_path, started_at, updated_at, running_since
             FROM sessions ORDER BY updated_ms DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], Self::map_session)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn upsert_session(&self, s: &SessionRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (session_id, agent, cwd, status, last_message, transcript_path, started_at, updated_at, updated_ms, running_since)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(session_id) DO UPDATE SET
                agent = excluded.agent, cwd = excluded.cwd, status = excluded.status,
                last_message = excluded.last_message, transcript_path = excluded.transcript_path,
                updated_at = excluded.updated_at, updated_ms = excluded.updated_ms,
                running_since = excluded.running_since",
            params![
                s.session_id,
                s.agent,
                s.cwd,
                s.status.as_str(),
                s.last_message,
                s.transcript_path,
                rfc(&s.started_at),
                rfc(&s.updated_at),
                s.updated_at.timestamp_millis(),
                s.running_since.as_ref().map(rfc),
            ],
        )?;
        Ok(())
    }

    /// Marca como `idle` as sessões nos estados dados sem evento desde `before`.
    /// Não mexe em `updated_at` (a última atividade real continua visível).
    pub fn mark_idle(&self, before: DateTime<Utc>, from_states: &[SessionStatus]) -> Result<usize> {
        let mut n = 0;
        for st in from_states {
            n += self.conn.execute(
                "UPDATE sessions SET status = 'idle', running_since = NULL WHERE status = ?1 AND updated_ms < ?2",
                params![st.as_str(), before.timestamp_millis()],
            )?;
        }
        Ok(n)
    }

    // ---------- contexts ----------

    pub fn list_contexts(&self) -> Result<Vec<ProjectContext>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, root_path, instructions, color, created_at, allowed_tools, refs, ai_enabled
             FROM contexts ORDER BY created_at, name",
        )?;
        let rows = stmt.query_map([], |r| {
            let tools: String = r.get(6)?;
            let refs: String = r.get(7)?;
            Ok(ProjectContext {
                id: r.get(0)?,
                name: r.get(1)?,
                root_path: r.get(2)?,
                instructions: r.get(3)?,
                color: r.get(4)?,
                created_at: r.get(5)?,
                allowed_tools: serde_json::from_str(&tools).unwrap_or_default(),
                references: serde_json::from_str(&refs).unwrap_or_default(),
                ai_enabled: r.get::<_, i64>(8)? != 0,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn save_context(&self, c: &ProjectContext) -> Result<()> {
        self.conn.execute(
            "INSERT INTO contexts (id, name, root_path, instructions, color, created_at, allowed_tools, refs, ai_enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, root_path = excluded.root_path,
                instructions = excluded.instructions, color = excluded.color,
                allowed_tools = excluded.allowed_tools, refs = excluded.refs, ai_enabled = excluded.ai_enabled",
            params![
                c.id,
                c.name,
                c.root_path,
                c.instructions,
                c.color,
                c.created_at,
                serde_json::to_string(&c.allowed_tools)?,
                serde_json::to_string(&c.references)?,
                c.ai_enabled as i64
            ],
        )?;
        Ok(())
    }

    pub fn get_context(&self, id: &str) -> Result<Option<ProjectContext>> {
        Ok(self.list_contexts()?.into_iter().find(|c| c.id == id))
    }

    /// Acesso direto para os módulos da Noite 2 (delegação, chat), que têm o SQL deles.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn add_ai_cost(&self, kind: &str, cost_usd: f64, at: DateTime<Utc>) -> Result<()> {
        if cost_usd > 0.0 {
            self.conn.execute(
                "INSERT INTO ai_costs (ts_ms, kind, cost_usd) VALUES (?1, ?2, ?3)",
                params![at.timestamp_millis(), kind, cost_usd],
            )?;
        }
        Ok(())
    }

    pub fn ai_cost_since(&self, from: DateTime<Utc>) -> Result<f64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(cost_usd), 0) FROM ai_costs WHERE ts_ms >= ?1",
            [from.timestamp_millis()],
            |r| r.get(0),
        )?)
    }

    pub fn delete_context(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM contexts WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---------- settings ----------

    pub fn get_setting<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let v: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .optional()?;
        Ok(match v {
            Some(s) => serde_json::from_str(&s).ok(),
            None => None,
        })
    }

    pub fn set_setting<T: serde::Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use chrono::TimeZone;

    pub fn rec(id: &str, session: &str, cwd: Option<&str>, model: &str, ts: DateTime<Utc>, input: u64) -> UsageRecord {
        UsageRecord {
            message_id: id.into(),
            session_id: session.into(),
            cwd: cwd.map(Into::into),
            model: model.into(),
            ts,
            input,
            output: input / 2,
            cache_write: 10,
            cache_read: 100,
        }
    }

    fn t(h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 7, h, 0, 0).unwrap()
    }

    #[test]
    fn migrates_file_db_and_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub").join("nyx.db");
        {
            let db = Db::open(&p).unwrap();
            assert_eq!(db.schema_version().unwrap(), MIGRATIONS.len() as i64);
            db.set_setting("x", &42).unwrap();
        }
        let db = Db::open(&p).unwrap();
        assert_eq!(db.schema_version().unwrap(), MIGRATIONS.len() as i64);
        assert_eq!(db.get_setting::<i32>("x").unwrap(), Some(42));
        assert_eq!(db.get_setting::<i32>("missing").unwrap(), None);
    }

    #[test]
    fn usage_dedup_by_message_id() {
        let mut db = Db::open_in_memory().unwrap();
        let a = rec("m1", "s1", Some("C:/p"), "claude-opus", t(1), 100);
        let n = db.insert_usage(&[(a.clone(), 1.0), (a.clone(), 1.0)]).unwrap();
        assert_eq!(n, 1);
        let n = db.insert_usage(&[(a, 1.0), (rec("m2", "s1", None, "claude-haiku", t(2), 50), 0.5)]).unwrap();
        assert_eq!(n, 1);
        // versão final (maior output) chegando depois da parcial: atualiza
        let mut fin = rec("m1", "s1", Some("C:/p"), "claude-opus", t(1), 100);
        fin.output = 999;
        assert_eq!(db.insert_usage(&[(fin, 2.0)]).unwrap(), 1);
        // parcial atrasada: não reduz
        assert_eq!(db.insert_usage(&[(rec("m1", "s1", None, "claude-opus", t(1), 100), 1.0)]).unwrap(), 0);
        let rows = db.usage_between(t(0), t(3)).unwrap();
        assert_eq!(rows.iter().find(|r| r.message_id == "m1").unwrap().tokens.output, 999);
        assert_eq!(rows.len(), 2);
        assert_eq!(db.usage_between(t(2), t(3)).unwrap().len(), 1);
        let (tok, cost) = db.session_totals("s1").unwrap();
        assert_eq!(tok.input, 150);
        assert!((cost - 2.5).abs() < 1e-9);
        assert_eq!(db.latest_usage_session().unwrap().as_deref(), Some("s1"));
    }

    #[test]
    fn events_roundtrip_and_filter() {
        let db = Db::open_in_memory().unwrap();
        let mk = |ctx: Option<&str>, h| NewEvent {
            kind: EventKind::TaskDone,
            priority: Priority::High,
            session_id: Some("s".into()),
            context_id: ctx.map(Into::into),
            title: "Claude Code concluiu".into(),
            detail: None,
            created_at: t(h),
            raw_hook: None,
        };
        let e1 = db.insert_event(&mk(Some("c1"), 1)).unwrap();
        db.insert_event(&mk(None, 2)).unwrap();
        assert_eq!(db.unread_count().unwrap(), 2);
        let all = db.list_events(10, None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].id, e1.id); // mais recente primeiro
        assert_eq!(db.list_events(10, Some("c1")).unwrap().len(), 1);
        db.mark_event_read(e1.id).unwrap();
        assert_eq!(db.unread_count().unwrap(), 1);
        assert!(db.list_events(1, Some("c1")).unwrap()[0].read);
    }

    #[test]
    fn offsets_sessions_contexts() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.get_offset("a").unwrap().is_none());
        db.set_offset("a", FileOffset { offset: 10, mtime: 5, size: 10 }).unwrap();
        db.set_offset("a", FileOffset { offset: 20, mtime: 6, size: 20 }).unwrap();
        assert_eq!(db.get_offset("a").unwrap().unwrap().offset, 20);

        let s = SessionRow {
            session_id: "s1".into(),
            agent: "claude-code".into(),
            cwd: Some("C:\\x".into()),
            status: SessionStatus::Running,
            last_message: None,
            transcript_path: None,
            started_at: t(1),
            updated_at: t(1),
            running_since: Some(t(1)),
        };
        db.upsert_session(&s).unwrap();
        db.upsert_session(&SessionRow { status: SessionStatus::Done, updated_at: t(2), started_at: t(9), ..s.clone() })
            .unwrap();
        let got = db.get_session("s1").unwrap().unwrap();
        assert_eq!(got.status, SessionStatus::Done);
        assert_eq!(got.started_at, t(1)); // started_at não muda no upsert
        assert_eq!(got.running_since, Some(t(1)));
        assert_eq!(db.mark_idle(t(3), &[SessionStatus::Done]).unwrap(), 1);
        assert_eq!(db.get_session("s1").unwrap().unwrap().status, SessionStatus::Idle);
        assert_eq!(db.list_sessions(10).unwrap().len(), 1);

        let c = ProjectContext {
            id: "c1".into(),
            name: "Projeto Demo".into(),
            root_path: "C:/Users/x/Documents/projeto demo".into(),
            instructions: String::new(),
            color: None,
            created_at: "2026-11-07T00:00:00Z".into(),
            ..Default::default()
        };
        db.save_context(&c).unwrap();
        db.save_context(&ProjectContext { name: "CD".into(), ..c.clone() }).unwrap();
        assert_eq!(db.list_contexts().unwrap()[0].name, "CD");
        db.delete_context("c1").unwrap();
        assert!(db.list_contexts().unwrap().is_empty());
    }
}
