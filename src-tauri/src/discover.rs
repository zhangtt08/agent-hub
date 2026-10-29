//! Auto-discovery for agents the app does not have a dedicated adapter for.
//!
//! A dedicated adapter (scan.rs) knows one product's exact layout. This module
//! covers everything else: it looks in the usual per-user data folders for
//! session-shaped files and, when it finds a readable one, turns it into normal
//! `Conversation` rows. Those rows then flow into the same cleanup buckets and
//! delete paths as everything else, so a new agent shows up without new code.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::model::Conversation;
use crate::scan::{fnv1a, home, iso, local, now_ms, roaming};

/// Folder names that are never an agent's session store.
const SKIP_DIRS: &[&str] = &[
    "cache", "caches", "logs", "log", "crashpad", "crashreports", "gpu", "gpucache", "dawncache",
    "dawnwebgpu", "dawnwebgraphite", "code cache", "code_cache", "blob_storage", "service worker",
    "node_modules", "dictionaries", "updates", "installer", "installers", "temp", "tmp",
];

/// Roots we already have a real adapter for; auto-discovery must not double count.
fn owned_by_adapter(path: &Path) -> bool {
    let p = path.to_string_lossy().to_lowercase().replace('/', "\\");
    let home = home().to_string_lossy().to_lowercase().replace('/', "\\");
    let roaming = roaming().to_string_lossy().to_lowercase().replace('/', "\\");
    let owned = [
        format!("{home}\\.codex"),
        format!("{home}\\.dsh"),
        format!("{home}\\.workbuddy"),
        format!("{home}\\.local\\share\\opencode"),
        format!("{home}\\.gemini\\antigravity"),
        format!("{roaming}\\trae cn"),
        format!("{roaming}\\com.qodercn.app.stable"),
        format!("{roaming}\\com.qoder.app.stable"),
        format!("{roaming}\\zcode"),
        // Qoder CLI session trees now have a dedicated adapter.
        format!("{home}\\.qoder-cn"),
        format!("{home}\\.qoder"),
        // Claude Code has one too.
        format!("{home}\\.claude"),
    ];
    owned.iter().any(|o| p == *o || p.starts_with(&(o.clone() + "\\")))
}

pub struct Discovered {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    pub rows: Vec<Conversation>,
}

/// Best-effort human name for a folder like `com.acme.agent.stable` or `AcmeAI`.
fn display_name(dir: &Path) -> String {
    let raw = dir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let lowered = raw.to_lowercase();
    // `com.vendor.product.stable` / `ai.vendor.app` -> product part
    let core = if lowered.contains('.') {
        let parts: Vec<&str> = lowered.split('.').filter(|p| !p.is_empty()).collect();
        let drop: &[&str] = &["com", "cn", "ai", "app", "stable", "beta", "insider", "desktop", "electron", "client"];
        parts
            .iter()
            .rev()
            .find(|p| !drop.contains(&p.to_lowercase().as_str()))
            .copied()
            .unwrap_or("agent")
            .to_string()
    } else {
        lowered.clone()
    };
    let mut chars = core.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Agent".to_string(),
    }
}

/// A session file must look like JSON lines with at least one message.
fn looks_like_session(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
    if !matches!(ext.as_str(), "jsonl" | "json") {
        return false;
    }
    let md = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if md.len() == 0 {
        return false;
    }
    // Peek at the first 64 KB only: session stores can be large.
    let mut buf = vec![0u8; 64 * 1024];
    let read = match File::open(path).and_then(|mut f| f.read(&mut buf)) {
        Ok(n) => n,
        Err(_) => return false,
    };
    let text = String::from_utf8_lossy(&buf[..read]);
    let mut messages = 0;
    for line in text.lines().take(200) {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        if line.contains("\"role\"") || line.contains("\"content\"") || line.contains("\"message\"") {
            messages += 1;
        }
    }
    messages > 0
}

