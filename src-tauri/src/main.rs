#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod delete;
mod discover;
mod index;
mod model;
mod preview;
mod scan;
mod store;

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{Manager, State};


use model::*;

pub static INDEXING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct AppState {
    inner: Mutex<Inner>,
}

struct Inner {
    agents: Vec<AgentReport>,
    rows: Vec<Conversation>,
    scanned_at: Option<String>,
    duration_ms: u64,
    progress: Progress,
    archived: std::collections::HashMap<String, String>,
    deleted: std::collections::HashMap<String, String>,
    starred: std::collections::HashSet<String>,
    data_dir: PathBuf,
}

impl Inner {
    fn new(data_dir: PathBuf) -> Self {
        let saved = store::load(&data_dir);
        Self {
            agents: Vec::new(),
            rows: Vec::new(),
            scanned_at: None,
            duration_ms: 0,
            progress: Progress::default(),
            archived: saved.archived,
            deleted: saved.deleted,
            starred: saved.starred.into_iter().collect(),
            data_dir,
        }
    }

    fn persist(&self) {
        let state = StoredState {
            version: 1,
            archived: self.archived.clone(),
            deleted: self.deleted.clone(),
            starred: self.starred.iter().cloned().collect(),
        };
        store::save(&self.data_dir, &state);
    }
}

fn visible_rows(inner: &Inner) -> Vec<Conversation> {
    inner
        .rows
        .iter()
        .filter(|r| !inner.deleted.contains_key(&r.id))
        .map(|r| {
            let mut c = r.clone();
            c.archived_by_user = inner.archived.contains_key(&r.id);
            c.archived = c.archived_by_user || c.in_archive_dir;
            c
        })
        .collect()
}

fn make_bucket<F: Fn(&Conversation) -> bool>(key: &str, label: &str, rows: &[Conversation], test: F) -> Bucket {
    let items: Vec<&Conversation> = rows.iter().filter(|r| test(r)).collect();
    Bucket {
        key: key.to_string(),
        label: label.to_string(),
        count: items.len(),
        bytes: items.iter().map(|r| r.size).sum(),
        ids: items.iter().map(|r| r.id.clone()).collect(),
    }
}

fn build_payload(inner: &Inner) -> Payload {
    let rows = visible_rows(inner);
    let now = scan::now_ms();

    let agents = inner
        .agents
        .iter()
        .map(|a| {
            let mine: Vec<&Conversation> = rows.iter().filter(|r| r.agent == a.id).collect();
            AgentReport {
                count: mine.len(),
                bytes: mine.iter().map(|r| r.size).sum(),
                messages: mine.iter().map(|r| r.messages).sum(),
                last_sync: mine.iter().map(|r| r.mtime_ms as u64).max().unwrap_or(0),
                installed: a.installed,
                roots: a.roots.clone(),
                ..a.clone()
            }
        })
        .collect::<Vec<_>>();

    // Every row is deletable now, including sessions inside an agent database.
    // Those occupy no standalone file, so they only contribute to the count buckets.
    // Index-only rows (auto-discovered sources we cannot fully parse) must never
    // appear as cleanup candidates: offering them for deletion is how data is lost.
    let cleanable: Vec<Conversation> = rows
        .iter()
        .filter(|r| !(r.readonly && !r.path.contains('#')))
        .cloned()
        .collect();
    // Rules describe *low value*, never "big". A long conversation is the most
    // valuable thing a user has; offering it by size caused real data loss.
    let buckets = vec![
        make_bucket("old", "超过 30 天未更新", &cleanable, |r| now - r.mtime_ms > 30 * 86400000),
        make_bucket("archived", "已归档的会话", &cleanable, |r| r.archived),
        make_bucket("tiny", "几乎没有内容（≤ 2 条消息）", &cleanable, |r| r.messages <= 2),
    ];

    let totals = Totals {
        conversations: rows.len(),
        bytes: rows.iter().map(|r| r.size).sum(),
        messages: rows.iter().map(|r| r.messages).sum(),
        installed: agents.iter().filter(|a| a.installed).count(),
        with_sessions: agents.iter().filter(|a| a.count > 0).count(),
    };

    Payload {
        scanning: inner.progress.clone(),
        scanned_at: inner.scanned_at.clone(),
        duration_ms: inner.duration_ms,
        agents,
        conversations: rows,
        buckets,
        totals,
        data_dir: inner.data_dir.to_string_lossy().to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        starred: inner.starred.iter().cloned().collect(),
        indexing: INDEXING.load(std::sync::atomic::Ordering::Relaxed),
    }
}

