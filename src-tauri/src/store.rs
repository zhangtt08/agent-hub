use std::fs;
use std::path::Path;

use crate::model::StoredState;

pub fn load(dir: &Path) -> StoredState {
    let file = dir.join("state.json");
    let text = match fs::read_to_string(&file) {
        Ok(t) => t,
        Err(_) => return StoredState { version: 1, ..Default::default() },
    };
    let mut state: StoredState = serde_json::from_str(&text).unwrap_or_default();
    if state.version == 0 {
        state.version = 1;
    }
    state
}

pub fn save(dir: &Path, state: &StoredState) {
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    if let Ok(text) = serde_json::to_string_pretty(state) {
        let tmp = dir.join("state.json.tmp");
        if fs::write(&tmp, text).is_ok() {
            let _ = fs::rename(&tmp, dir.join("state.json"));
        }
    }
}
