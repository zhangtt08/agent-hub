use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, TimeZone};
use serde_json::Value;

use crate::model::{AgentReport, Conversation};

pub const CODEX: &str = "codex";
pub const DSH: &str = "dsh";
pub const WORKBUDDY: &str = "workbuddy";
pub const OPENCODE: &str = "opencode";
pub const ANTIGRAVITY: &str = "antigravity";
pub const TRAE: &str = "trae";
pub const QODER: &str = "qoder";
pub const ZCODE: &str = "zcode";
pub const CLAUDE: &str = "claude";

pub struct ScanOutput {
    pub agents: Vec<AgentReport>,
    pub conversations: Vec<Conversation>,
    pub duration_ms: u64,
}

/* ---------------------------------------------------------------- paths */

pub fn home() -> PathBuf {
    // AGENT_HUB_HOME lets tests and portable installs point at another profile.
    std::env::var_os("AGENT_HUB_HOME")
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("C:\\"))
}

pub fn roaming() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home)
}

pub fn local() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(home)
}

pub fn agent_home(id: &str) -> PathBuf {
    match id {
        CODEX => home().join(".codex"),
        DSH => home().join(".dsh"),
        WORKBUDDY => home().join(".workbuddy"),
        OPENCODE => home().join(".local").join("share").join("opencode"),
        ANTIGRAVITY => home().join(".gemini").join("antigravity"),
        TRAE => roaming().join("Trae CN"),
        QODER => roaming().join("com.qodercn.app.stable"),
        ZCODE => roaming().join("ZCode"),
        CLAUDE => home().join(".claude"),
        _ => home(),
    }
}

pub fn agent_roots(id: &str) -> Vec<PathBuf> {
    match id {
        CODEX => vec![agent_home(id).join("sessions"), agent_home(id).join("archived_sessions")],
        DSH => vec![agent_home(id).join("sessions")],
        ANTIGRAVITY => vec![agent_home(id).join("conversations")],
        WORKBUDDY => vec![agent_home(id).join("workbuddy.db")],
        OPENCODE => vec![agent_home(id).join("opencode.db")],
        TRAE => vec![agent_home(id).join("ModularData").join("ai-agent")],
        QODER => vec![
            roaming().join("com.qodercn.app.stable"),
            roaming().join("com.qoder.app.stable"),
        ],
        ZCODE => vec![agent_home(id)],
        CLAUDE => vec![agent_home(id).join("projects")],
        _ => vec![],
    }
}

pub fn all_roots() -> Vec<PathBuf> {
    [CODEX, DSH, ANTIGRAVITY, WORKBUDDY, OPENCODE, TRAE, QODER, ZCODE, CLAUDE]
        .iter()
        .flat_map(|id| agent_roots(id))
        .collect()
}

fn meta(id: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    match id {
        CODEX => ("Codex", "OpenAI", "openai", "#0F7A56"),
        DSH => ("DeepSeek Harness", "DeepSeek", "deepseek", "#4E6BD8"),
        WORKBUDDY => ("WorkBuddy", "Tencent", "workbuddy", "#2E6FB7"),
        OPENCODE => ("OpenCode", "OpenCode", "opencode", "#7A4FA0"),
        ANTIGRAVITY => ("Antigravity", "Google", "gemini", "#4A64C4"),
        TRAE => ("Trae", "ByteDance", "trae", "#DA6F35"),
        QODER => ("Qoder", "Alibaba", "qoder", "#B5482F"),
        ZCODE => ("ZCode", "ZCode", "zcode", "#1B1815"),
        CLAUDE => ("Claude Code", "Anthropic", "claude", "#BE5B33"),
        _ => ("Agent", "", "qwen", "#6E665A"),
    }
}

/* ---------------------------------------------------------------- utils */

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn file_ms(md: &fs::Metadata) -> i64 {
    md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub fn iso(ms: i64) -> String {
    match Local.timestamp_millis_opt(ms).single() {
        Some(dt) => dt.to_rfc3339(),
        None => String::new(),
    }
}

pub fn fnv1a(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", h)
}

fn month_day(ms: i64) -> String {
    match Local.timestamp_millis_opt(ms).single() {
        Some(dt) => format!("{}月{}日", chrono::Datelike::month(&dt), chrono::Datelike::day(&dt)),
        None => String::new(),
    }
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>, pred: &dyn Fn(&Path) -> bool) {
    if depth > 6 {
        return;
    }
    let rd = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let ft = match entry.file_type() {
            Ok(f) => f,
            Err(_) => continue,
        };
        if ft.is_dir() {
            walk(&path, depth + 1, out, pred);
        } else if ft.is_file() && pred(&path) {
            out.push(path);
        }
    }
}