/* ------------------------------------------------------------- commands */

#[tauri::command]
fn get_state(state: State<AppState>) -> Payload {
    let inner = state.inner.lock().unwrap();
    build_payload(&inner)
}

fn spawn_scan(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        {
            let state = app.state::<AppState>();
            let mut inner = state.inner.lock().unwrap();
            if inner.progress.running {
                return;
            }
            inner.progress = Progress { running: true, done: 0, total: 0 };
        }

        let output = scan::scan(&mut |done, total| {
            if let Ok(mut inner) = app.state::<AppState>().inner.lock() {
                inner.progress.done = done;
                inner.progress.total = total;
            }
        });

        let state = app.state::<AppState>();
        let (rows, data_dir) = {
            let mut inner = state.inner.lock().unwrap();
            inner.agents = output.agents;
            // A file that came back (restored from the recycle bin, or a DB row
            // that was re-indexed) must not stay hidden by a stale delete record.
            for row in output.conversations.iter() {
                inner.deleted.remove(&row.id);
            }
            inner.rows = output.conversations;
            inner.duration_ms = output.duration_ms;
            inner.scanned_at = Some(scan::iso(scan::now_ms()));
            let done = inner.progress.done;
            let total = inner.progress.total;
            inner.progress = Progress { running: false, done, total };
            inner.persist();
            (inner.rows.clone(), inner.data_dir.clone())
        };

        // Build the search index off the UI thread; unchanged files are skipped.
        INDEXING.store(true, std::sync::atomic::Ordering::Relaxed);
        std::thread::spawn(move || {
            if let Some(conn) = index::open(&data_dir.join("index.db")) {
                index::sync(&conn, &rows, &|c| preview::extract_all(c));
            }
            INDEXING.store(false, std::sync::atomic::Ordering::Relaxed);
        });
    });
}

#[tauri::command]
fn rescan(app: tauri::AppHandle) {
    spawn_scan(app);
}

#[tauri::command]
fn archive(id: String, archived: bool, state: State<AppState>) -> Result<(), String> {
    let mut inner = state.inner.lock().unwrap();
    if archived {
        inner.archived.insert(id, scan::iso(scan::now_ms()));
    } else {
        inner.archived.remove(&id);
    }
    inner.persist();
    Ok(())
}

#[tauri::command]
fn star(id: String, starred: bool, state: State<AppState>) -> Result<(), String> {
    let mut inner = state.inner.lock().unwrap();
    if starred {
        inner.starred.insert(id);
    } else {
        inner.starred.remove(&id);
    }
    inner.persist();
    Ok(())
}

#[derive(serde::Serialize)]
struct SearchHit {
    seq: i64,
    role: String,
    snippet: String,
}

#[derive(serde::Serialize)]
struct SearchGroup {
    conversation: Conversation,
    hits: Vec<SearchHit>,
}

#[derive(serde::Serialize)]
struct SearchResult {
    ready: bool,
    groups: Vec<SearchGroup>,
}

#[tauri::command]
async fn search_text(q: String, state: State<'_, AppState>) -> Result<SearchResult, String> {
    let q = q.trim().to_string();
    if q.chars().count() < 2 {
        return Ok(SearchResult { ready: true, groups: Vec::new() });
    }
    let (data_dir, rows) = {
        let inner = state.inner.lock().unwrap();
        (inner.data_dir.clone(), visible_rows(&inner))
    };
    if !data_dir.join("index.db").exists() {
        return Ok(SearchResult { ready: false, groups: Vec::new() });
    }
    let mut groups: Vec<SearchGroup> = Vec::new();
    for hit in index::search(&data_dir, &q, 400) {
        let Some(c) = rows.iter().find(|c| c.id == hit.session_id) else { continue };
        if let Some(g) = groups.iter_mut().find(|g| g.conversation.id == hit.session_id) {
            if g.hits.len() < 3 {
                g.hits.push(SearchHit { seq: hit.seq, role: hit.role, snippet: hit.snippet });
            }
        } else if groups.len() < 30 {
            groups.push(SearchGroup {
                conversation: c.clone(),
                hits: vec![SearchHit { seq: hit.seq, role: hit.role, snippet: hit.snippet }],
            });
        }
    }
    Ok(SearchResult { ready: true, groups })
}

