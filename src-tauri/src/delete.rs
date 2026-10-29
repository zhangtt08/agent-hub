use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection};

use crate::scan::all_roots;

pub fn inside_roots(path: &Path, roots: &[PathBuf]) -> bool {
    let target = path.to_string_lossy().to_lowercase().replace('/', "\\");
    roots.iter().any(|root| {
        let r = root.to_string_lossy().to_lowercase().replace('/', "\\");
        target == r || target.starts_with(&(r + "\\"))
    })
}

/// Delete a conversation file (plus sqlite companions for antigravity).
/// When `purge` is false the file goes to the recycle bin.
pub fn delete_files(path: &Path, purge: bool) -> Result<u64, String> {
    let mut targets = vec![path.to_path_buf()];
    if path.extension().map(|e| e.eq_ignore_ascii_case("db")).unwrap_or(false) {
        for suffix in ["-wal", "-shm"] {
            let extra = PathBuf::from(format!("{}{}", path.to_string_lossy(), suffix));
            if extra.exists() {
                targets.push(extra);
            }
        }
    }

    let mut freed = 0u64;
    for target in targets {
        let md = match fs::metadata(&target) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let size = md.len();
        let result = if purge {
            fs::remove_file(&target)
        } else {
            trash::delete(&target).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        };
        match result {
            Ok(_) => freed += size,
            Err(err) => return Err(format!("{}: {}", target.to_string_lossy(), err)),
        }
    }
    Ok(freed)
}