fn uuid_from_name(path: &Path) -> String {
    let stem = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let parts: Vec<&str> = stem.split('-').collect();
    if parts.len() >= 5 {
        let tail = &parts[parts.len() - 5..];
        let lens = [8usize, 4, 4, 4, 12];
        let ok = tail.iter().zip(lens.iter()).all(|(p, l)| {
            p.len() == *l && p.chars().all(|c| c.is_ascii_hexdigit())
        });
        if ok {
            return tail.join("-").to_lowercase();
        }
    }
    String::new()
}

fn is_id_like(seg: &str) -> bool {
    let s = seg.trim();
    if s.is_empty() {
        return true;
    }
    if s.starts_with("thread-") {
        return true;
    }
    if s.chars().all(|c| c.is_ascii_digit()) && s.len() <= 4 {
        return true;
    }
    s.len() >= 20 && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn project_from_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let p = Path::new(path.trim_end_matches(['\\', '/']));
    let mut name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    if is_id_like(&name) {
        name = p.parent().and_then(|x| x.file_name()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    }
    if is_id_like(&name) {
        return String::new();
    }
    name
}

fn clean_title(raw: &str, max: usize) -> String {
    let mut out = String::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty()
            || t.starts_with('#')
            || t.starts_with('<')
            || t.starts_with('{')
            || t.starts_with("Here is a list of plugins")
            || t.starts_with("The following is the Codex agent history")
            || t.starts_with("we're continuing")
            || t.starts_with("- ")
        {
            continue;
        }
        out.push_str(t);
        out.push(' ');
    }
    let joined = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() > max {
        let mut s: String = joined.chars().take(max).collect();
        s.push('…');
        s
    } else {
        joined
    }
}

fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|it| {
                it.get("text")
                    .and_then(|t| t.as_str())
                    .or_else(|| it.get("input_text").and_then(|t| t.as_str()))
                    .or_else(|| it.get("output_text").and_then(|t| t.as_str()))
            })
            .collect::<Vec<_>>()
            .join(" "),
        Value::Object(o) => o.get("text").and_then(|t| t.as_str()).unwrap_or_default().to_string(),
        _ => String::new(),
    }
}

fn count_in(data: &[u8], patterns: &[&[u8]], counts: &mut [u64]) {
    for (i, p) in patterns.iter().enumerate() {
        counts[i] += memchr::memmem::find_iter(data, p).count() as u64;
    }
}

fn count_file(path: &Path, patterns: &[&[u8]]) -> std::io::Result<Vec<u64>> {
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; 4 * 1024 * 1024];
    let mut counts = vec![0u64; patterns.len()];
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let mut data = Vec::with_capacity(carry.len() + n);
        data.extend_from_slice(&carry);
        data.extend_from_slice(&buf[..n]);
        count_in(&data, patterns, &mut counts);
        let keep = data.len().saturating_sub(96);
        carry = data[keep..].to_vec();
    }
    Ok(counts)
}

fn zstd_head_and_counts(path: &Path, head_limit: usize, patterns: &[&[u8]]) -> std::io::Result<(String, Vec<u64>)> {
    let file = File::open(path)?;
    let mut decoder = zstd::stream::read::Decoder::new(file)?;
    let mut head: Vec<u8> = Vec::with_capacity(head_limit.min(2 * 1024 * 1024));
    let mut counts = vec![0u64; patterns.len()];
    let mut buf = vec![0u8; 1024 * 1024];
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let n = decoder.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if head.len() < head_limit {
            let take = (head_limit - head.len()).min(n);
            head.extend_from_slice(&buf[..take]);
        }
        let mut data = Vec::with_capacity(carry.len() + n);
        data.extend_from_slice(&carry);
        data.extend_from_slice(&buf[..n]);
        count_in(&data, patterns, &mut counts);
        let keep = data.len().saturating_sub(96);
        carry = data[keep..].to_vec();
    }
    Ok((String::from_utf8_lossy(&head).into_owned(), counts))
}