#[tauri::command]
async fn delete_sessions(
    ids: Vec<String>,
    purge: bool,
    state: State<'_, AppState>,
) -> Result<DeleteResult, String> {
    // Run the disk work off the UI thread: deleting a session out of a 280 MB
    // agent database takes seconds and would otherwise freeze the window.
    let (targets, missing, roots) = {
        let inner = state.inner.lock().unwrap();
        let mut targets = Vec::new();
        let mut missing = Vec::new();
        for id in ids.iter() {
            match inner.rows.iter().find(|r| &r.id == id) {
                Some(r) => targets.push(r.clone()),
                None => missing.push(id.clone()),
            }
        }
        (targets, missing, scan::all_roots())
    };

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let mut result = DeleteResult { purge, ..Default::default() };
        for id in missing {
            result.errors.push(DeleteError { id, error: "会话不在索引中，请重新扫描".into() });
        }
        for row in targets {
            match delete_one(&row, purge, &roots, false) {
                Ok(freed) => {
                    result.freed += freed;
                    result.deleted.push(row.id);
                }
                Err(err) => result.errors.push(DeleteError { id: row.id, error: err }),
            }
        }
        result
    })
    .await
    .map_err(|e| format!("删除任务失败：{e}"))?;

    finish_delete(&state, outcome)
}

#[tauri::command]
async fn force_delete(ids: Vec<String>, state: State<'_, AppState>) -> Result<DeleteResult, String> {
    let (targets, missing, roots) = {
        let inner = state.inner.lock().unwrap();
        let mut targets = Vec::new();
        let mut missing = Vec::new();
        for id in ids.iter() {
            match inner.rows.iter().find(|r| &r.id == id) {
                Some(r) => targets.push(r.clone()),
                None => missing.push(id.clone()),
            }
        }
        (targets, missing, scan::all_roots())
    };

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let mut result = DeleteResult { purge: true, ..Default::default() };
        for id in missing {
            result.errors.push(DeleteError { id, error: "会话不在索引中，请重新扫描".into() });
        }
        let mut touched: Vec<PathBuf> = Vec::new();
        for row in targets {
            if row.readonly {
                if let Some((p, _)) = row.path.split_once('#') {
                    touched.push(PathBuf::from(p));
                }
            }
            match delete_one(&row, true, &roots, true) {
                Ok(freed) => {
                    result.freed += freed;
                    result.deleted.push(row.id);
                }
                Err(err) => result.errors.push(DeleteError { id: row.id, error: err }),
            }
        }
        // Compact each touched database once, after all rows are gone.
        touched.sort();
        touched.dedup();
        for db in touched {
            result.freed += delete::vacuum(&db);
        }
        result
    })
    .await
    .map_err(|e| format!("删除任务失败：{e}"))?;

    finish_delete(&state, outcome)
}

/// Delete one row on disk. Shared by the normal and force paths.
fn delete_one(row: &Conversation, purge: bool, roots: &[PathBuf], force: bool) -> Result<u64, String> {
    let (file_path, session_id) = match row.path.split_once('#') {
        Some((p, s)) => (p.to_string(), Some(s.to_string())),
        None => (row.path.clone(), None),
    };
    if !delete::inside_roots(std::path::Path::new(&file_path), roots) {
        return Err("路径不在已知 Agent 目录内，已拒绝".into());
    }
    match session_id {
        Some(sid) if row.readonly => {
            // Only compact on an explicit force delete: VACUUM on a 280 MB db
            // costs seconds and would make every normal delete feel stuck.
            delete::delete_db_session(&row.agent, std::path::Path::new(&file_path), &sid, force)
        }
        // A read-only row with no database id is index-only (auto-discovered).
        // Deleting its file would remove data we never understood.
        None if row.readonly => Err("该来源为只读索引，不支持删除".into()),
        _ if force => delete::force_delete_files(std::path::Path::new(&file_path)),
        _ => delete::delete_files(std::path::Path::new(&file_path), purge),
    }
}