/// Rows that hold a single session inside an agent's own database.
/// Each entry is (table, column, also_delete_children).
fn db_targets(agent: &str) -> Vec<(&'static str, &'static str)> {
    match agent {
        "workbuddy" => vec![
            ("session_usage", "session_id"),
            ("automation_runs", "thread_id"),
        ],
        "opencode" => vec![
            ("session_message", "session_id"),
            ("session_inbox", "session_id"),
            ("session_pending", "session_id"),
            ("instruction_entry", "session_id"),
            ("instruction_state", "session_id"),
        ],
        _ => Vec::new(),
    }
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> bool {
    let sql = format!("PRAGMA table_info(\"{}\")", table.replace('"', "\"\""));
    match conn.prepare(&sql) {
        Ok(mut stmt) => {
            let names: Vec<String> = stmt
                .query_map([], |r| r.get::<_, String>(1))
                .map(|rows| rows.filter_map(Result::ok).collect())
                .unwrap_or_default();
            names.iter().any(|n| n.eq_ignore_ascii_case(column))
        }
        Err(_) => false,
    }
}

/// Delete one session from the agent's sqlite database.
/// Returns the bytes reclaimed on disk after checkpoint (and VACUUM when purging).
pub fn delete_db_session(agent: &str, db: &Path, session_id: &str, _vacuum: bool) -> Result<u64, String> {
    let before = fs::metadata(db).map(|m| m.len()).unwrap_or(0);
    let conn = Connection::open(db).map_err(|e| format!("打开数据库失败：{e}"))?;
    // A running agent may hold the lock; wait instead of failing instantly.
    let _ = conn.busy_timeout(Duration::from_secs(20));

    let tx = conn.unchecked_transaction().map_err(|e| format!("事务开启失败：{e}"))?;
    let mut removed = 0usize;

    // Explicit tables first, then the agent's own session table.
    for (table, column) in db_targets(agent) {
        if !table_has_column(&conn, table, column) {
            continue;
        }
        let sql = format!("DELETE FROM \"{}\" WHERE {} = ?1", table, column);
        if let Ok(n) = tx.execute(&sql, params![session_id]) {
            removed += n;
        }
    }

    match agent {
        "workbuddy" => {
            removed += tx.execute("DELETE FROM sessions WHERE id = ?1", params![session_id]).unwrap_or(0);
        }
        "opencode" => {
            removed += tx
                .execute("DELETE FROM session_v2 WHERE id = ?1 OR parent_id = ?1", params![session_id])
                .unwrap_or(0);
        }
        "qoder" => {
            // Qoder spreads one session over ~20 tables, all keyed by session_id.
            let tables: Vec<String> = {
                let mut stmt = tx
                    .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
                    .map_err(|e| format!("读取表结构失败：{e}"))?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
                rows.filter_map(Result::ok).collect()
            };
            for table in tables {
                if !table_has_column(&tx, &table, "session_id") {
                    continue;
                }
                let sql = format!("DELETE FROM \"{}\" WHERE session_id = ?1", table.replace('"', "\"\""));
                if let Ok(n) = tx.execute(&sql, params![session_id]) {
                    removed += n;
                }
            }
            removed += tx.execute("DELETE FROM chat_sessions WHERE session_id = ?1", params![session_id]).unwrap_or(0);
        }
        _ => {}
    }

    if removed == 0 {
        return Err("数据库里没有找到这条会话，可能已在原应用中删除".into());
    }
    tx.commit().map_err(|e| format!("提交失败：{e}"))?;

    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    drop(conn);

    let after = fs::metadata(db).map(|m| m.len()).unwrap_or(before);
    Ok(before.saturating_sub(after))
}

/// Last-resort deletion: clear read-only/system attributes, retry, and drop
/// sqlite sidecars so the agent cannot resurrect the row from a WAL file.
pub fn force_delete_files(path: &Path) -> Result<u64, String> {
    let mut freed = 0u64;
    let mut targets = vec![path.to_path_buf()];
    if path.extension().map(|e| e.eq_ignore_ascii_case("db")).unwrap_or(false) {
        for suffix in ["-wal", "-shm"] {
            let extra = PathBuf::from(format!("{}{}", path.to_string_lossy(), suffix));
            if extra.exists() {
                targets.push(extra);
            }
        }
    }
    for target in targets {
        let size = fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
        if let Ok(md) = fs::metadata(&target) {
            let mut perms = md.permissions();
            if perms.readonly() {
                perms.set_readonly(false);
                let _ = fs::set_permissions(&target, perms);
            }
        }
        let mut last = String::new();
        for attempt in 0..3 {
            match fs::remove_file(&target) {
                Ok(_) => { freed += size; last.clear(); break; }
                Err(err) => {
                    last = err.to_string();
                    if attempt == 0 {
                        let _ = std::process::Command::new("attrib")
                            .args(["-r", "-s", "-h", &target.to_string_lossy()])
                            .output();
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(120));
                    }
                }
            }
        }
        if !last.is_empty() {
            return Err(format!("{}: {}", target.to_string_lossy(), last));
        }
    }
    Ok(freed)
}

/// Reclaim the space a batch of row deletions left behind. VACUUM rewrites the
/// whole database, so it runs once per file after all rows are gone.
pub fn vacuum(db: &Path) -> u64 {
    let before = fs::metadata(db).map(|m| m.len()).unwrap_or(0);
    if let Ok(conn) = Connection::open(db) {
        let _ = conn.busy_timeout(Duration::from_secs(30));
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;");
    }
    let after = fs::metadata(db).map(|m| m.len()).unwrap_or(before);
    before.saturating_sub(after)
}