fn zstd_full_text(path: &Path, limit_bytes: usize) -> std::io::Result<String> {
    let file = File::open(path)?;
    let mut decoder = zstd::stream::read::Decoder::new(file)?;
    let mut out: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = decoder.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if out.len() > limit_bytes {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn open_ro(path: &Path) -> Option<rusqlite::Connection> {
    use rusqlite::OpenFlags;
    rusqlite::Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()
}

/* ---------------------------------------------------------------- codex */

pub fn load_codex_thread_names() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let file = home().join(".codex").join("session_index.jsonl");
    let text = match fs::read_to_string(&file) {
        Ok(t) => t,
        Err(_) => return map,
    };
    for line in text.lines() {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            let id = v.get("id").and_then(|x| x.as_str()).unwrap_or_default().to_lowercase();
            let name = v.get("thread_name").and_then(|x| x.as_str()).unwrap_or_default().trim().to_string();
            if !id.is_empty() && !name.is_empty() {
                map.insert(id, name);
            }
        }
    }
    map
}

fn codex_conversation(path: &Path, in_archive: bool, names: &HashMap<String, String>) -> Option<Conversation> {
    let md = fs::metadata(path).ok()?;
    let size = md.len();
    let mtime_ms = file_ms(&md);
    let is_zst = path.extension().map(|e| e.eq_ignore_ascii_case("zst")).unwrap_or(false);

    let (head, counts) = if is_zst {
        zstd_head_and_counts(path, 1_500_000, &[b"\"role\":\"user\"", b"\"role\":\"assistant\""]).ok()?
    } else {
        let mut f = File::open(path).ok()?;
        let mut buf = vec![0u8; 1_500_000];
        let n = f.read(&mut buf).ok()?;
        let head = String::from_utf8_lossy(&buf[..n]).into_owned();
        let counts = count_file(path, &[b"\"role\":\"user\"", b"\"role\":\"assistant\""]).ok()?;
        (head, counts)
    };

    let mut cwd = String::new();
    let mut sid = String::new();
    let mut created_ms = 0i64;
    let mut title = String::new();

    for line in head.lines() {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("session_meta") => {
                let payload = &v["payload"];
                cwd = payload.get("cwd").and_then(|x| x.as_str()).unwrap_or_default().to_string();
                sid = payload
                    .get("id")
                    .and_then(|x| x.as_str())
                    .or_else(|| payload.get("session_id").and_then(|x| x.as_str()))
                    .unwrap_or_default()
                    .to_lowercase();
                created_ms = v
                    .get("timestamp")
                    .and_then(|x| x.as_str())
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.timestamp_millis())
                    .unwrap_or(0);
            }
            Some("response_item") if title.is_empty() => {
                let payload = &v["payload"];
                if payload.get("type").and_then(|x| x.as_str()) == Some("message")
                    && payload.get("role").and_then(|x| x.as_str()) == Some("user")
                {
                    title = clean_title(&content_text(&payload["content"]), 68);
                }
            }
            _ => {}
        }
    }

    if sid.is_empty() {
        sid = uuid_from_name(path);
    }
    let project = project_from_path(&cwd);
    let indexed = names.get(&sid).cloned().unwrap_or_default();
    let fallback = format!(
        "{} · {}",
        if project.is_empty() { "会话" } else { &project },
        month_day(if mtime_ms > 0 { mtime_ms } else { now_ms() })
    );
    let title = {
        let t = if !indexed.is_empty() { indexed } else { title };
        if t.is_empty() { fallback } else { t }
    };

    Some(Conversation {
        id: fnv1a(&path.to_string_lossy().to_lowercase()),
        agent: CODEX.to_string(),
        path: path.to_string_lossy().to_string(),
        size,
        mtime_ms,
        mtime: iso(mtime_ms),
        created_at: iso(if created_ms > 0 { created_ms } else { mtime_ms }),
        title,
        project: if project.is_empty() { "未识别项目".into() } else { project },
        project_path: cwd,
        messages: counts[0] + counts[1],
        compressed: is_zst,
        in_archive_dir: in_archive,
        archived: false,
        archived_by_user: false,
        readonly: false,
        previewable: true,
    })
}

/* ---------------------------------------------------------------- dsh */

