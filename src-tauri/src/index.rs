use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection};

use crate::model::{Conversation, PreviewMessage};

const MAX_CHARS: usize = 2000;
const SNIPPET_RADIUS: usize = 48;

pub struct Hit {
    pub session_id: String,
    pub seq: i64,
    pub role: String,
    pub snippet: String,
}

pub fn open(path: &Path) -> Option<Connection> {
    let conn = Connection::open(path).ok()?;
    let _ = conn.busy_timeout(Duration::from_secs(10));
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    let _ = conn.pragma_update(None, "synchronous", "NORMAL");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS indexed(
             session_id TEXT PRIMARY KEY, size INTEGER, mtime_ms INTEGER, messages INTEGER);
         CREATE TABLE IF NOT EXISTS messages(
             id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
             seq INTEGER, role TEXT, text TEXT);
         CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);",
    )
    .ok()?;
    Some(conn)
}



/// Refresh the message index for the scanned conversations; unchanged files are
/// skipped by (size, mtime) so a rescan of 600 MB stays cheap.
pub fn sync(conn: &Connection, rows: &[Conversation], extract: &dyn Fn(&Conversation) -> Vec<PreviewMessage>) -> usize {
    let mut changed = 0;
    let mut current: HashSet<&str> = HashSet::new();

    for c in rows.iter().filter(|c| c.previewable) {
        current.insert(c.id.as_str());
        let known: Option<(i64, i64)> = conn
            .query_row("SELECT size, mtime_ms FROM indexed WHERE session_id = ?1", [&c.id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .ok();
        if let Some((size, mtime)) = known {
            if size == c.size as i64 && mtime == c.mtime_ms {
                continue;
            }
        }
        let messages = extract(c);
        let _ = conn.execute("DELETE FROM messages WHERE session_id = ?1", [&c.id]);
        let _ = conn.execute("DELETE FROM indexed WHERE session_id = ?1", [&c.id]);
        {
            let Ok(mut stmt) = conn.prepare("INSERT INTO messages(session_id, seq, role, text) VALUES(?1, ?2, ?3, ?4)") else {
                continue;
            };
            for (i, m) in messages.iter().enumerate() {
                let text: String = m.text.chars().take(MAX_CHARS).collect();
                let _ = stmt.execute(params![c.id, i as i64 + 1, m.role, text]);
            }
        }
        let _ = conn.execute(
            "INSERT INTO indexed(session_id, size, mtime_ms, messages) VALUES(?1, ?2, ?3, ?4)",
            params![c.id, c.size as i64, c.mtime_ms, messages.len() as i64],
        );
        changed += 1;
    }

    let stale: Vec<String> = {
        let Ok(mut stmt) = conn.prepare("SELECT session_id FROM indexed") else { return changed };
        let Ok(iter) = stmt.query_map([], |r| r.get::<_, String>(0)) else { return changed };
        iter.filter_map(Result::ok).filter(|id| !current.contains(id.as_str())).collect()
    };
    for id in stale {
        let _ = conn.execute("DELETE FROM messages WHERE session_id = ?1", [&id]);
        let _ = conn.execute("DELETE FROM indexed WHERE session_id = ?1", [&id]);
    }
    changed
}

fn snippet(text: &str, needle: &str) -> String {
    // Work in chars: byte offsets would split multi-byte CJK characters.
    let chars: Vec<char> = text.chars().collect();
    let lower: String = text.to_lowercase();
    let low = needle.to_lowercase();
    let Some(byte_at) = lower.find(&low) else {
        return chars.iter().take(SNIPPET_RADIUS * 2).collect();
    };
    let at = lower[..byte_at].chars().count();
    let width = low.chars().count();
    let start = at.saturating_sub(SNIPPET_RADIUS);
    let end = (at + width + SNIPPET_RADIUS).min(chars.len());
    let body: String = chars[start..end].iter().collect::<String>().replace('\n', " ");
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(&body);
    if end < chars.len() {
        out.push('…');
    }
    out
}

pub fn search(data_dir: &Path, q: &str, limit: usize) -> Vec<Hit> {
    // Reuse the writer open path: a read-only handle cannot attach to a WAL db.
    let Some(conn) = open(&data_dir.join("index.db")) else { return Vec::new() };
    let escaped = q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    let pattern = format!("%{}%", escaped);
    let Ok(mut stmt) = conn.prepare(
        "SELECT session_id, seq, role, text FROM messages
         WHERE text LIKE ?1 ESCAPE '\\' ORDER BY id DESC LIMIT ?2",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map(params![pattern, limit as i64], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
    }) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok)
        .map(|(session_id, seq, role, text)| Hit { snippet: snippet(&text, q), session_id, seq, role })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run against a real index: set AGENT_HUB_TEST_DB to the index.db path.
    #[test]
    fn search_finds_and_bounds_hits() {
        let Ok(path) = std::env::var("AGENT_HUB_TEST_DB") else { return };
        let dir = Path::new(&path).parent().unwrap();
        let started = std::time::Instant::now();
        let hits = search(dir, "Tauri", 400);
        let elapsed = started.elapsed();
        println!("hits={} elapsed={:?}", hits.len(), elapsed);
        assert!(elapsed.as_secs() < 5, "search must not block: {:?}", elapsed);
        assert!(!hits.is_empty(), "expected at least one hit for Tauri");
        assert!(hits.len() <= 400);
        assert!(!hits[0].snippet.is_empty());
    }
}