//! history: draw마다 1건(설정 + 섬네일), 이후 item 상태 변경을 덧붙인다. 앱 데이터 폴더의 JSON + PNG.

use std::collections::HashMap;
use std::path::PathBuf;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::spec::{ChartSpec, ItemState};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub created_ms: i64,
    pub spec: ChartSpec,
    /// split 조합 키 → item 상태
    #[serde(default)]
    pub items: HashMap<String, ItemState>,
    pub item_count: usize,
    /// draw에 걸린 시간 (ms)
    #[serde(default)]
    pub draw_ms: f64,
    /// 끝난 상태 (success / cancelled / failed)
    pub status: String,
}

pub struct History {
    dir: PathBuf,
    entries: Mutex<Vec<HistoryEntry>>,
}

impl History {
    pub fn open(dir: PathBuf) -> History {
        let dir = dir.join("history");
        let _ = std::fs::create_dir_all(&dir);
        let entries = std::fs::read(dir.join("history.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        History {
            dir,
            entries: Mutex::new(entries),
        }
    }

    fn save(&self) {
        let e = self.entries.lock().clone();
        if let Ok(b) = serde_json::to_vec(&e) {
            let _ = std::fs::write(self.dir.join("history.json"), b);
        }
    }

    pub fn list(&self) -> Vec<HistoryEntry> {
        let mut v = self.entries.lock().clone();
        v.sort_by(|a, b| b.created_ms.cmp(&a.created_ms));
        v
    }

    pub fn add(&self, entry: HistoryEntry, thumb_png: Option<Vec<u8>>) {
        if let Some(png) = thumb_png {
            let _ = std::fs::write(self.thumb_path(&entry.id), png);
        }
        self.entries.lock().push(entry);
        self.save();
    }

    pub fn update(&self, id: &str, f: impl FnOnce(&mut HistoryEntry)) {
        if let Some(e) = self.entries.lock().iter_mut().find(|e| e.id == id) {
            f(e);
        }
        self.save();
    }

    pub fn remove(&self, id: &str) {
        self.entries.lock().retain(|e| e.id != id);
        let _ = std::fs::remove_file(self.thumb_path(id));
        self.save();
    }

    pub fn thumb_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.png"))
    }
}

pub fn new_id() -> String {
    format!("h{}", chrono::Utc::now().timestamp_micros())
}