fn dsh_conversation(path: &Path) -> Option<Conversation> {
    let md = fs::metadata(path).ok()?;
    let size = md.len();
    let mtime_ms = file_ms(&md);
    let (head, counts) = zstd_head_and_counts(
        path,
        1_500_000,
        &[b"\"type\":\"user/message\"", b"\"type\":\"assistant/message\""],
    )
    .ok()?;

    let mut cwd = String::new();
    let mut created_ms = 0i64;
    let mut title = String::new();
    let mut session_id = String::new();

    for line in head.lines() {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("session") => {
                session_id = v.get("id").and_then(|x| x.as_str()).unwrap_or_default().to_string();
                cwd = v.get("cwd").and_then(|x| x.as_str()).unwrap_or_default().to_string();
                created_ms = v.get("createdAt").and_then(|x| x.as_i64()).unwrap_or(0);
            }
            Some("session/title") if title.is_empty() => {
                title = v["data"]["title"].as_str().unwrap_or_default().trim().to_string();
                title = clean_title(&title, 68);
            }
            Some("user/message") if title.is_empty() => {
                title = clean_title(&content_text(&v["data"]["content"]), 68);
            }
            _ => {}
        }
    }

    let project = project_from_path(&cwd);
    let fallback = format!(
        "{} · {}",
        if project.is_empty() { "会话" } else { &project },
        month_day(mtime_ms)
    );
    let id_key = if session_id.is_empty() { path.to_string_lossy().to_string() } else { session_id };

    Some(Conversation {
        id: fnv1a(&format!("dsh:{}", id_key).to_lowercase()),
        agent: DSH.to_string(),
        path: path.to_string_lossy().to_string(),
        size,
        mtime_ms,
        mtime: iso(mtime_ms),
        created_at: iso(if created_ms > 0 { created_ms } else { mtime_ms }),
        title: if title.is_empty() { fallback } else { title },
        project: if project.is_empty() { "未识别项目".into() } else { project },
        project_path: cwd,
        messages: counts[0] + counts[1],
        compressed: true,
        in_archive_dir: false,
        archived: false,
        archived_by_user: false,
        readonly: false,
        previewable: true,
    })
}

/* ------------------------------------------------------------ antigravity */

fn antigravity_rows() -> Vec<Conversation> {
    let base = agent_home(ANTIGRAVITY);
    let dir = base.join("conversations");
    let mut out = Vec::new();
    let mut summaries: HashMap<String, (String, u64, String, String)> = HashMap::new();

    if let Some(conn) = open_ro(&base.join("conversation_summaries.db")) {
        if let Ok(mut stmt) = conn.prepare(
            "SELECT conversation_id, COALESCE(title,''), COALESCE(step_count,0), \
             COALESCE(workspace_uris,''), COALESCE(preview,'') FROM conversation_summaries",
        ) {
            let iter = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)? as u64,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            });
            if let Ok(rows) = iter {
                for r in rows.flatten() {
                    summaries.insert(r.0, (r.1, r.2, r.3, r.4));
                }
            }
        }
    }

    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.extension().map(|e| e.eq_ignore_ascii_case("db")).unwrap_or(false) {
            continue;
        }
        let md = match fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let key = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let (title, steps, workspace, preview) = summaries
            .get(&key)
            .cloned()
            .unwrap_or_default();

        let mut size = md.len();
        for ext in ["-wal", "-shm"] {
            if let Ok(m) = fs::metadata(format!("{}{}", path.to_string_lossy(), ext)) {
                size += m.len();
            }
        }
        let project = workspace
            .trim_matches(['[', ']', '"'])
            .split("\",")
            .next()
            .map(|s| s.trim_matches('"').to_string())
            .filter(|s| !s.is_empty())
            .map(|s| project_from_path(&s.replace("file:///", "")))
            .unwrap_or_default();
        let mtime_ms = file_ms(&md);
        // Antigravity often records no workspace; its preview text is the best title.
        let title = if !title.trim().is_empty() {
            title
        } else if !preview.trim().is_empty() {
            preview.chars().take(60).collect::<String>()
        } else {
            format!("会话 {}", &key.chars().take(8).collect::<String>())
        };

        out.push(Conversation {
            id: fnv1a(&path.to_string_lossy().to_lowercase()),
            agent: ANTIGRAVITY.to_string(),
            path: path.to_string_lossy().to_string(),
            size,
            mtime_ms,
            mtime: iso(mtime_ms),
            created_at: iso(mtime_ms),
            title,
            project: if project.is_empty() { "未识别项目".into() } else { project },
            project_path: workspace,
            messages: steps,
            compressed: false,
            in_archive_dir: false,
            archived: false,
            archived_by_user: false,
            readonly: false,
            previewable: false,
        });
    }
    out
}

/* -------------------------------------------------------------- workbuddy */

