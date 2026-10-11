//! Local settings and recent files (SQLite, WAL).

use opdf_core::{Error, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub struct Settings {
    conn: Mutex<Connection>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentEntry {
    pub path: PathBuf,
    pub opened_at: i64,
}

const MAX_RECENT: i64 = 30;
const MAX_UI_SETTINGS_BYTES: usize = 64 * 1024;

fn db_err(e: rusqlite::Error) -> Error {
    Error::internal(format!("settings database: {e}"))
}

impl Settings {
    pub fn open(path: &Path) -> Result<Settings> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path).map_err(db_err)?;
        Self::init(conn)
    }

    pub fn in_memory() -> Settings {
        Self::init(Connection::open_in_memory().expect("in-memory sqlite")).expect("schema")
    }

    fn init(conn: Connection) -> Result<Settings> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS recent (path TEXT PRIMARY KEY, opened_at INTEGER NOT NULL);",
        )
        .map_err(db_err)?;
        Ok(Settings {
            conn: Mutex::new(conn),
        })
    }

    pub fn ui(&self) -> serde_json::Value {
        let conn = self.conn.lock();
        conn.query_row("SELECT value FROM kv WHERE key = 'ui'", [], |r| {
            r.get::<_, String>(0)
        })
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Object(Default::default()))
    }

    pub fn set_ui(&self, v: &serde_json::Value) -> Result<()> {
        if !v.is_object() {
            return Err(Error::invalid_arg("settings must be an object"));
        }
        let s = v.to_string();
        if s.len() > MAX_UI_SETTINGS_BYTES {
            return Err(Error::invalid_arg("settings are too large"));
        }
        self.conn
            .lock()
            .execute(
                "INSERT INTO kv(key, value) VALUES('ui', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![s],
            )
            .map_err(db_err)?;
        Ok(())
    }

    pub fn add_recent(&self, path: &Path) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let conn = self.conn.lock();
        let _ = conn.execute(
            "INSERT INTO recent(path, opened_at) VALUES(?1, ?2) ON CONFLICT(path) DO UPDATE SET opened_at = excluded.opened_at",
            params![path.to_string_lossy(), now],
        );
        let _ = conn.execute(
            "DELETE FROM recent WHERE path NOT IN (SELECT path FROM recent ORDER BY opened_at DESC LIMIT ?1)",
            params![MAX_RECENT],
        );
    }

    pub fn recent(&self) -> Vec<RecentEntry> {
        let conn = self.conn.lock();
        let Ok(mut stmt) =
            conn.prepare("SELECT path, opened_at FROM recent ORDER BY opened_at DESC")
        else {
            return Vec::new();
        };
        stmt.query_map([], |r| {
            Ok(RecentEntry {
                path: PathBuf::from(r.get::<_, String>(0)?),
                opened_at: r.get(1)?,
            })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    pub fn remove_recent(&self, path: &Path) {
        let _ = self.conn.lock().execute(
            "DELETE FROM recent WHERE path = ?1",
            params![path.to_string_lossy()],
        );
    }

    pub fn clear_recent(&self) {
        let _ = self.conn.lock().execute("DELETE FROM recent", []);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_are_ordered_and_capped() {
        let s = Settings::in_memory();
        for i in 0..40 {
            s.add_recent(Path::new(&format!("/tmp/{i}.pdf")));
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let r = s.recent();
        assert_eq!(r.len(), MAX_RECENT as usize);
        assert_eq!(r[0].path, PathBuf::from("/tmp/39.pdf"));
        s.remove_recent(Path::new("/tmp/39.pdf"));
        assert_eq!(s.recent()[0].path, PathBuf::from("/tmp/38.pdf"));
        s.clear_recent();
        assert!(s.recent().is_empty());
    }

    #[test]
    fn ui_settings_round_trip() {
        let s = Settings::in_memory();
        assert_eq!(s.ui(), serde_json::json!({}));
        s.set_ui(&serde_json::json!({"theme": "dark"})).unwrap();
        assert_eq!(s.ui()["theme"], "dark");
        assert!(s.set_ui(&serde_json::json!(5)).is_err());
    }
}
