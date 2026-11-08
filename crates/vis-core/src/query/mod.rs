//! 원본의 SQL 쿼리(PGlite)를 열 배열 연산으로 옮긴 것.
//! 필터 → 행 번호 목록, split 조합 → 그룹(CSR), 차트별 집계 순으로 돈다. 큰 단계는 rayon으로 나눠 돈다.

pub mod agg;
pub mod candle;
pub mod category;
pub mod gantt;
pub mod heatmap;
pub mod histogram;
pub mod points;
pub mod sankey;
pub mod summary;

use rayon::prelude::*;

use crate::dataset::{Column, ColumnData, ColumnKind, Dataset, NULL_CODE};
use crate::spec::Filter;

pub const MAX_SPLIT_ITEMS: usize = 30_000;

#[derive(Debug)]
pub struct QueryError(pub String);

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for QueryError {}

pub type QResult<T> = Result<T, QueryError>;

pub fn column<'a>(ds: &'a Dataset, name: &str, kinds: &[ColumnKind]) -> QResult<&'a Column> {
    let c = ds.column(name).ok_or_else(|| QueryError(format!("'{name}' 컬럼이 없습니다.")))?;
    if !kinds.is_empty() && !kinds.contains(&c.kind) {
        return Err(QueryError(format!(
            "'{}'은 {} 컬럼이라 이 자리에 쓸 수 없습니다.",
            c.label,
            c.kind.label()
        )));
    }
    Ok(c)
}

const CHUNK: usize = 1 << 16;

/// 필터와 "값이 있어야 하는 컬럼" 조건을 모두 만족하는 행 번호 (원래 순서)
pub fn select_rows(ds: &Dataset, filters: &[Filter], not_null: &[&Column]) -> QResult<Vec<u32>> {
    enum Test<'a> {
        Range {
            v: &'a [f64],
            min: f64,
            max: f64,
            inverse: bool,
            include_null: bool,
        },
        Exclude {
            codes: &'a [u32],
            excluded: Vec<bool>,
            null_excluded: bool,
        },
    }
    let mut tests = Vec::new();
    for f in filters {
        match f {
            Filter::Range {
                column: name,
                min,
                max,
                inverse,
                include_null,
            } => {
                let c = column(ds, name, &[ColumnKind::Numeric, ColumnKind::Datetime])?;
                tests.push(Test::Range {
                    v: c.nums().unwrap(),
                    min: *min,
                    max: *max,
                    inverse: *inverse,
                    include_null: *include_null,
                });
            }
            Filter::Exclude { column: name, values } => {
                let c = column(ds, name, &[ColumnKind::Categorical])?;
                if values.is_empty() {
                    continue;
                }
                let (codes, dict) = c.cat().unwrap();
                let set: std::collections::HashSet<&str> = values.iter().map(String::as_str).collect();
                tests.push(Test::Exclude {
                    codes,
                    excluded: dict.iter().map(|d| set.contains(d.as_str())).collect(),
                    null_excluded: set.contains("null"),
                });
            }
        }
    }
    enum Need<'a> {
        Num(&'a [f64]),
        Cat(&'a [u32]),
    }
    let needs: Vec<Need> = not_null
        .iter()
        .map(|c| match &c.data {
            ColumnData::Num(v) => Need::Num(v),
            ColumnData::Cat { codes, .. } => Need::Cat(codes),
        })
        .collect();
    let n = ds.rows;
    let parts: Vec<Vec<u32>> = (0..n.div_ceil(CHUNK))
        .into_par_iter()
        .map(|chunk| {
            let start = chunk * CHUNK;
            let end = (start + CHUNK).min(n);
            let mut out = Vec::with_capacity(end - start);
            'row: for i in start..end {
                for need in &needs {
                    let missing = match need {
                        Need::Num(v) => v[i].is_nan(),
                        Need::Cat(c) => c[i] == NULL_CODE,
                    };
                    if missing {
                        continue 'row;
                    }
                }
                for t in &tests {
                    let keep = match t {
                        Test::Range {
                            v,
                            min,
                            max,
                            inverse,
                            include_null,
                        } => {
                            let x = v[i];
                            if x.is_nan() {
                                *include_null
                            } else if *inverse {
                                x < *min || x > *max
                            } else {
                                x >= *min && x <= *max
                            }
                        }
                        Test::Exclude {
                            codes,
                            excluded,
                            null_excluded,
                        } => {
                            let c = codes[i];
                            if c == NULL_CODE { !null_excluded } else { !excluded[c as usize] }
                        }
                    };
                    if !keep {
                        continue 'row;
                    }
                }
                out.push(i as u32);
            }
            out
        })
        .collect();
    let total = parts.iter().map(Vec::len).sum();
    let mut rows = Vec::with_capacity(total);
    for p in parts {
        rows.extend_from_slice(&p);
    }
    Ok(rows)
}