fn workbuddy_rows() -> Vec<Conversation> {
    let db = agent_home(WORKBUDDY).join("workbuddy.db");
    let conn = match open_ro(&db) {
        Some(c) => c,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    let sql = "SELECT id, COALESCE(cwd,''), COALESCE(custom_title,title,''), COALESCE(created_at,0), COALESCE(updated_at,0), COALESCE(status,'') FROM sessions WHERE deleted_at IS NULL";
    if let Ok(mut stmt) = conn.prepare(sql) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
            ))
        }) {
            for r in rows.flatten() {
                let project = project_from_path(&r.1);
                out.push(Conversation {
                    id: fnv1a(&format!("wb:{}", r.0).to_lowercase()),
                    agent: WORKBUDDY.to_string(),
                    path: format!("{}#{}", db.to_string_lossy(), r.0),
                    size: 0,
                    mtime_ms: r.4,
                    mtime: iso(r.4),
                    created_at: iso(r.3),
                    title: if r.2.trim().is_empty() {
                        format!("{} · {}", if project.is_empty() { "会话" } else { &project }, month_day(r.4))
                    } else {
                        r.2
                    },
                    project: if project.is_empty() { "未识别项目".into() } else { project },
                    project_path: r.1,
                    messages: 0,
                    compressed: false,
                    in_archive_dir: r.5 == "archived",
                    archived: false,
                    archived_by_user: false,
                    readonly: true,
                    previewable: false,
                });
            }
        }
    }
    out
}

/* ---------------------------------------------------------------- opencode */

fn opencode_rows() -> Vec<Conversation> {
    let db = agent_home(OPENCODE).join("opencode.db");
    let conn = match open_ro(&db) {
        Some(c) => c,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    let sql = "SELECT s.id, COALESCE(s.title,''), COALESCE(s.directory,''), COALESCE(s.time_created,0), COALESCE(s.time_updated,0), \
               (SELECT COUNT(*) FROM session_message m WHERE m.session_id = s.id AND m.type IN ('user','assistant')) \
               FROM session_v2 s";
    if let Ok(mut stmt) = conn.prepare(sql) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)? as u64,
            ))
        }) {
            for r in rows.flatten() {
                let project = project_from_path(&r.2);
                out.push(Conversation {
                    id: fnv1a(&format!("oc:{}", r.0).to_lowercase()),
                    agent: OPENCODE.to_string(),
                    path: format!("{}#{}", db.to_string_lossy(), r.0),
                    size: 0,
                    mtime_ms: r.4,
                    mtime: iso(r.4),
                    created_at: iso(r.3),
                    title: if r.1.trim().is_empty() {
                        format!("{} · {}", if project.is_empty() { "会话" } else { &project }, month_day(r.4))
                    } else {
                        r.1
                    },
                    project: if project.is_empty() { "未识别项目".into() } else { project },
                    project_path: r.2,
                    messages: r.5,
                    compressed: false,
                    in_archive_dir: false,
                    archived: false,
                    archived_by_user: false,
                    readonly: true,
                    previewable: false,
                });
            }
        }
    }
    out
}

/* ---------------------------------------------------------------- qoder */

/// Qoder CLI keeps Claude-Code-style JSONL sessions under `~/.qoder-cn/projects`
/// (and `~/.qoder` for the international build). Those are real, readable
/// conversations, so they belong to Qoder rather than to auto-discovery.
fn qoder_cli_rows() -> Vec<Conversation> {
    cli_jsonl_rows(QODER, &[home().join(".qoder-cn"), home().join(".qoder")])
}

/// Claude Code keeps the same JSONL layout under ~/.claude/projects.
fn claude_rows() -> Vec<Conversation> {
    cli_jsonl_rows(CLAUDE, &[home().join(".claude")])
}

fn cli_jsonl_rows(agent: &str, bases: &[PathBuf]) -> Vec<Conversation> {
    let mut out = Vec::new();
    for base in bases {
        let projects = base.join("projects");
        if !projects.exists() {
            continue;
        }
        let mut files = Vec::new();
        walk(&projects, 0, &mut files, &|p| p.extension().map(|e| e.eq_ignore_ascii_case("jsonl")).unwrap_or(false));
        for path in files {
            if let Some(row) = cli_jsonl_conversation(agent, &path) {
                out.push(row);
            }
        }
    }
    out
}

