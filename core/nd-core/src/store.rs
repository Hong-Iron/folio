//! SQLite persistence. WAL + synchronous=NORMAL: a crash of the app loses
//! nothing that was committed, and commits don't fsync every tick.

use rusqlite::{params, Connection, OptionalExtension};

use crate::classify::Activity;
use crate::error::{NdError, Result};
use crate::rules::{Category, Rule, RuleKind, Rules};

const SCHEMA_VERSION: i32 = 1;

#[derive(Clone, Debug, PartialEq)]
pub struct SegmentRow {
    pub id: i64,
    pub session_id: i64,
    pub start_wall: i64,
    pub end_wall: i64,
    pub activity: Activity,
    pub detail: Option<String>,
    pub claimed: bool,
    pub tz_offset_min: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionRow {
    pub id: i64,
    pub started_wall: i64,
    pub ended_wall: Option<i64>,
    pub tz_offset_min: i32,
    pub label: Option<String>,
    pub goal_min: Option<u32>,
    pub closed: bool,
}

pub struct Store {
    conn: Connection,
}

fn cat_str(c: Category) -> &'static str {
    match c {
        Category::Focus => "focus",
        Category::Neutral => "neutral",
        Category::Drift => "drift",
    }
}

fn cat_parse(s: &str) -> Category {
    match s {
        "focus" => Category::Focus,
        "drift" => Category::Drift,
        _ => Category::Neutral,
    }
}

fn kind_str(k: RuleKind) -> &'static str {
    match k {
        RuleKind::App => "app",
        RuleKind::Domain => "domain",
    }
}