/// Record the deletions in the index and hand back fresh totals.
fn finish_delete(state: &State<'_, AppState>, mut result: DeleteResult) -> Result<DeleteResult, String> {
    let mut inner = state.inner.lock().unwrap();
    for id in result.deleted.iter() {
        inner.deleted.insert(id.clone(), scan::iso(scan::now_ms()));
    }
    inner.rows.retain(|r| !result.deleted.contains(&r.id));
    inner.persist();
    result.totals = Some(build_payload(&inner).totals);
    Ok(result)
}

#[tauri::command]
fn preview_conversation(id: String, state: State<AppState>) -> Result<Preview, String> {
    let inner = state.inner.lock().unwrap();
    let row = inner
        .rows
        .iter()
        .find(|r| r.id == id)
        .cloned()
        .ok_or_else(|| "会话不存在".to_string())?;
    preview::build(&row)
}

#[derive(serde::Serialize)]
struct Transcript {
    conversation: Conversation,
    messages: Vec<PreviewMessage>,
    total: usize,
}

#[tauri::command]
fn get_transcript(id: String, state: State<AppState>) -> Result<Transcript, String> {
    let row = {
        let inner = state.inner.lock().unwrap();
        inner.rows.iter().find(|r| r.id == id).cloned()
    }
    .ok_or_else(|| "会话不存在，请重新扫描".to_string())?;
    let messages = preview::extract_all(&row);
    Ok(Transcript { total: messages.len(), messages, conversation: row })
}

#[tauri::command]
fn export_markdown(name: String, content: String) -> Result<String, String> {
    let dir = dirs::download_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| "找不到下载目录".to_string())?;
    let clean: String = name
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '-' } else { c })
        .collect();
    let base = clean.trim_matches('.').trim();
    let base = if base.is_empty() { "会话" } else { base };
    let mut path = dir.join(format!("{}.md", base));
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{}-{}.md", base, n));
        n += 1;
    }
    fs::write(&path, content.as_bytes()).map_err(|e| format!("写入失败：{e}"))?;
    Ok(path.to_string_lossy().to_string())
}


#[tauri::command]
fn reveal(id: String, state: State<AppState>) -> Result<(), String> {
    let inner = state.inner.lock().unwrap();
    let row = inner.rows.iter().find(|r| r.id == id).ok_or_else(|| "会话不存在".to_string())?;
    let path = row.path.split('#').next().unwrap_or(&row.path).to_string();
    if !delete::inside_roots(std::path::Path::new(&path), &scan::all_roots()) {
        return Err("路径不在已知 Agent 目录内".into());
    }
    std::process::Command::new("explorer.exe")
        .arg("/select,")
        .arg(path)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn reveal_data(state: State<AppState>) -> Result<(), String> {
    let dir = state.inner.lock().unwrap().data_dir.clone();
    fs::create_dir_all(&dir).ok();
    std::process::Command::new("explorer.exe")
        .arg(dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn window_minimize(window: tauri::Window) {
    let _ = window.minimize();
}

#[tauri::command]
fn window_toggle_maximize(window: tauri::Window) {
    if window.is_maximized().unwrap_or(false) {
        let _ = window.unmaximize();
    } else {
        let _ = window.maximize();
    }
}

#[tauri::command]
fn window_close(window: tauri::Window) {
    let _ = window.close();
}

/// Remove the 1px DWM frame Windows draws around a frameless window and keep
/// rounded corners, so the app looks like one surface instead of a bordered box.
#[cfg(windows)]
fn polish_window(window: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWM_WINDOW_CORNER_PREFERENCE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_SHOWNORMAL};

    let Ok(raw) = window.hwnd() else { return };
    let hwnd = HWND(raw.0);
    unsafe {
        let transparent_border: u32 = 0x00DD_E9F0; // COLORREF of the page paper #F0E9DD
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &transparent_border as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );
        let round = DWM_WINDOW_CORNER_PREFERENCE(2); // DWMWCP_ROUND
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &round as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
        // Windows applies the launcher's STARTUPINFO to the first ShowWindow call,
        // which can leave the window minimized with no visible window at all.
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// The launcher can minimize the window again right after setup, so keep
/// restoring until it is really on screen (bounded, a few short attempts).
#[cfg(windows)]
fn ensure_window_visible(window: tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        IsIconic, SetForegroundWindow, ShowWindow, SW_SHOWNORMAL,
    };
    let Ok(raw) = window.hwnd() else { return };
    let raw_handle = raw.0 as usize;
    std::thread::spawn(move || {
        let hwnd = HWND(raw_handle as *mut core::ffi::c_void);
        for attempt in 0..12 {
            std::thread::sleep(std::time::Duration::from_millis(if attempt < 4 { 120 } else { 400 }));
            let _ = window.unminimize();
            let _ = window.show();
            unsafe {
                if IsIconic(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
                }
                let _ = SetForegroundWindow(hwnd);
            }
            if window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false) {
                break;
            }
        }
    });
}

/* ------------------------------------------------------- startup helpers */

#[cfg(windows)]
fn startup_log(message: &str) {
    let dir = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|p| p.join("com.agenthub.desktop"))
        .unwrap_or_else(std::env::temp_dir);
    let _ = fs::create_dir_all(&dir);
    let line = format!(
        "[{}] {}\r\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        message
    );
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("startup.log"))
    {
        use std::io::Write;
        let _ = file.write_all(line.as_bytes());
    }
}