fn cli_jsonl_conversation(agent: &str, path: &Path) -> Option<Conversation> {
    let md = fs::metadata(path).ok()?;
    if md.len() == 0 {
        return None;
    }
    let mtime_ms = file_ms(&md);

    let file = File::open(path).ok()?;
    let reader = std::io::BufReader::with_capacity(256 * 1024, file);

    let mut title = String::new();
    let mut cwd = String::new();
    let mut created_ms = 0i64;
    let mut messages = 0u64;

    for line in reader.lines().map_while(Result::ok).take(4000) {
        if !line.starts_with('{') {
            continue;
        }
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if cwd.is_empty() {
            if let Some(c) = v.get("cwd").and_then(|x| x.as_str()) {
                if !c.is_empty() {
                    cwd = c.to_string();
                }
            }
        }
        // Some records only carry the workspace under `workspace-directories`.
        if cwd.is_empty() && v.get("type").and_then(|t| t.as_str()) == Some("workspace-directories") {
            if let Some(list) = v.get("directories").and_then(|d| d.as_array()) {
                if let Some(first) = list.iter().find_map(|d| d.as_str()).filter(|s| !s.is_empty()) {
                    cwd = first.to_string();
                }
            }
        }
        match v.get("type").and_then(|t| t.as_str()) {
            Some("user") => {
                messages += 1;
                if title.is_empty() {
                    let text = content_text(&v["message"]["content"]);
                    title = clean_title(&text, 68);
                }
                if created_ms == 0 {
                    created_ms = parse_iso_ms(v.get("timestamp").and_then(|t| t.as_str()).unwrap_or_default());
                }
            }
            Some("assistant") => messages += 1,
            _ => {}
        }
    }
    if messages == 0 {
        return None;
    }

    let mut project = project_from_path(&cwd);
    // Qoder stores sessions as .../<date>/<8-hex-id>; the date is the useful label.
    if project.len() == 8 && project.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Some(parent) = Path::new(&cwd).parent().and_then(|p| p.file_name()) {
            project = parent.to_string_lossy().to_string();
        }
    }
    let fallback = format!(
        "{} · {}",
        if project.is_empty() { "会话" } else { &project },
        month_day(mtime_ms)
    );
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    Some(Conversation {
        id: fnv1a(&format!("{}:{}", agent, stem).to_lowercase()),
        agent: agent.to_string(),
        path: path.to_string_lossy().to_string(),
        size: md.len(),
        mtime_ms,
        mtime: iso(mtime_ms),
        created_at: iso(if created_ms > 0 { created_ms } else { mtime_ms }),
        title: if title.is_empty() { fallback } else { title },
        project: if project.is_empty() { "未识别项目".into() } else { project },
        project_path: cwd,
        messages,
        compressed: false,
        in_archive_dir: false,
        archived: false,
        archived_by_user: false,
        readonly: false,
        previewable: true,
    })
}

fn parse_iso_ms(text: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(text)
        .map(|d| d.timestamp_millis())
        .unwrap_or(0)
}

fn qoder_rows() -> Vec<Conversation> {
    let mut out = Vec::new();
    for base in [
        roaming().join("com.qodercn.app.stable"),
        roaming().join("com.qoder.app.stable"),
    ] {
        let db = base.join("main.sqlite");
        if !db.exists() {
            continue;
        }
        let conn = match open_ro(&db) {
            Some(c) => c,
            None => continue,
        };
        let sql = "SELECT s.session_id, COALESCE(s.title,''), COALESCE(s.cwd,''), \
                   COALESCE(s.created_at,0), COALESCE(s.updated_at,0), \
                   COALESCE(s.archived,0), COALESCE(s.deleted_at,0), \
                   (SELECT COUNT(*) FROM chat_session_messages m WHERE m.session_id = s.session_id) \
                   FROM chat_sessions s";
        let mut stmt = match conn.prepare(sql) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mapped = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)? as u64,
            ))
        });
        let mut rows = match mapped {
            Ok(r) => r,
            Err(_) => continue,
        };
        while let Some(Ok(r)) = rows.next() {
            let mut project = project_from_path(&r.2);
            // Qoder stores each session in .../<date>/<8-hex-id>; the date is the useful label.
            if project.len() == 8 && project.chars().all(|c| c.is_ascii_hexdigit()) {
                if let Some(parent) = Path::new(&r.2).parent().and_then(|p| p.file_name()) {
                    project = parent.to_string_lossy().to_string();
                }
            }
            let archived = r.5 == 1 || r.6 > 0;
            out.push(Conversation {
                id: fnv1a(&format!("qoder:{}:{}", db.to_string_lossy(), r.0).to_lowercase()),
                agent: QODER.to_string(),
                path: format!("{}#{}", db.to_string_lossy(), r.0),
                size: 0,
                mtime_ms: r.4,
                mtime: iso(r.4),
                created_at: iso(r.3),
                title: if r.1.trim().is_empty() {
                    format!(
                        "{} · {}",
                        if project.is_empty() { "会话" } else { &project },
                        month_day(r.4)
                    )
                } else {
                    r.1
                },
                project: if project.is_empty() { "未识别项目".into() } else { project },
                project_path: r.2,
                messages: r.7,
                compressed: false,
                in_archive_dir: archived,
                archived: false,
                archived_by_user: false,
                readonly: true,
                previewable: false,
            });
        }
    }
    out
}