pub fn guard_ok(path: &str) -> bool {
    inside_roots(Path::new(path), &all_roots())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vacuum_reclaims_freed_pages() {
        let path = sandbox_db("vacuum-test.db");
        let _ = fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE payload(id INTEGER PRIMARY KEY, blob BLOB);").unwrap();
            let big = vec![7u8; 4096];
            for i in 0..400 {
                conn.execute("INSERT INTO payload(id, blob) VALUES(?1, ?2)", params![i, big]).unwrap();
            }
            conn.execute("DELETE FROM payload WHERE id < 380", []).unwrap();
        }
        let before = fs::metadata(&path).unwrap().len();
        let freed = vacuum(&path);
        let after = fs::metadata(&path).unwrap().len();
        assert!(after < before, "vacuum must shrink the file: {} -> {}", before, after);
        assert_eq!(freed, before - after, "reported bytes must match the shrink");
    }

    /// Qoder spreads one session over many tables keyed by session_id.
    #[test]
    fn qoder_session_delete_clears_every_table() {
        let path = sandbox_db("qoder-test.db");
        let _ = fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE chat_sessions(session_id TEXT PRIMARY KEY, title TEXT);
                 CREATE TABLE chat_session_messages(session_id TEXT, message_id TEXT);
                 CREATE TABLE chat_session_highlights(session_id TEXT, title TEXT);
                 CREATE TABLE chat_session_search_segments(session_id TEXT, text TEXT);
                 CREATE TABLE unrelated(setting_key TEXT PRIMARY KEY, value TEXT);
                 INSERT INTO chat_sessions VALUES('a','keep'), ('b','drop');
                 INSERT INTO chat_session_messages VALUES('a','m1'), ('b','m2'), ('b','m3');
                 INSERT INTO chat_session_highlights VALUES('a','h1'), ('b','h2');
                 INSERT INTO chat_session_search_segments VALUES('a','t1'), ('b','t2');
                 INSERT INTO unrelated VALUES('theme','dark');",
            )
            .unwrap();
        }

        delete_db_session("qoder", &path, "b", true).expect("qoder delete should succeed");

        let conn = Connection::open(&path).unwrap();
        for table in ["chat_sessions", "chat_session_messages", "chat_session_highlights", "chat_session_search_segments"] {
            let left: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {} WHERE session_id = 'b'", table), [], |r| r.get(0))
                .unwrap();
            assert_eq!(left, 0, "{} still has rows for the deleted session", table);
        }
        let kept: i64 = conn.query_row("SELECT COUNT(*) FROM chat_sessions WHERE session_id = 'a'", [], |r| r.get(0)).unwrap();
        assert_eq!(kept, 1, "other sessions must survive");
        let settings: String = conn.query_row("SELECT value FROM unrelated", [], |r| r.get(0)).unwrap();
        assert_eq!(settings, "dark", "tables without session_id are untouched");
    }

    fn sandbox_db(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("agenthub-delete-test");
        let _ = fs::create_dir_all(&dir);
        dir.join(name)
    }

    /// WorkBuddy layout: one row in `sessions`, usage rows keyed by session_id.
    #[test]
    fn db_session_delete_removes_every_row() {
        let path = sandbox_db("wb-test.db");
        let _ = fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions(id TEXT PRIMARY KEY, updated_at INTEGER, deleted_at INTEGER);
                 CREATE TABLE session_usage(session_id TEXT, used INTEGER);
                 INSERT INTO sessions VALUES('keep', 1, NULL), ('drop', 2, NULL);
                 INSERT INTO session_usage VALUES('keep', 1), ('drop', 2);",
            )
            .unwrap();
        }

        delete_db_session("workbuddy", &path, "drop", true).expect("delete should succeed");

        let conn = Connection::open(&path).unwrap();
        let sessions: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
        let usage: i64 = conn.query_row("SELECT COUNT(*) FROM session_usage", [], |r| r.get(0)).unwrap();
        assert_eq!(sessions, 1, "only the kept session remains");
        assert_eq!(usage, 1, "child rows are removed with the session");
        let kept: String = conn.query_row("SELECT id FROM sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(kept, "keep");
    }

    #[test]
    fn db_session_delete_reports_missing_rows() {
        let path = sandbox_db("wb-missing.db");
        let _ = fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE sessions(id TEXT PRIMARY KEY, updated_at INTEGER, deleted_at INTEGER);").unwrap();
        }
        assert!(delete_db_session("workbuddy", &path, "nope", true).is_err());
    }

    #[test]
    fn force_delete_clears_readonly_and_removes_file() {
        let path = sandbox_db("force-me.txt");
        fs::write(&path, b"x").unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&path, perms).unwrap();

        force_delete_files(&path).expect("force delete should succeed");
        assert!(!path.exists(), "file must be gone even when read-only");
    }
}