fn collect_files(root: &Path, depth: usize, out: &mut Vec<PathBuf>, budget: &mut usize) {
    // `budget` counts directory visits, not just hits: AppData can hold 50k+
    // folders and walking all of them costs seconds on every rescan.
    if depth > 4 || *budget == 0 {
        return;
    }
    *budget -= 1;
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        if *budget == 0 {
            return;
        }
        let path = entry.path();
        let name = path.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        if path.is_dir() {
            if SKIP_DIRS.iter().any(|d| name.contains(d)) {
                continue;
            }
            collect_files(&path, depth + 1, out, budget);
        } else if out.len() < 60 && looks_like_session(&path) {
            out.push(path);
        }
    }
}

fn count_messages(path: &Path) -> u64 {
    let Ok(file) = File::open(path) else { return 0 };
    let mut n = 0u64;
    for line in BufReader::new(file).lines().map_while(Result::ok).take(20_000) {
        let l = line.trim();
        if l.starts_with('{') && (l.contains("\"role\"") || l.contains("\"content\"")) {
            n += 1;
        }
    }
    n
}

/// Title from the first user-looking message, so discovered sessions are named.
fn title_from_file(path: &Path) -> String {
    let Ok(mut file) = File::open(path) else { return String::new() };
    let mut text = String::new();
    let _ = file.seek(SeekFrom::Start(0));
    let _ = file.take(96 * 1024).read_to_string(&mut text);
    for line in text.lines().take(200) {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { continue };
        let role = v
            .get("role")
            .or_else(|| v.get("type"))
            .and_then(|r| r.as_str())
            .unwrap_or_default();
        if role != "user" && role != "human" {
            continue;
        }
        let raw = v
            .get("content")
            .map(|c| match c {
                Value::String(s) => s.clone(),
                Value::Array(items) => items
                    .iter()
                    .filter_map(|i| i.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join(" "),
                _ => String::new(),
            })
            .unwrap_or_default();
        let cleaned = raw.trim().replace('\n', " ");
        if cleaned.len() > 4 {
            return cleaned.chars().take(60).collect();
        }
    }
    String::new()
}

/// Folder names that are user data, not an agent's session store. Discovery
/// must stay conservative: indexing a random folder of JSON would let a user
/// delete real documents by mistake.
fn is_agentish(dir: &Path) -> bool {
    let name = dir.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    const DENY: &[&str] = &[
        "documents", "desktop", "downloads", "pictures", "music", "videos", "appdata", "programdata",
        "program files", "program files (x86)", "windows", "temp", "tmp", "onedrive", "favorites",
        "links", "contacts", "saved games", "searches", "3d objects", "public", "default",
        "microsoft", "packages", "intel", "nvidia", "amd", "google", "mozilla", "adobe", "java",
    ];
    if DENY.iter().any(|d| name == *d) {
        return false;
    }
    // An agent's folder usually names its product and keeps sessions below it.
    let has_marker = fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|e| {
                let n = e.file_name().to_string_lossy().to_lowercase();
                e.path().is_dir()
                    && ["session", "sessions", "conversations", "chats", "history", "threads", "agent", "agents"]
                        .iter()
                        .any(|m| n.contains(m))
            })
        })
        .unwrap_or(false);
    has_marker
}