/* ---------------------------------------------------------------- scan */

pub fn scan(progress: &mut dyn FnMut(usize, usize)) -> ScanOutput {
    let started = Instant::now();
    let names = load_codex_thread_names();

    let codex_base = agent_home(CODEX);
    let mut codex_files: Vec<(PathBuf, bool)> = Vec::new();
    let mut sessions_files = Vec::new();
    walk(
        &codex_base.join("sessions"),
        0,
        &mut sessions_files,
        &|p| {
            let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            name.starts_with("rollout-") && (name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"))
        },
    );
    let mut archived_files = Vec::new();
    walk(
        &codex_base.join("archived_sessions"),
        0,
        &mut archived_files,
        &|p| {
            let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            name.starts_with("rollout-") && name.ends_with(".jsonl")
        },
    );
    for p in sessions_files {
        codex_files.push((p, false));
    }
    for p in archived_files {
        codex_files.push((p, true));
    }

    let mut dsh_files = Vec::new();
    walk(
        &agent_home(DSH).join("sessions"),
        0,
        &mut dsh_files,
        &|p| p.to_string_lossy().ends_with(".jsonl.zstd"),
    );

    let antigravity = antigravity_rows();

    // counts for progress
    let wb_rows = workbuddy_rows();
    let oc_rows = opencode_rows();
    let qd_rows = { let mut v = qoder_rows(); v.extend(qoder_cli_rows()); v };
    let cl_rows = claude_rows();
    let total = codex_files.len() + dsh_files.len() + antigravity.len() + wb_rows.len() + oc_rows.len() + qd_rows.len() + cl_rows.len();

    let mut rows: Vec<Conversation> = Vec::new();
    let mut done = 0usize;
    progress(done, total);

    for (path, archived) in codex_files.iter() {
        if let Some(row) = codex_conversation(path, *archived, &names) {
            rows.push(row);
        }
        done += 1;
        progress(done, total);
    }
    for path in dsh_files.iter() {
        if let Some(row) = dsh_conversation(path) {
            rows.push(row);
        }
        done += 1;
        progress(done, total);
    }
    for row in antigravity.into_iter() {
        rows.push(row);
        done += 1;
        progress(done, total);
    }
    for row in wb_rows.into_iter() {
        rows.push(row);
        done += 1;
        progress(done, total);
    }
    for row in oc_rows.into_iter() {
        rows.push(row);
        done += 1;
        progress(done, total);
    }
    for row in qd_rows.into_iter() {
        rows.push(row);
        done += 1;
        progress(done, total);
    }
    for row in cl_rows.into_iter() {
        rows.push(row);
        done += 1;
        progress(done, total);
    }

    // Agents without a dedicated adapter: pick up any session-shaped files we
    // can read so they show up (and become deletable) without new code.
    let discovered = crate::discover::discover(1500);

    rows.sort_by(|a, b| b.mtime_ms.cmp(&a.mtime_ms));

    let ids = [CODEX, DSH, WORKBUDDY, OPENCODE, ANTIGRAVITY, TRAE, QODER, ZCODE, CLAUDE];
    let mut agents: Vec<AgentReport> = ids
        .iter()
        .map(|id| {
            let (name, vendor, logo, tint) = meta(id);
            let roots = agent_roots(id);
            let installed = roots.iter().any(|p| p.exists());
            let mine: Vec<&Conversation> = rows.iter().filter(|r| r.agent == *id).collect();
            AgentReport {
                id: id.to_string(),
                name: name.to_string(),
                vendor: vendor.to_string(),
                logo: logo.to_string(),
                tint: tint.to_string(),
                // Qoder now mixes database rows (read-only) with CLI JSONL files
                // (deletable), so derive the badge from the rows themselves.
                mode: {
                    let readonly_rows = mine.iter().filter(|r| r.readonly).count();
                    if mine.is_empty() {
                        match *id {
                            TRAE | ZCODE => "detect",
                            WORKBUDDY | OPENCODE | QODER => "readonly",
                            _ => "file",
                        }
                    } else if readonly_rows == mine.len() {
                        "readonly"
                    } else if readonly_rows == 0 {
                        "file"
                    } else {
                        "mixed"
                    }
                }
                .to_string(),
                installed,
                roots: roots.iter().map(|p| p.to_string_lossy().to_string()).collect(),
                count: mine.len(),
                bytes: mine.iter().map(|r| r.size).sum(),
                messages: mine.iter().map(|r| r.messages).sum(),
                last_sync: mine.iter().map(|r| r.mtime_ms as u64).max().unwrap_or(0),
            }
        })
        .collect();

    for found in discovered {
        let bytes: u64 = found.rows.iter().map(|r| r.size).sum();
        let messages: u64 = found.rows.iter().map(|r| r.messages).sum();
        let last_sync = found.rows.iter().map(|r| r.mtime_ms as u64).max().unwrap_or(0);
        agents.push(AgentReport {
            id: found.id.clone(),
            name: found.name,
            vendor: "自动发现".to_string(),
            logo: "qwen".to_string(),
            tint: "#6E665A".to_string(),
            mode: "file".to_string(),
            installed: true,
            roots: vec![found.root.to_string_lossy().to_string()],
            count: found.rows.len(),
            bytes,
            messages,
            last_sync,
        });
        rows.extend(found.rows);
    }

    rows.sort_by(|a, b| b.mtime_ms.cmp(&a.mtime_ms));

    ScanOutput {
        agents,
        conversations: rows,
        duration_ms: started.elapsed().as_millis() as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    /// Qoder CLI writes Claude-Code-style session JSONL. Those records must be
    /// recognised as Qoder, not left to auto-discovery (that caused the duplicate
    /// "Qoder" / "Qoder-cn" entries).
    #[test]
    fn qoder_cli_session_is_recognised() {
        let dir = std::env::temp_dir().join("agenthub-qoder-cli-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("8b40ef6e-2035-468d-aee0-3f1fa867cfa6.jsonl");
        fs::write(
            &file,
            "{\"type\":\"workspace-directories\",\"sessionId\":\"s1\"}\n\
             {\"type\":\"user\",\"timestamp\":\"2026-10-25T10:00:00+08:00\",\"cwd\":\"C:\\\\work\\\\proj\",\
             \"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"fix the login bug\"}]}}\n\
             {\"type\":\"assistant\",\"timestamp\":\"2026-10-25T10:01:00+08:00\",\
             \"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"done\"}]}}\n",
        )
        .unwrap();

        let row = cli_jsonl_conversation(QODER, &file).expect("session must be recognised");
        assert_eq!(row.agent, QODER);
        assert_eq!(row.messages, 2, "user + assistant counted");
        assert_eq!(row.title, "fix the login bug", "first user text becomes the title");
        assert!(!row.readonly, "Qoder CLI sessions are plain files and can be deleted");
        assert!(row.previewable);
    }

    /// A JSONL file with no messages is not a conversation.
    #[test]
    fn qoder_cli_ignores_metadata_only_file() {
        let dir = std::env::temp_dir().join("agenthub-qoder-cli-test2");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("meta.jsonl");
        fs::write(&file, "{\"type\":\"workspace-directories\"}\n{\"type\":\"runtime-config\"}\n").unwrap();
        assert!(cli_jsonl_conversation(QODER, &file).is_none());
    }

    /// Antigravity stores step_count/workspace in a summary db whose
    /// last_modified_time is a datetime string. Reading it as an integer made
    /// every row fail, and the map silently ended up empty (all sessions showed
    /// 0 messages). This guards the query shape.
    #[test]
    fn antigravity_summary_query_reads_every_row() {
        let dir = std::env::temp_dir().join("agenthub-scan-test");
        let _ = fs::create_dir_all(&dir);
        let db = dir.join("summaries.db");
        let _ = fs::remove_file(&db);
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE conversation_summaries(
                    conversation_id TEXT, title TEXT, preview TEXT, step_count INTEGER,
                    last_modified_time TEXT, workspace_uris TEXT);
                 INSERT INTO conversation_summaries VALUES
                    ('a','', 'first prompt', 339, '2026-10-21 13:22:48.2+00:00', ''),
                    ('b','Named', 'other', 1476, '2026-10-25 02:14:04.3+00:00', '');",
            )
            .unwrap();
        }

        let conn = open_ro(&db).expect("open summary db");
        let mut stmt = conn
            .prepare(
                "SELECT conversation_id, COALESCE(title,''), COALESCE(step_count,0), \
                 COALESCE(workspace_uris,''), COALESCE(preview,'') FROM conversation_summaries",
            )
            .unwrap();
        let rows: Vec<(String, String, u64, String, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)? as u64,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .unwrap()
            .flatten()
            .collect();

        assert_eq!(rows.len(), 2, "every summary row must decode");
        let a = rows.iter().find(|r| r.0 == "a").unwrap();
        assert_eq!(a.2, 339, "step count must survive the round trip");
        assert_eq!(a.4, "first prompt", "preview is the fallback title");
    }
}