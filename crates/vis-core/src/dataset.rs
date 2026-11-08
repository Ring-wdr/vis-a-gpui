//! 열 단위(columnar) 데이터셋. 원본은 PGlite 테이블이었고, 여기서는 메모리의 열 배열이다.
//! - 숫자·시계열: `Vec<f64>` (시계열은 epoch ms, 값 없음은 NaN)
//! - 범주: 사전(dict) + 코드. 사전은 정렬해 두어 코드 순서가 곧 정렬 순서다 (Postgres C 정렬 = 바이트 순).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const NULL_CODE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColumnKind {
    Categorical,
    Numeric,
    Datetime,
}

impl ColumnKind {
    pub fn label(self) -> &'static str {
        match self {
            ColumnKind::Categorical => "범주",
            ColumnKind::Numeric => "숫자",
            ColumnKind::Datetime => "시계열",
        }
    }
}

pub enum ColumnData {
    Num(Vec<f64>),
    Cat { codes: Vec<u32>, dict: Vec<String> },
}

pub struct Column {
    pub name: String,
    pub label: String,
    pub kind: ColumnKind,
    pub data: ColumnData,
}

impl Column {
    pub fn nums(&self) -> Option<&[f64]> {
        match &self.data {
            ColumnData::Num(v) => Some(v),
            _ => None,
        }
    }

    pub fn cat(&self) -> Option<(&[u32], &[String])> {
        match &self.data {
            ColumnData::Cat { codes, dict } => Some((codes, dict)),
            _ => None,
        }
    }

    pub fn len(&self) -> usize {
        match &self.data {
            ColumnData::Num(v) => v.len(),
            ColumnData::Cat { codes, .. } => codes.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 한 행의 값을 화면 글자로
    pub fn text_at(&self, row: usize) -> String {
        match &self.data {
            ColumnData::Num(v) => {
                let x = v[row];
                if x.is_nan() {
                    "null".into()
                } else if self.kind == ColumnKind::Datetime {
                    crate::timefmt::format_kst(x, "%Y-%m-%d %H:%M:%S")
                } else {
                    fmt_num(x)
                }
            }
            ColumnData::Cat { codes, dict } => {
                let c = codes[row];
                if c == NULL_CODE { "null".into() } else { dict[c as usize].clone() }
            }
        }
    }

    pub fn bytes(&self) -> usize {
        match &self.data {
            ColumnData::Num(v) => v.len() * 8,
            ColumnData::Cat { codes, dict } => codes.len() * 4 + dict.iter().map(|s| s.len() + 24).sum::<usize>(),
        }
    }
}

/// Postgres float8::text처럼 다시 읽으면 같은 값이 되는 가장 짧은 글자
pub fn fmt_num(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    format!("{x}")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColumnSchema {
    pub name: String,
    pub label: String,
    pub kind: ColumnKind,
    /// 범주 컬럼의 고유값 수 (null 제외)
    pub distinct: Option<usize>,
}

pub struct Dataset {
    pub id: String,
    pub name: String,
    pub rows: usize,
    pub columns: Vec<Column>,
    pub created_ms: i64,
}

impl Dataset {
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }

    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }

    pub fn schema(&self) -> Vec<ColumnSchema> {
        self.columns
            .iter()
            .map(|c| ColumnSchema {
                name: c.name.clone(),
                label: c.label.clone(),
                kind: c.kind,
                distinct: c.cat().map(|(_, d)| d.len()),
            })
            .collect()
    }

    pub fn bytes(&self) -> usize {
        self.columns.iter().map(Column::bytes).sum()
    }

    // ── 저장 (앱 데이터 폴더의 바이너리 파일) ─────────────────────────────

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut w = std::io::BufWriter::new(std::fs::File::create(path)?);
        w.write_all(b"VISDS1")?;
        write_str(&mut w, &self.id)?;
        write_str(&mut w, &self.name)?;
        w.write_all(&(self.rows as u64).to_le_bytes())?;
        w.write_all(&self.created_ms.to_le_bytes())?;
        w.write_all(&(self.columns.len() as u32).to_le_bytes())?;
        for c in &self.columns {
            write_str(&mut w, &c.name)?;
            write_str(&mut w, &c.label)?;
            w.write_all(&[match c.kind {
                ColumnKind::Categorical => 0,
                ColumnKind::Numeric => 1,
                ColumnKind::Datetime => 2,
            }])?;
            match &c.data {
                ColumnData::Num(v) => {
                    w.write_all(&[0])?;
                    for x in v {
                        w.write_all(&x.to_le_bytes())?;
                    }
                }
                ColumnData::Cat { codes, dict } => {
                    w.write_all(&[1])?;
                    w.write_all(&(dict.len() as u32).to_le_bytes())?;
                    for s in dict {
                        write_str(&mut w, s)?;
                    }
                    for x in codes {
                        w.write_all(&x.to_le_bytes())?;
                    }
                }
            }
        }
        w.flush()
    }