/// Only one instance may own the WebView2 profile. A second launch brings the
/// existing window forward instead of silently failing to create a window.
#[cfg(windows)]
fn claim_single_instance() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    unsafe {
        match CreateMutexW(None, false, w!("Local\\AgentHub.SingleInstance")) {
            Ok(handle) => {
                GetLastError() != ERROR_ALREADY_EXISTS
            }
            Err(_) => true,
        }
    }
}

#[cfg(windows)]
fn focus_existing_window() {
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };
    unsafe {
        if let Ok(hwnd) = FindWindowW(None, w!("Agent 对话管理")) {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

#[cfg(windows)]
fn show_error_dialog(message: &str) {
    use windows::core::{w, HSTRING};
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let text = HSTRING::from(message);
    unsafe {
        MessageBoxW(None, &text, w!("Agent 对话管理"), MB_OK | MB_ICONERROR);
    }
}

fn main() {
    #[cfg(windows)]
    {
        if !claim_single_instance() {
            focus_existing_window();
            return;
        }
        startup_log("start");
    }

    let result = tauri::Builder::default()
        .setup(|app| {
            let data_dir = std::env::var_os("AGENT_HUB_DATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    app.path()
                        .app_data_dir()
                        .unwrap_or_else(|_| PathBuf::from("."))
                });
            fs::create_dir_all(&data_dir).ok();
            app.manage(AppState { inner: Mutex::new(Inner::new(data_dir)) });
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                #[cfg(windows)]
                polish_window(&window);
                let _ = window.show();
                let _ = window.set_focus();
                #[cfg(windows)]
                startup_log(&format!(
                    "window visible={:?} minimized={:?}",
                    window.is_visible().unwrap_or(false),
                    window.is_minimized().unwrap_or(false)
                ));
                #[cfg(windows)]
                ensure_window_visible(window.clone());
            }
            spawn_scan(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            rescan,
            archive,
            star,
            search_text,
            delete_sessions,
            force_delete,
            preview_conversation,
            get_transcript,
            export_markdown,
            reveal,
            reveal_data,
            window_minimize,
            window_toggle_maximize,
            window_close
        ])
        .run(tauri::generate_context!());

    if let Err(err) = result {
        #[cfg(windows)]
        {
            startup_log(&format!("fatal: {err}"));
            show_error_dialog(&format!(
                "Agent 对话管理启动失败：\n\n{}\n\n日志：%LOCALAPPDATA%\\com.agenthub.desktop\\startup.log",
                err
            ));
        }
        #[cfg(not(windows))]
        eprintln!("fatal: {err}");
    }

    #[cfg(windows)]
    startup_log("exit");
}
