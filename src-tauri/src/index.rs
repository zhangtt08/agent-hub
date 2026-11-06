use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection};

use crate::model::{Conversation, PreviewMessage};

const MAX_CHARS: usize = 2000;
const SNIPPET_RADIUS: usize = 48;
const PREVIEW_CHARS: usize = 220;
const SCHEMA_VERSION: i64 = 3;

pub struct Hit {
    pub session_id: String,
    pub seq: i64,
    pub role: String,
    pub snippet: String,
}

/// 会话列表里的最近一轮问答摘要。只保留一行的短文本，不把完整正文塞进 state。
#[derive(Clone, Debug, Default)]
pub struct Snippet {
    pub user: String,
    pub assistant: String,
}

pub fn compact(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    let mut pending_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() || ch.is_control() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            count += 1;
            pending_space = false;
        }
        out.push(ch);
        count += 1;
        if count >= max {
            out.push('…');
            break;
        }
    }
    out
}

/// Harness 会把运行环境、提醒、技能清单等作为「用户消息」写进会话文件。
/// 这些不是用户真正说过的话，放进列表只会让每条会话看起来一模一样。
pub fn is_injected_user_text(text: &str) -> bool {
    let t = text.trim_start();
    // Codex 的 goal 续跑提示是一整段英文模板；只要开头命中就不再当作用户提问。
    if t.starts_with("A goal has been set.") || t.starts_with("The objective below is user-provided") {
        return true;
    }
    [
        "Current runtime context",
        "Current working directory:",
        "Continue working toward the active thread goal",
        "The objective below is user-provided data",
        "This session is being continued",
        "Time sampled while preparing turn",
        "[Request interrupted by user]",
        "[SYSTEM NOTIFICATION",
        "<task-notification",
        "<codex_internal_context",
        "<untrusted_objective>",
        "<environment_context>",
        "<system-reminder>",
        "system-reminder",
        "# AGENTS.md",
        "Here is a list of plugins",
        "The following is the Codex agent history",
        "we're continuing",
    ]
    .iter()
    .any(|prefix| t.starts_with(prefix))
}

/// 每个会话取最近一条真正的用户提问和最近一条 Agent 回复。
/// 列表需要的是「这是什么对话」而不是完整正文，因此只查询几行候选摘要。
pub fn latest_snippets(conn: &Connection) -> HashMap<String, Snippet> {
    let mut out = HashMap::new();
    let Ok(mut stmt) = conn.prepare(
        "SELECT session_id, role, text FROM (
             SELECT session_id, role, text,
                    ROW_NUMBER() OVER (PARTITION BY session_id, role ORDER BY seq DESC) AS rn
             FROM messages
             WHERE role IN ('user', 'assistant')
         ) WHERE rn <= 8
         ORDER BY session_id, role, rn",
    ) else {
        return out;
    };
    let Ok(rows) = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    }) else {
        return out;
    };
    for (id, role, text) in rows.flatten() {
        let item = out.entry(id).or_insert_with(Snippet::default);
        if role == "user" {
            if item.user.is_empty() && !is_injected_user_text(&text) {
                item.user = compact(&text, PREVIEW_CHARS);
            }
        } else if item.assistant.is_empty() {
            item.assistant = compact(&text, PREVIEW_CHARS);
        }
    }
    out
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
    // 解析规则升级后必须让旧索引失效；否则未变化的文件会被 (size, mtime)
    // 跳过，列表里仍会残留 reasoning / tool-call 过程消息。
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap_or(0);
    if version < SCHEMA_VERSION {
        let _ = conn.execute_batch("DELETE FROM messages; DELETE FROM indexed;");
        let _ = conn.pragma_update(None, "user_version", SCHEMA_VERSION);
    }
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

    #[test]
    fn latest_snippets_returns_last_message_per_role() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE messages(
                 id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
                 seq INTEGER, role TEXT, text TEXT);",
        )
        .unwrap();
        for (seq, role, text) in [
            (1, "user", "old question"),
            (2, "assistant", "old answer"),
            (3, "user", "new question\nwith a line break"),
            (4, "assistant", "new answer"),
        ] {
            conn.execute(
                "INSERT INTO messages(session_id, seq, role, text) VALUES('s1', ?1, ?2, ?3)",
                params![seq, role, text],
            )
            .unwrap();
        }
        let snippets = latest_snippets(&conn);
        let item = snippets.get("s1").expect("session snippet");
        assert_eq!(item.user, "new question with a line break");
        assert_eq!(item.assistant, "new answer");
    }

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