    pub fn load(path: &Path) -> std::io::Result<Dataset> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?.read_to_end(&mut bytes)?;
        let mut r = Reader { b: &bytes, at: 0 };
        if r.take(6)? != b"VISDS1" {
            return Err(std::io::Error::other("not a dataset file"));
        }
        let id = r.str()?;
        let name = r.str()?;
        let rows = r.u64()? as usize;
        let created_ms = r.u64()? as i64;
        let ncols = r.u32()? as usize;
        let mut columns = Vec::with_capacity(ncols);
        for _ in 0..ncols {
            let name = r.str()?;
            let label = r.str()?;
            let kind = match r.take(1)?[0] {
                0 => ColumnKind::Categorical,
                1 => ColumnKind::Numeric,
                _ => ColumnKind::Datetime,
            };
            let data = if r.take(1)?[0] == 0 {
                let raw = r.take(rows * 8)?;
                ColumnData::Num(raw.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect())
            } else {
                let n = r.u32()? as usize;
                let mut dict = Vec::with_capacity(n);
                for _ in 0..n {
                    dict.push(r.str()?);
                }
                let raw = r.take(rows * 4)?;
                ColumnData::Cat {
                    codes: raw.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect(),
                    dict,
                }
            };
            columns.push(Column { name, label, kind, data });
        }
        Ok(Dataset {
            id,
            name,
            rows,
            columns,
            created_ms,
        })
    }
}

fn write_str(w: &mut impl Write, s: &str) -> std::io::Result<()> {
    w.write_all(&(s.len() as u32).to_le_bytes())?;
    w.write_all(s.as_bytes())
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> std::io::Result<&'a [u8]> {
        if self.at + n > self.b.len() {
            return Err(std::io::Error::other("truncated dataset file"));
        }
        let s = &self.b[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }
    fn u32(&mut self) -> std::io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> std::io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn str(&mut self) -> std::io::Result<String> {
        let n = self.u32()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
}

/// 범주 컬럼을 만드는 도우미. 넣는 순서대로 코드를 주고, `finish`에서 사전을 정렬해 코드를 다시 매긴다.
#[derive(Default)]
pub struct CatBuilder {
    index: HashMap<String, u32>,
    dict: Vec<String>,
    codes: Vec<u32>,
}

impl CatBuilder {
    pub fn with_capacity(n: usize) -> Self {
        Self {
            codes: Vec::with_capacity(n),
            ..Default::default()
        }
    }

    /// 미리 사전 값을 넣어 둔다 (코드를 직접 push할 때)
    pub fn intern(&mut self, v: &str) -> u32 {
        if let Some(&c) = self.index.get(v) {
            return c;
        }
        let c = self.dict.len() as u32;
        self.dict.push(v.to_string());
        self.index.insert(v.to_string(), c);
        c
    }

    pub fn push(&mut self, v: Option<&str>) {
        let c = match v {
            Some(v) => self.intern(v),
            None => NULL_CODE,
        };
        self.codes.push(c);
    }

    #[inline]
    pub fn push_code(&mut self, c: u32) {
        self.codes.push(c);
    }

    pub fn finish(self) -> ColumnData {
        let CatBuilder { dict, mut codes, .. } = self;
        let mut order: Vec<u32> = (0..dict.len() as u32).collect();
        order.sort_by(|&a, &b| dict[a as usize].cmp(&dict[b as usize]));
        let mut remap = vec![0u32; dict.len()];
        for (new, &old) in order.iter().enumerate() {
            remap[old as usize] = new as u32;
        }
        use rayon::prelude::*;
        codes.par_iter_mut().for_each(|c| {
            if *c != NULL_CODE {
                *c = remap[*c as usize];
            }
        });
        let sorted = order.into_iter().map(|i| dict[i as usize].clone()).collect();
        ColumnData::Cat { codes, dict: sorted }
    }
}