/// 정렬 순서를 지키는 컬럼 코드 (split·범주 축에 쓴다). null은 맨 뒤 코드다.
pub struct Ordinal {
    /// rows와 같은 순서의 코드
    pub codes: Vec<u32>,
    pub labels: Vec<String>,
}

impl Ordinal {
    pub fn card(&self) -> usize {
        self.labels.len()
    }
}

/// 고른 행들의 컬럼 값 → 정렬 코드. 범주는 사전 순서, 숫자·시계열은 값 순서다.
pub fn ordinal(col: &Column, rows: &[u32]) -> Ordinal {
    match &col.data {
        ColumnData::Cat { codes, dict } => {
            let null = dict.len() as u32;
            let has_null = rows.par_iter().any(|&r| codes[r as usize] == NULL_CODE);
            let mut labels = dict.clone();
            if has_null {
                labels.push("null".into());
            }
            let out = rows
                .par_iter()
                .map(|&r| {
                    let c = codes[r as usize];
                    if c == NULL_CODE { null } else { c }
                })
                .collect();
            Ordinal { codes: out, labels }
        }
        ColumnData::Num(v) => {
            let mut uniq: Vec<f64> = rows.par_iter().map(|&r| v[r as usize]).filter(|x| !x.is_nan()).collect();
            uniq.par_sort_unstable_by(|a, b| a.total_cmp(b));
            uniq.dedup();
            let has_null = rows.par_iter().any(|&r| v[r as usize].is_nan());
            let null = uniq.len() as u32;
            let codes = rows
                .par_iter()
                .map(|&r| {
                    let x = v[r as usize];
                    if x.is_nan() {
                        null
                    } else {
                        uniq.binary_search_by(|p| p.total_cmp(&x)).unwrap() as u32
                    }
                })
                .collect();
            let time = col.kind == ColumnKind::Datetime;
            let mut labels: Vec<String> = uniq
                .iter()
                .map(|&x| {
                    if time {
                        crate::timefmt::format_kst(x, "%Y-%m-%d %H:%M:%S")
                    } else {
                        crate::dataset::fmt_num(x)
                    }
                })
                .collect();
            if has_null {
                labels.push("null".into());
            }
            Ordinal { codes, labels }
        }
    }
}

/// split 조합으로 나눈 행 (CSR). 그룹은 split 값 순서로 정렬돼 있다.
pub struct Groups {
    pub labels: Vec<Vec<String>>,
    pub offsets: Vec<usize>,
    pub rows: Vec<u32>,
}

impl Groups {
    pub fn len(&self) -> usize {
        self.labels.len()
    }
    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }
    pub fn rows_of(&self, g: usize) -> &[u32] {
        &self.rows[self.offsets[g]..self.offsets[g + 1]]
    }
    pub fn single(rows: Vec<u32>) -> Self {
        let n = rows.len();
        Groups {
            labels: vec![vec![]],
            offsets: vec![0, n],
            rows,
        }
    }
}

/// 행마다 split 키(혼합 진법). 키 순서 = 정렬 순서
fn split_keys(split: &[&Column], rows: &[u32]) -> (Vec<u128>, Vec<Ordinal>, u128) {
    let ords: Vec<Ordinal> = split.iter().map(|c| ordinal(c, rows)).collect();
    let mut radix: u128 = 1;
    for o in &ords {
        radix = radix.saturating_mul(o.card().max(1) as u128);
    }
    let keys = (0..rows.len())
        .into_par_iter()
        .map(|i| {
            let mut k: u128 = 0;
            for o in &ords {
                k = k * o.card().max(1) as u128 + o.codes[i] as u128;
            }
            k
        })
        .collect();
    (keys, ords, radix)
}

fn decode_labels(mut key: u128, ords: &[Ordinal]) -> Vec<String> {
    let mut out = vec![String::new(); ords.len()];
    for (i, o) in ords.iter().enumerate().rev() {
        let card = o.card().max(1) as u128;
        out[i] = o.labels[(key % card) as usize].clone();
        key /= card;
    }
    out
}

