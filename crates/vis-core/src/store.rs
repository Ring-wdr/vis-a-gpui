//! 데이터셋 저장소 (원본 datastore). 샘플은 `등록`해야 쓰고, 한 번 만든 데이터는 앱 데이터 폴더에 남는다.
//! 원본은 IndexedDB(PGlite idb://)에 남겼다.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::dataset::Dataset;
use crate::generator::{SAMPLES, generate, sample_of};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DatasetInfo {
    pub id: String,
    pub name: String,
    pub rows: usize,
    pub bytes: usize,
    pub created_ms: i64,
    pub sample: bool,
    /// 적재에 걸린 시간 (ms)
    #[serde(default)]
    pub load_ms: f64,
}

#[derive(Clone, Debug)]
pub enum DatasetState {
    /// 샘플인데 아직 등록하지 않음
    Unregistered,
    Loading(f32),
    Ready,
    Failed(String),
}

pub struct Store {
    dir: PathBuf,
    index: Mutex<Vec<DatasetInfo>>,
    loaded: Mutex<HashMap<String, Arc<Dataset>>>,
    loading: Mutex<HashMap<String, f32>>,
    failed: Mutex<HashMap<String, String>>,
}

pub fn app_dir() -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
    base.join("vis-a-gpui")
}

impl Store {
    pub fn open(dir: PathBuf) -> Arc<Store> {
        let _ = std::fs::create_dir_all(dir.join("datasets"));
        let index: Vec<DatasetInfo> = std::fs::read(dir.join("datasets/index.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        // 파일이 없는 항목은 뺀다
        let index = index
            .into_iter()
            .filter(|i| dir.join("datasets").join(format!("{}.bin", i.id)).exists())
            .collect();
        Arc::new(Store {
            dir,
            index: Mutex::new(index),
            loaded: Mutex::new(HashMap::new()),
            loading: Mutex::new(HashMap::new()),
            failed: Mutex::new(HashMap::new()),
        })
    }

    pub fn dir(&self) -> &PathBuf {
        &self.dir
    }

    fn file(&self, id: &str) -> PathBuf {
        self.dir.join("datasets").join(format!("{id}.bin"))
    }

    fn save_index(&self) {
        let idx = self.index.lock().clone();
        if let Ok(b) = serde_json::to_vec_pretty(&idx) {
            let _ = std::fs::write(self.dir.join("datasets/index.json"), b);
        }
    }

    /// 업로드 데이터(최근 순) 다음에 샘플. 샘플은 등록 여부와 함께 늘 보인다
    pub fn list(&self) -> Vec<(DatasetInfo, DatasetState)> {
        let idx = self.index.lock().clone();
        let mut out: Vec<(DatasetInfo, DatasetState)> = idx.iter().filter(|i| !i.sample).map(|i| (i.clone(), self.state(&i.id))).collect();
        out.sort_by(|a, b| b.0.created_ms.cmp(&a.0.created_ms));
        for s in SAMPLES.iter() {
            let info = idx.iter().find(|i| i.id == s.id).cloned().unwrap_or(DatasetInfo {
                id: s.id.into(),
                name: s.name.into(),
                rows: s.rows,
                bytes: 0,
                created_ms: 0,
                sample: true,
                load_ms: 0.0,
            });
            let st = self.state(s.id);
            out.push((info, st));
        }
        // 지금 적재 중인 업로드
        for (id, p) in self.loading.lock().iter() {
            if !out.iter().any(|o| &o.0.id == id) {
                out.insert(
                    0,
                    (
                        DatasetInfo {
                            id: id.clone(),
                            name: id.clone(),
                            rows: 0,
                            bytes: 0,
                            created_ms: 0,
                            sample: false,
                            load_ms: 0.0,
                        },
                        DatasetState::Loading(*p),
                    ),
                );
            }
        }
        out
    }

    pub fn state(&self, id: &str) -> DatasetState {
        if let Some(p) = self.loading.lock().get(id) {
            return DatasetState::Loading(*p);
        }
        if let Some(e) = self.failed.lock().get(id) {
            return DatasetState::Failed(e.clone());
        }
        if self.index.lock().iter().any(|i| i.id == id) {
            DatasetState::Ready
        } else {
            DatasetState::Unregistered
        }
    }

    pub fn ready(&self) -> Vec<DatasetInfo> {
        self.list()
            .into_iter()
            .filter(|(_, s)| matches!(s, DatasetState::Ready))
            .map(|(i, _)| i)
            .collect()
    }

    /// 메모리에 올린 데이터셋 (없으면 파일에서 읽는다)
    pub fn get(&self, id: &str) -> Option<Arc<Dataset>> {
        if let Some(d) = self.loaded.lock().get(id) {
            return Some(d.clone());
        }
        if !self.index.lock().iter().any(|i| i.id == id) {
            return None;
        }
        let d = Arc::new(Dataset::load(&self.file(id)).ok()?);
        self.loaded.lock().insert(id.to_string(), d.clone());
        Some(d)
    }

    fn finish(&self, ds: Dataset, sample: bool, started: Instant) -> Arc<Dataset> {
        let info = DatasetInfo {
            id: ds.id.clone(),
            name: ds.name.clone(),
            rows: ds.rows,
            bytes: ds.bytes(),
            created_ms: ds.created_ms,
            sample,
            load_ms: started.elapsed().as_secs_f64() * 1000.0,
        };
        let _ = ds.save(&self.file(&ds.id));
        let ds = Arc::new(ds);
        self.loaded.lock().insert(info.id.clone(), ds.clone());
        {
            let mut idx = self.index.lock();
            idx.retain(|i| i.id != info.id);
            idx.push(info);
        }
        self.save_index();
        ds
    }

    /// 샘플 등록 (생성 + 저장). 진행률은 state()로 읽는다
    pub fn register_sample(&self, id: &str) -> Option<Arc<Dataset>> {
        let s = sample_of(id)?;
        self.loading.lock().insert(id.into(), 0.0);
        self.failed.lock().remove(id);
        let started = Instant::now();
        let ds = generate(s.id, s.name, s.rows, |p| {
            self.loading.lock().insert(id.into(), p as f32);
        });
        let ds = self.finish(ds, true, started);
        self.loading.lock().remove(id);
        Some(ds)
    }

    /// CSV 파일 가져오기
    pub fn import_csv(&self, path: &std::path::Path, opts: &crate::csv_import::CsvOptions) -> Result<Arc<Dataset>, String> {
        let id = format!("up-{}", chrono::Utc::now().timestamp_millis());
        self.loading.lock().insert(id.clone(), 0.0);
        let started = Instant::now();
        let res = crate::csv_import::import(path, &id, opts, |p| {
            self.loading.lock().insert(id.clone(), p);
        });
        self.loading.lock().remove(&id);
        match res {
            Ok(ds) => Ok(self.finish(ds, false, started)),
            Err(e) => {
                self.failed.lock().insert(id, e.clone());
                Err(e)
            }
        }
    }

    pub fn delete(&self, id: &str) {
        self.loaded.lock().remove(id);
        self.index.lock().retain(|i| i.id != id);
        let _ = std::fs::remove_file(self.file(id));
        self.save_index();
    }

    pub fn total_bytes(&self) -> usize {
        self.index.lock().iter().map(|i| i.bytes).sum()
    }
}
