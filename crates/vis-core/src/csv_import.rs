//! CSV 가져오기. 원본 업로드(papaparse + COPY)의 핵심 규칙만 옮겼다:
//! 구분 기호, 첫 행 헤더, Null 값, 타입 추론(모두 숫자 → 숫자, 모두 날짜 시간 → 시계열, 그 밖 → 범주),
//! 데이터 오류 무시(끄면 읽을 수 없는 값이 있을 때 사유와 함께 거절).

use std::path::Path;

use crate::dataset::{CatBuilder, Column, ColumnData, ColumnKind, Dataset};
use crate::timefmt::parse_datetime;

#[derive(Clone, Debug)]
pub struct CsvOptions {
    pub delimiter: u8,
    pub header: bool,
    pub null_value: String,
    pub ignore_errors: bool,
}

impl Default for CsvOptions {
    fn default() -> Self {
        Self {
            delimiter: b',',
            header: true,
            null_value: String::new(),
            ignore_errors: true,
        }
    }
}

const MAX_COLUMNS: usize = 200;

pub fn import(path: &Path, id: &str, opts: &CsvOptions, mut progress: impl FnMut(f32)) -> Result<Dataset, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("파일을 읽을 수 없습니다: {e}"))?;
    let text = String::from_utf8_lossy(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes)).into_owned();
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(opts.delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    let mut records: Vec<csv::StringRecord> = Vec::new();
    for (i, r) in rdr.records().enumerate() {
        match r {
            Ok(r) => {
                if r.len() == 1 && r[0].trim().is_empty() {
                    continue;
                }
                records.push(r)
            }
            Err(e) if !opts.ignore_errors => return Err(format!("{}행을 읽을 수 없습니다: {e}", i + 1)),
            Err(_) => {}
        }
    }
    if records.is_empty() {
        return Err("데이터가 없습니다.".into());
    }
    let header: Vec<String> = if opts.header {
        records.remove(0).iter().map(|s| s.trim().to_string()).collect()
    } else {
        (1..=records[0].len()).map(|i| format!("column_{i}")).collect()
    };
    let ncols = header.len();
    if ncols > MAX_COLUMNS {
        return Err(format!("컬럼이 {ncols}개로 최대 {MAX_COLUMNS}개를 넘습니다."));
    }
    let rows: Vec<&csv::StringRecord> = records
        .iter()
        .enumerate()
        .filter(|(i, r)| {
            if r.len() == ncols {
                return true;
            }
            let _ = i;
            false
        })
        .map(|(_, r)| r)
        .collect();
    if !opts.ignore_errors && rows.len() != records.len() {
        let bad = records.iter().position(|r| r.len() != ncols).unwrap();
        return Err(format!(
            "{}행의 필드 수가 헤더({ncols}개)와 다릅니다.",
            bad + 1 + opts.header as usize
        ));
    }
    let is_null = |s: &str| s == opts.null_value || (opts.null_value.is_empty() && s.trim().is_empty());
    let mut columns = Vec::with_capacity(ncols);
    for (c, name) in header.iter().enumerate() {
        // 타입 추론
        let mut all_num = true;
        let mut all_time = true;
        let mut any = false;
        for r in &rows {
            let v = &r[c];
            if is_null(v) {
                continue;
            }
            any = true;
            if all_num && v.trim().parse::<f64>().is_err() {
                all_num = false;
            }
            if all_time && parse_datetime(v).is_none() {
                all_time = false;
            }
            if !all_num && !all_time {
                break;
            }
        }
        let kind = if any && all_num {
            ColumnKind::Numeric
        } else if any && all_time {
            ColumnKind::Datetime
        } else {
            ColumnKind::Categorical
        };
        let data = match kind {
            ColumnKind::Numeric => ColumnData::Num(
                rows.iter()
                    .map(|r| {
                        if is_null(&r[c]) {
                            f64::NAN
                        } else {
                            r[c].trim().parse().unwrap_or(f64::NAN)
                        }
                    })
                    .collect(),
            ),
            ColumnKind::Datetime => ColumnData::Num(
                rows.iter()
                    .map(|r| {
                        if is_null(&r[c]) {
                            f64::NAN
                        } else {
                            parse_datetime(&r[c]).unwrap_or(f64::NAN)
                        }
                    })
                    .collect(),
            ),
            ColumnKind::Categorical => {
                let mut b = CatBuilder::with_capacity(rows.len());
                for r in &rows {
                    let v = &r[c];
                    b.push(if is_null(v) { None } else { Some(v) });
                }
                b.finish()
            }
        };
        let name = if name.is_empty() {
            format!("column_{}", c + 1)
        } else {
            name.clone()
        };
        columns.push(Column {
            label: name.clone(),
            name,
            kind,
            data,
        });
        progress((c + 1) as f32 / ncols as f32);
    }
    let file = path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| id.to_string());
    Ok(Dataset {
        id: id.into(),
        name: file,
        rows: rows.len(),
        columns,
        created_ms: chrono::Utc::now().timestamp_millis(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_types() {
        let dir = std::env::temp_dir().join("vis-csv-test.csv");
        std::fs::write(&dir, "ts,name,v\n2026-09-01 00:00:00,a,1.5\n2026-09-02 00:00:00,b,\n").unwrap();
        let d = import(&dir, "up-1", &CsvOptions::default(), |_| {}).unwrap();
        assert_eq!(d.rows, 2);
        assert_eq!(d.columns[0].kind, ColumnKind::Datetime);
        assert_eq!(d.columns[1].kind, ColumnKind::Categorical);
        assert_eq!(d.columns[2].kind, ColumnKind::Numeric);
        assert!(d.columns[2].nums().unwrap()[1].is_nan());
    }
}