impl Store {
    pub fn open(path: &str) -> Result<Self> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| NdError::Storage { msg: e.to_string() })?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let version: i32 = self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(NdError::Storage { msg: format!("database is from a newer version ({version})") });
        }
        if version < 1 {
            self.conn.execute_batch(
                "BEGIN;
                CREATE TABLE IF NOT EXISTS sessions (
                    id INTEGER PRIMARY KEY,
                    started_wall INTEGER NOT NULL,
                    ended_wall INTEGER,
                    tz_offset_min INTEGER NOT NULL,
                    label TEXT,
                    goal_min INTEGER,
                    closed INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE IF NOT EXISTS segments (
                    id INTEGER PRIMARY KEY,
                    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    start_wall INTEGER NOT NULL,
                    end_wall INTEGER NOT NULL,
                    activity TEXT NOT NULL,
                    detail TEXT,
                    claimed INTEGER NOT NULL DEFAULT 0,
                    edited INTEGER NOT NULL DEFAULT 0
                );
                CREATE INDEX IF NOT EXISTS segments_session ON segments(session_id);
                CREATE INDEX IF NOT EXISTS segments_start ON segments(start_wall);
                CREATE TABLE IF NOT EXISTS rules (
                    pattern TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    category TEXT NOT NULL,
                    PRIMARY KEY (pattern, kind)
                );
                CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS corrections (
                    id INTEGER PRIMARY KEY,
                    segment_id INTEGER,
                    created_wall INTEGER NOT NULL,
                    label TEXT NOT NULL
                );
                PRAGMA user_version = 1;
                COMMIT;",
            )?;
        }
        Ok(())
    }

    pub fn integrity_ok(&self) -> bool {
        self.conn
            .query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
            .map(|s| s == "ok")
            .unwrap_or(false)
    }

    pub fn create_session(&self, started_wall: i64, tz: i32, label: Option<&str>, goal_min: Option<u32>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO sessions (started_wall, ended_wall, tz_offset_min, label, goal_min) VALUES (?1, ?1, ?2, ?3, ?4)",
            params![started_wall, tz, label, goal_min],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn touch_session(&self, id: i64, ended_wall: i64) -> Result<()> {
        self.conn.execute("UPDATE sessions SET ended_wall = ?2 WHERE id = ?1", params![id, ended_wall])?;
        Ok(())
    }

    pub fn close_session(&self, id: i64, ended_wall: i64) -> Result<()> {
        self.conn.execute("UPDATE sessions SET ended_wall = ?2, closed = 1 WHERE id = ?1", params![id, ended_wall])?;
        Ok(())
    }

    pub fn insert_segment(&self, session_id: i64, start: i64, end: i64, a: Activity, detail: Option<&str>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO segments (session_id, start_wall, end_wall, activity, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, start, end, a.as_str(), detail],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_segment_end(&self, id: i64, end: i64) -> Result<()> {
        self.conn.execute("UPDATE segments SET end_wall = ?2 WHERE id = ?1", params![id, end])?;
        Ok(())
    }

    pub fn update_segment_start(&self, id: i64, start: i64) -> Result<()> {
        self.conn.execute("UPDATE segments SET start_wall = ?2 WHERE id = ?1", params![id, start])?;
        Ok(())
    }

    pub fn delete_segment(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM segments WHERE id = ?1", params![id])?;
        Ok(())
    }

    fn record_correction(&self, segment_id: i64, label: &str, now_wall: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO corrections (segment_id, created_wall, label) VALUES (?1, ?2, ?3)",
            params![segment_id, now_wall, label],
        )?;
        Ok(())
    }

    pub fn set_claimed(&self, id: i64, claimed: bool, now_wall: i64) -> Result<()> {
        let n = self.conn.execute("UPDATE segments SET claimed = ?2 WHERE id = ?1", params![id, claimed])?;
        if n == 0 {
            return Err(NdError::Invalid { msg: format!("no segment {id}") });
        }
        self.record_correction(id, if claimed { "worked" } else { "not_worked" }, now_wall)
    }

    pub fn set_segment_activity(&self, id: i64, a: Activity, now_wall: i64) -> Result<()> {
        let n = self
            .conn
            .execute("UPDATE segments SET activity = ?2, edited = 1, claimed = 0 WHERE id = ?1", params![id, a.as_str()])?;
        if n == 0 {
            return Err(NdError::Invalid { msg: format!("no segment {id}") });
        }
        self.record_correction(id, a.as_str(), now_wall)
    }

    fn segment_from_row(r: &rusqlite::Row) -> rusqlite::Result<SegmentRow> {
        let a: String = r.get(4)?;
        Ok(SegmentRow {
            id: r.get(0)?,
            session_id: r.get(1)?,
            start_wall: r.get(2)?,
            end_wall: r.get(3)?,
            activity: Activity::parse(&a).unwrap_or(Activity::Unmonitored),
            detail: r.get(5)?,
            claimed: r.get(6)?,
            tz_offset_min: r.get(7)?,
        })
    }

    const SEGMENT_COLS: &'static str = "s.id, s.session_id, s.start_wall, s.end_wall, s.activity, s.detail, s.claimed, x.tz_offset_min";

    /// Segments overlapping [from, to), oldest first.
    pub fn segments_between(&self, from_wall: i64, to_wall: i64) -> Result<Vec<SegmentRow>> {
        let sql = format!(
            "SELECT {} FROM segments s JOIN sessions x ON x.id = s.session_id
             WHERE s.end_wall > ?1 AND s.start_wall < ?2 AND s.end_wall > s.start_wall
             ORDER BY s.start_wall",
            Self::SEGMENT_COLS
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![from_wall, to_wall], Self::segment_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn session_segments(&self, session_id: i64) -> Result<Vec<SegmentRow>> {
        let sql = format!(
            "SELECT {} FROM segments s JOIN sessions x ON x.id = s.session_id WHERE s.session_id = ?1 ORDER BY s.start_wall",
            Self::SEGMENT_COLS
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![session_id], Self::segment_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn segment(&self, id: i64) -> Result<Option<SegmentRow>> {
        let sql = format!(
            "SELECT {} FROM segments s JOIN sessions x ON x.id = s.session_id WHERE s.id = ?1",
            Self::SEGMENT_COLS
        );
        Ok(self.conn.query_row(&sql, params![id], Self::segment_from_row).optional()?)
    }

    fn session_from_row(r: &rusqlite::Row) -> rusqlite::Result<SessionRow> {
        Ok(SessionRow {
            id: r.get(0)?,
            started_wall: r.get(1)?,
            ended_wall: r.get(2)?,
            tz_offset_min: r.get(3)?,
            label: r.get(4)?,
            goal_min: r.get(5)?,
            closed: r.get(6)?,
        })
    }

    pub fn session(&self, id: i64) -> Result<Option<SessionRow>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, started_wall, ended_wall, tz_offset_min, label, goal_min, closed FROM sessions WHERE id = ?1",
                params![id],
                Self::session_from_row,
            )
            .optional()?)
    }

    pub fn open_sessions(&self) -> Result<Vec<SessionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, started_wall, ended_wall, tz_offset_min, label, goal_min, closed FROM sessions WHERE closed = 0 ORDER BY id",
        )?;
        let rows = stmt.query_map([], Self::session_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn sessions_between(&self, from_wall: i64, to_wall: i64) -> Result<Vec<SessionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, started_wall, ended_wall, tz_offset_min, label, goal_min, closed FROM sessions
             WHERE COALESCE(ended_wall, started_wall) > ?1 AND started_wall < ?2 ORDER BY started_wall",
        )?;
        let rows = stmt.query_map(params![from_wall, to_wall], Self::session_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn load_rules(&self) -> Result<Rules> {
        let count: i64 = self.conn.query_row("SELECT COUNT(*) FROM rules", [], |r| r.get(0))?;
        let seeded: Option<String> = self.get_setting("rules_seeded")?;
        if count == 0 && seeded.is_none() {
            for r in Rules::defaults() {
                self.save_rule(&r)?;
            }
            self.set_setting("rules_seeded", "1")?;
        }
        let mut stmt = self.conn.prepare("SELECT pattern, kind, category FROM rules ORDER BY kind, pattern")?;
        let rows = stmt.query_map([], |r| {
            let kind: String = r.get(1)?;
            let cat: String = r.get(2)?;
            Ok(Rule {
                pattern: r.get(0)?,
                kind: if kind == "app" { RuleKind::App } else { RuleKind::Domain },
                category: cat_parse(&cat),
            })
        })?;
        Ok(Rules::new(rows.collect::<rusqlite::Result<_>>()?))
    }

    pub fn save_rule(&self, r: &Rule) -> Result<()> {
        self.conn.execute(
            "INSERT INTO rules (pattern, kind, category) VALUES (?1, ?2, ?3)
             ON CONFLICT(pattern, kind) DO UPDATE SET category = excluded.category",
            params![r.pattern, kind_str(r.kind), cat_str(r.category)],
        )?;
        Ok(())
    }

    pub fn delete_rule(&self, pattern: &str, kind: RuleKind) -> Result<()> {
        self.conn.execute("DELETE FROM rules WHERE pattern = ?1 AND kind = ?2", params![pattern, kind_str(kind)])?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        self.conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
        Ok(())
    }

    /// Wipes history and identity; keeps the database usable.
    pub fn delete_all(&self) -> Result<()> {
        self.conn.execute_batch(
            "BEGIN; DELETE FROM corrections; DELETE FROM segments; DELETE FROM sessions;
             DELETE FROM settings WHERE key IN ('face_template', 'calibration'); COMMIT;",
        )?;
        Ok(())
    }

    pub fn export_csv(&self, path: &str) -> Result<u32> {
        let rows = self.segments_between(i64::MIN, i64::MAX)?;
        let mut out = String::from("session_id,segment_id,start_utc_ms,end_utc_ms,tz_offset_min,activity,detail,claimed\n");
        for r in &rows {
            let detail = r.detail.as_deref().unwrap_or("").replace('"', "\"\"");
            out.push_str(&format!(
                "{},{},{},{},{},{},\"{}\",{}\n",
                r.session_id,
                r.id,
                r.start_wall,
                r.end_wall,
                r.tz_offset_min,
                r.activity.as_str(),
                detail,
                r.claimed as u8
            ));
        }
        std::fs::write(path, out).map_err(|e| NdError::Storage { msg: e.to_string() })?;
        Ok(rows.len() as u32)
    }
}
