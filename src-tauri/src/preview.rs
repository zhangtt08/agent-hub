use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

use crate::model::{Conversation, Preview, PreviewMessage};

const MAX_MESSAGES: usize = 6;
const MAX_CHARS: usize = 600;
const TRANSCRIPT_CHARS: usize = 8000;

fn tidy(text: &str, max: usize) -> String {
    let mut t = text.to_string();
    // Harness-injected reminders are not part of the conversation; drop them.
    for tag in ["system-reminder"] {
        let open = format!("<{}>", tag);
        let close = format!("</{}>", tag);
        while let Some(a) = t.find(&open) {
            match t[a..].find(&close) {
                Some(b) => t.replace_range(a..a + b + close.len(), " "),
                None => { t.truncate(a); break; }
            }
        }
    }
    for tag in ["environment_context", "user_instructions"] {
        let open = format!("<{}>", tag);
        let close = format!("</{}>", tag);
        while let Some(a) = t.find(&open) {
            match t[a..].find(&close) {
                Some(b) => t.replace_range(a..a + b + close.len(), " "),
                None => break,
            }
        }
    }
    if let Some(idx) = t.find("## My request:") {
        let probe = idx.min(400);
        let cut = (0..=probe).rev().find(|&i| t.is_char_boundary(i)).unwrap_or(0);
        if t[..cut].to_lowercase().contains("files mentioned by the user") {
            t = t[idx + "## My request:".len()..].to_string();
        }
    }
    let t = t.trim().to_string();
    if t.starts_with("# AGENTS.md") || t.starts_with("Here is a list of plugins") {
        return String::new();
    }
    if t.chars().count() > max {
        let mut s: String = t.chars().take(max).collect();
        s.push('…');
        s
    } else {
        t
    }
}

fn push_msg(list: &mut Vec<PreviewMessage>, role: &str, text: &str, time: &str, max: usize) {
    let t = tidy(text, max);
    if t.is_empty() {
        return;
    }
    if let Some(last) = list.last() {
        if last.role == role && last.text == t && last.time == time {
            return;
        }
    }
    list.push(PreviewMessage { role: role.to_string(), text: t, time: time.to_string() });
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
            .join("\n"),
        _ => String::new(),
    }
}

fn dsh_time(v: &Value) -> String {
    v.get("time")
        .and_then(|t| t.as_i64())
        .map(|ms| {
            use chrono::TimeZone;
            chrono::Local
                .timestamp_millis_opt(ms)
                .single()
                .map(|d| d.to_rfc3339())
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

/// Parse every user/assistant message in a Codex or DSH JSONL stream.
fn parse_lines<R: BufRead>(reader: R, max: usize, out: &mut Vec<PreviewMessage>) {
    for line in reader.lines().map_while(Result::ok) {
        if !line.starts_with('{') {
            continue;
        }
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("response_item") => {
                let payload = &v["payload"];
                if payload.get("type").and_then(|t| t.as_str()) != Some("message") {
                    continue;
                }
                let role = payload.get("role").and_then(|r| r.as_str()).unwrap_or_default();
                if role == "user" || role == "assistant" {
                    let ts = v.get("timestamp").and_then(|t| t.as_str()).unwrap_or_default();
                    push_msg(out, role, &content_text(&payload["content"]), ts, max);
                }
            }
            Some("user/message") => {
                let ts = dsh_time(&v);
                push_msg(out, "user", &content_text(&v["data"]["content"]), &ts, max);
            }
            Some("assistant/message") => {
                let ts = dsh_time(&v);
                push_msg(out, "assistant", &content_text(&v["data"]["message"]["content"]), &ts, max);
            }
            // Claude Code / Qoder CLI: {"type":"user","message":{"role":"user","content":[...]}}
            Some("user") | Some("assistant") => {
                let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or_default();
                let msg = &v["message"];
                let ts = v
                    .get("timestamp")
                    .and_then(|t| t.as_str())
                    .unwrap_or_default();
                push_msg(out, kind, &content_text(&msg["content"]), ts, max);
            }
            _ => {}
        }
    }
}

fn read_tail(path: &Path, bytes: u64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if start > 0 {
        if let Some(nl) = text.find('\n') {
            text = text[nl + 1..].to_string();
        }
    }
    Ok(text)
}

fn read_zstd(path: &Path, limit: usize) -> std::io::Result<String> {
    let file = File::open(path)?;
    let mut decoder = zstd::stream::read::Decoder::new(file)?;
    let mut out = Vec::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = decoder.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if out.len() > limit {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Every message of a conversation, streamed so big Codex rollouts stay cheap.
pub fn extract_all(conversation: &Conversation) -> Vec<PreviewMessage> {
    if !conversation.previewable {
        return Vec::new();
    }
    let path = Path::new(&conversation.path);
    let mut out = Vec::new();
    if conversation.compressed {
        if let Ok(file) = File::open(path) {
            if let Ok(decoder) = zstd::stream::read::Decoder::new(file) {
                parse_lines(BufReader::with_capacity(256 * 1024, decoder), TRANSCRIPT_CHARS, &mut out);
            }
        }
    } else if let Ok(file) = File::open(path) {
        parse_lines(BufReader::with_capacity(256 * 1024, file), TRANSCRIPT_CHARS, &mut out);
    }
    out
}

pub fn build(conversation: &Conversation) -> Result<Preview, String> {
    if !conversation.previewable {
        let who = match conversation.agent.as_str() {
            "qoder" => "Qoder",
            "workbuddy" => "WorkBuddy",
            "opencode" => "OpenCode",
            "trae" => "Trae",
            "zcode" => "ZCode",
            _ => "这个 Agent",
        };
        return Err(format!("{} 将会话保存在数据库中，暂不支持预览，请在原应用中查看", who));
    }
    let path = Path::new(&conversation.path);

    let mut all = Vec::new();
    if conversation.compressed {
        let text = read_zstd(path, 64 * 1024 * 1024).map_err(|e| format!("读取失败：{}", e))?;
        parse_lines(text.as_bytes(), MAX_CHARS, &mut all);
    } else {
        let text = read_tail(path, 2 * 1024 * 1024).map_err(|e| format!("读取失败：{}", e))?;
        parse_lines(text.as_bytes(), MAX_CHARS, &mut all);
    }

    let total = all.len();
    let messages: Vec<PreviewMessage> = all.into_iter().rev().take(MAX_MESSAGES).rev().collect();

    Ok(Preview {
        messages,
        truncated: total > MAX_MESSAGES,
        hint: "显示这个对话最近的消息".to_string(),
        conversation: conversation.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Claude Code / Qoder CLI sessions use {"type":"user","message":{...}}.
    /// If this format is not parsed, those sessions show up but preview is empty.
    #[test]
    fn parses_claude_style_records() {
        let jsonl = "\
{\"type\":\"workspace-directories\",\"sessionId\":\"s1\"}
{\"type\":\"user\",\"timestamp\":\"2026-10-25T10:00:00+08:00\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"fix the login bug\"}]}}
{\"type\":\"assistant\",\"timestamp\":\"2026-10-25T10:01:00+08:00\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"done, patched it\"}]}}
";
        let mut out = Vec::new();
        parse_lines(jsonl.as_bytes(), 600, &mut out);
        assert_eq!(out.len(), 2, "user + assistant must both parse");
        assert_eq!(out[0].role, "user");
        assert_eq!(out[0].text, "fix the login bug");
        assert_eq!(out[1].role, "assistant");
        assert!(out[1].text.contains("patched"));
    }
}