/// split 조합 수. limit을 넘으면 Err(조합 수)
pub fn split_count(ds: &Dataset, split_by: &[String], filters: &[Filter], limit: usize) -> QResult<Result<usize, usize>> {
    if split_by.is_empty() {
        return Ok(Ok(1));
    }
    let split: Vec<&Column> = split_by.iter().map(|s| column(ds, s, &[])).collect::<QResult<_>>()?;
    let rows = select_rows(ds, filters, &[])?;
    let (mut keys, _, _) = split_keys(&split, &rows);
    keys.par_sort_unstable();
    keys.dedup();
    let n = keys.len();
    Ok(if n > limit { Err(n) } else { Ok(n) })
}

/// 행들을 split 조합으로 나눈다. 그룹 안의 행 순서는 원래 순서다.
pub fn group_rows(split: &[&Column], rows: Vec<u32>) -> QResult<Groups> {
    if split.is_empty() {
        return Ok(Groups::single(rows));
    }
    let (keys, ords, radix) = split_keys(split, &rows);
    let n = rows.len();
    if radix <= (1 << 22) {
        // 조합 공간이 작으면 계수 정렬 (O(n), 안정)
        let r = radix as usize;
        let mut counts = vec![0usize; r + 1];
        for &k in &keys {
            counts[k as usize + 1] += 1;
        }
        let used: Vec<usize> = (0..r).filter(|&k| counts[k + 1] > 0).collect();
        if used.len() > MAX_SPLIT_ITEMS {
            return Err(QueryError(format!(
                "split 조합이 {}개로 최대 {}개를 넘습니다.",
                used.len(),
                MAX_SPLIT_ITEMS
            )));
        }
        for k in 0..r {
            counts[k + 1] += counts[k];
        }
        let mut out = vec![0u32; n];
        let mut cursor = counts.clone();
        for (i, &k) in keys.iter().enumerate() {
            out[cursor[k as usize]] = rows[i];
            cursor[k as usize] += 1;
        }
        let mut offsets = Vec::with_capacity(used.len() + 1);
        let mut labels = Vec::with_capacity(used.len());
        for &k in &used {
            offsets.push(counts[k]);
            labels.push(decode_labels(k as u128, &ords));
        }
        offsets.push(n);
        return Ok(Groups {
            labels,
            offsets,
            rows: out,
        });
    }
    let mut pairs: Vec<(u128, u32)> = keys.into_iter().zip(rows).collect();
    pairs.par_sort_by_key(|p| p.0);
    let mut offsets = vec![];
    let mut labels = vec![];
    let mut last: Option<u128> = None;
    for (i, (k, _)) in pairs.iter().enumerate() {
        if last != Some(*k) {
            offsets.push(i);
            labels.push(decode_labels(*k, &ords));
            last = Some(*k);
            if labels.len() > MAX_SPLIT_ITEMS {
                return Err(QueryError(format!("split 조합이 최대 {MAX_SPLIT_ITEMS}개를 넘습니다.")));
            }
        }
    }
    offsets.push(n);
    Ok(Groups {
        labels,
        offsets,
        rows: pairs.into_iter().map(|p| p.1).collect(),
    })
}

/// legend 컬럼: 전체 데이터 기준 값 목록과, 행마다 legend 번호(null은 0)
pub struct LegendCodes<'a> {
    pub values: Vec<String>,
    codes: &'a [u32],
}

impl<'a> LegendCodes<'a> {
    pub fn of(ds: &'a Dataset, name: &str) -> QResult<Self> {
        let c = column(ds, name, &[ColumnKind::Categorical])?;
        let (codes, dict) = c.cat().unwrap();
        Ok(Self {
            values: dict.to_vec(),
            codes,
        })
    }
    #[inline]
    pub fn at(&self, row: u32) -> u32 {
        let c = self.codes[row as usize];
        if c == NULL_CODE { 0 } else { c }
    }
}

/// 범주 컬럼 코드(null은 사전 길이)
#[inline]
pub fn cat_code(codes: &[u32], dict_len: usize, row: u32) -> u32 {
    let c = codes[row as usize];
    if c == NULL_CODE { dict_len as u32 } else { c }
}

/// 그룹 단위 병렬 처리. 그룹이 하나뿐이면 안에서 병렬로 나누는 것은 각 쿼리가 맡는다.
pub fn par_groups<T: Send>(groups: &Groups, f: impl Fn(usize, &[u32]) -> T + Sync + Send) -> Vec<T> {
    (0..groups.len()).into_par_iter().map(|g| f(g, groups.rows_of(g))).collect()
}