/// Scan the usual per-user folders for agents we have no adapter for.
/// `budget` caps how many files we open so a huge profile cannot stall startup.
pub fn discover(budget: usize) -> Vec<Discovered> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    // Dot-folders in the profile (`.acme`, `.config`) are candidates themselves;
    // never Documents/Desktop, and never their children.
    if let Ok(entries) = fs::read_dir(home()) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if path.is_dir() && name.starts_with('.') && !owned_by_adapter(&path) && is_agentish(&path) {
                candidates.push(path);
            }
        }
    }
    for base in [roaming(), local()] {
        let Ok(entries) = fs::read_dir(&base) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = path.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            // Dot-prefixed agent folders (`.acme`, `.config`) are legitimate;
            // is_agentish() decides what counts as a session store.
            if SKIP_DIRS.iter().any(|d| name.contains(d)) {
                continue;
            }
            if owned_by_adapter(&path) || !is_agentish(&path) {
                continue;
            }
            candidates.push(path);
        }
    }

    let mut budget = budget;
    let mut out = Vec::new();
    for root in candidates {
        if budget == 0 {
            break;
        }
        let mut files = Vec::new();
        // Per-root cap keeps one huge folder from eating the whole budget.
        let mut per_root = 400usize;
        collect_files(&root, 0, &mut files, &mut per_root);
        if files.is_empty() {
            continue;
        }
        budget = budget.saturating_sub(files.len());

        let id = format!("auto-{}", &fnv1a(&root.to_string_lossy().to_lowercase())[..8]);
        let name = display_name(&root);
        let mut rows = Vec::new();
        for path in files {
            let md = match fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime_ms = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or_else(now_ms);
            let messages = count_messages(&path);
            if messages == 0 {
                continue;
            }
            let title = title_from_file(&path);
            let project = path
                .parent()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| name.clone());
            rows.push(Conversation {
                id: fnv1a(&path.to_string_lossy().to_lowercase()),
                agent: id.clone(),
                path: path.to_string_lossy().to_string(),
                size: md.len(),
                mtime_ms,
                mtime: iso(mtime_ms),
                created_at: iso(mtime_ms),
                title: if title.is_empty() {
                    format!("{} · {}", project, &fnv1a(&path.to_string_lossy())[..6])
                } else {
                    title
                },
                project,
                project_path: path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                messages,
                compressed: false,
                in_archive_dir: false,
                archived: false,
                archived_by_user: false,
                // Auto-discovered rows are index-only: deleting a file we do not fully
                // understand is exactly how real conversations got lost.
                readonly: true,
                previewable: true,
            });
        }
        if rows.is_empty() {
            continue;
        }
        out.push(Discovered { id, name, root, rows });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn only_agent_shaped_folders_are_candidates() {
        let base = std::env::temp_dir().join("agenthub-discover-guard");
        let _ = fs::remove_dir_all(&base);
        // Looks like an agent: has a sessions subfolder.
        let agent = base.join("com.acme.copilot.stable");
        fs::create_dir_all(agent.join("sessions")).unwrap();
        // Looks like user data: JSON files but no session folder.
        let docs = base.join("Documents");
        fs::create_dir_all(&docs).unwrap();
        fs::write(docs.join("notes.jsonl"), "{\"role\":\"user\",\"content\":\"x\"}\n").unwrap();

        assert!(is_agentish(&agent), "a sessions/ folder marks an agent");
        assert!(!is_agentish(&docs), "plain user documents must be ignored");
    }

    #[test]
    fn display_name_turns_package_ids_into_product_names() {
        assert_eq!(display_name(Path::new("C:/x/com.acme.copilot.stable")), "Copilot");
        assert_eq!(display_name(Path::new("C:/x/AcmeAI")), "Acmeai");
    }

    #[test]
    fn discovers_a_session_file_and_makes_a_deletable_row() {
        let root = std::env::temp_dir().join("agenthub-discover-test").join("com.example.newagent");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sessions")).unwrap();
        fs::write(
            root.join("sessions").join("chat-1.jsonl"),
            "{\"role\":\"user\",\"content\":\"hello there, please refactor this\"}\n\
             {\"role\":\"assistant\",\"content\":\"sure\"}\n",
        )
        .unwrap();

        // Drive the same helpers the discovery pass uses.
        let mut files = Vec::new();
        let mut budget = 10;
        collect_files(&root, 0, &mut files, &mut budget);
        assert_eq!(files.len(), 1, "the jsonl session must be picked up");
        assert_eq!(count_messages(&files[0]), 2);
        assert_eq!(title_from_file(&files[0]), "hello there, please refactor this");
    }

    #[test]
    fn ignores_files_that_are_not_sessions() {
        let root = std::env::temp_dir().join("agenthub-discover-test2");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let plain = root.join("notes.json");
        fs::write(&plain, "{\"hello\":\"world\"}").unwrap();
        assert!(!looks_like_session(&plain), "a JSON file without messages is not a session");
        let empty = root.join("empty.jsonl");
        fs::write(&empty, "").unwrap();
        assert!(!looks_like_session(&empty));
    }
}
