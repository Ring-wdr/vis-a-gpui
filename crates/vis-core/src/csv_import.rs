//! CSV 가져오기 (원본 3.5 업로드). 전처리 옵션: 구분 기호, 첫 행 헤더, Null 값, 따옴표, 데이터 오류 무시,
//! 인코딩(자동: UTF-8, 깨지면 EUC-KR), 컬럼별 타입(자동 추론 / 범주 / 숫자 / 시계열)과 포함 여부.
//! CSV 하나 또는 CSV를 묶은 zip(안의 CSV를 이름순으로 한 데이터셋으로 합친다. 스키마가 다르면 전체 거절).
//! 미리보기는 앞부분(1MB, 100행)만, 검사는 전체를 읽어 행 수와 컬럼별 변환 불가 값 수를 센다.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::dataset::{CatBuilder, Column, ColumnData, ColumnKind, Dataset};
use crate::timefmt::parse_datetime;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Encoding {
    /// UTF-8, 깨지면 EUC-KR
    #[default]
    Auto,
    Utf8,
    EucKr,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CsvOptions {
    pub delimiter: u8,
    pub header: bool,
    /// 이 값과 같은 필드를 null로 (빈 값이면 빈 필드)
    pub null_value: String,
    pub ignore_errors: bool,
    #[serde(default = "dq")]
    pub quote: u8,
    #[serde(default)]
    pub encoding: Encoding,
    /// 컬럼별 타입 (없으면 자동 추론)
    #[serde(default)]
    pub types: BTreeMap<String, ColumnKind>,
    /// 빼는 컬럼
    #[serde(default)]
    pub exclude: Vec<String>,
    /// 데이터 이름 (비우면 파일 이름)
    #[serde(default)]
    pub name: String,
}

fn dq() -> u8 {
    b'"'
}

impl Default for CsvOptions {
    fn default() -> Self {
        Self {
            delimiter: b',',
            header: true,
            null_value: String::new(),
            ignore_errors: true,
            quote: b'"',
            encoding: Encoding::Auto,
            types: BTreeMap::new(),
            exclude: vec![],
            name: String::new(),
        }
    }
}

const MAX_COLUMNS: usize = 200;
const PREVIEW_BYTES: usize = 1 << 20;
pub const PREVIEW_ROWS: usize = 100;

/// 파일 하나 (zip이면 안의 CSV 하나)
pub struct SourceFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// CSV 하나 또는 zip 안의 CSV들 (이름순, 폴더 포함, macOS 메타 파일 제외)
pub fn read_sources(path: &Path) -> Result<Vec<SourceFile>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("파일을 읽을 수 없습니다: {e}"))?;
    let name = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    if !bytes.starts_with(b"PK\x03\x04") {
        return Ok(vec![SourceFile { name, bytes }]);
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("zip을 열 수 없습니다: {e}"))?;
    let mut out = vec![];
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        let n = f.name().map(|s| s.to_string()).unwrap_or_default();
        if f.is_dir() || n.starts_with("__MACOSX/") || n.rsplit('/').next().is_some_and(|b| b.starts_with("._")) {
            continue;
        }
        if !n.to_lowercase().ends_with(".csv") {
            continue;
        }
        let mut buf = vec![];
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        out.push(SourceFile { name: n, bytes: buf });
    }
    if out.is_empty() {
        return Err("zip 안에 CSV 파일이 없습니다.".into());
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// 바이트 → 글자 (BOM은 뗀다)
pub fn decode(bytes: &[u8], enc: Encoding) -> String {
    let b = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    match enc {
        Encoding::Utf8 => String::from_utf8_lossy(b).into_owned(),
        Encoding::EucKr => encoding_rs::EUC_KR.decode(b).0.into_owned(),
        Encoding::Auto => match std::str::from_utf8(b) {
            Ok(s) => s.to_string(),
            // 끝이 잘린 미리보기면 마지막 글자만 깨질 수 있어 앞부분으로 다시 본다
            Err(e) if e.error_len().is_none() => String::from_utf8_lossy(&b[..e.valid_up_to()]).into_owned(),
            Err(_) => encoding_rs::EUC_KR.decode(b).0.into_owned(),
        },
    }
}

struct Parsed {
    header: Vec<String>,
    rows: Vec<Vec<String>>,
    /// 필드 수가 헤더와 다르거나 읽지 못한 행
    bad_rows: usize,
}

fn parse(text: &str, opts: &CsvOptions, max_rows: Option<usize>) -> Result<Parsed, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(opts.delimiter)
        .quote(opts.quote)
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    let mut header: Option<Vec<String>> = None;
    let mut rows = vec![];
    let mut bad = 0;
    for (i, r) in rdr.records().enumerate() {
        let r = match r {
            Ok(r) => r,
            Err(e) if !opts.ignore_errors => return Err(format!("{}행을 읽을 수 없습니다: {e}", i + 1)),
            Err(_) => {
                bad += 1;
                continue;
            }
        };
        if r.len() == 1 && r[0].trim().is_empty() {
            continue;
        }
        if header.is_none() {
            if opts.header {
                header = Some(r.iter().map(|s| s.trim().to_string()).collect());
                continue;
            }
            header = Some((1..=r.len()).map(|i| format!("column_{i}")).collect());
        }
        let n = header.as_ref().unwrap().len();
        if r.len() != n {
            if !opts.ignore_errors {
                return Err(format!("{}행의 필드 수({})가 헤더({n}개)와 다릅니다.", i + 1, r.len()));
            }
            bad += 1;
            continue;
        }
        rows.push(r.iter().map(String::from).collect());
        if max_rows.is_some_and(|m| rows.len() >= m) {
            break;
        }
    }
    let mut header = header.ok_or("데이터가 없습니다.")?;
    for (i, h) in header.iter_mut().enumerate() {
        if h.is_empty() {
            *h = format!("column_{}", i + 1);
        }
    }
    if header.len() > MAX_COLUMNS {
        return Err(format!("컬럼이 {}개로 최대 {MAX_COLUMNS}개를 넘습니다.", header.len()));
    }
    Ok(Parsed {
        header,
        rows,
        bad_rows: bad,
    })
}

fn is_null(opts: &CsvOptions, s: &str) -> bool {
    s == opts.null_value || (opts.null_value.is_empty() && s.trim().is_empty())
}

/// 값으로 타입 추론: 모두 숫자면 숫자, 모두 시계열이면 시계열, 아니면 범주
pub fn infer(values: &[&str], opts: &CsvOptions) -> ColumnKind {
    let (mut num, mut time, mut any) = (true, true, false);
    for v in values {
        if is_null(opts, v) {
            continue;
        }
        any = true;
        if num && v.trim().parse::<f64>().is_err() {
            num = false;
        }
        if time && parse_datetime(v).is_none() {
            time = false;
        }
        if !num && !time {
            break;
        }
    }
    if any && num {
        ColumnKind::Numeric
    } else if any && time {
        ColumnKind::Datetime
    } else {
        ColumnKind::Categorical
    }
}

/// 값 하나를 타입으로 바꿀 수 있는지
pub fn convertible(v: &str, kind: ColumnKind, opts: &CsvOptions) -> bool {
    is_null(opts, v)
        || match kind {
            ColumnKind::Numeric => v.trim().parse::<f64>().is_ok(),
            ColumnKind::Datetime => parse_datetime(v).is_some(),
            ColumnKind::Categorical => true,
        }
}

/// 미리보기: 첫 파일의 앞부분(1MB, 100행). kinds는 앞부분으로 추론한 타입 (옵션에 정한 타입이 아니라)
pub struct Preview {
    pub files: Vec<String>,
    /// 원본 앞부분 글자 (가운데 표시)
    pub raw: String,
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub inferred: Vec<ColumnKind>,
}

pub fn preview(path: &Path, opts: &CsvOptions) -> Result<Preview, String> {
    let files = read_sources(path)?;
    let first = &files[0];
    let head = &first.bytes[..first.bytes.len().min(PREVIEW_BYTES)];
    let text = decode(head, opts.encoding);
    let p = parse(&text, opts, Some(PREVIEW_ROWS))?;
    let inferred = (0..p.header.len())
        .map(|c| infer(&p.rows.iter().map(|r| r[c].as_str()).collect::<Vec<_>>(), opts))
        .collect();
    Ok(Preview {
        files: files.iter().map(|f| f.name.clone()).collect(),
        raw: text.lines().take(30).collect::<Vec<_>>().join("\n"),
        header: p.header,
        rows: p.rows,
        inferred,
    })
}

/// 검사: 적재하지 않고 전체를 읽어 행 수와 컬럼별 변환 불가 값 수
pub struct Inspect {
    pub rows: usize,
    pub bad_rows: usize,
    pub header: Vec<String>,
    /// 전체 기준 추론 타입
    pub inferred: Vec<ColumnKind>,
    /// 정한 타입(없으면 추론)으로 바꿀 수 없는 값 수
    pub bad_values: Vec<usize>,
}

fn load_all(path: &Path, opts: &CsvOptions) -> Result<(Parsed, Vec<String>), String> {
    let files = read_sources(path)?;
    let mut all: Option<Parsed> = None;
    let mut first_name = String::new();
    for f in &files {
        let p = parse(&decode(&f.bytes, opts.encoding), opts, None)?;
        match &mut all {
            None => {
                first_name = f.name.clone();
                all = Some(p);
            }
            Some(a) => {
                // zip: 스키마(컬럼 수, 이름)가 다르면 전체를 거절
                if a.header != p.header {
                    let diff = a
                        .header
                        .iter()
                        .zip(&p.header)
                        .position(|(x, y)| x != y)
                        .map(|i| format!("{}번째 컬럼 '{}' ≠ '{}'", i + 1, a.header[i], p.header[i]))
                        .unwrap_or_else(|| format!("컬럼 수 {} ≠ {}", a.header.len(), p.header.len()));
                    return Err(format!("'{}'의 스키마가 '{first_name}'와 다릅니다: {diff}", f.name));
                }
                a.rows.extend(p.rows);
                a.bad_rows += p.bad_rows;
            }
        }
    }
    let names = files.iter().map(|f| f.name.clone()).collect();
    Ok((all.ok_or("데이터가 없습니다.")?, names))
}

pub fn inspect(path: &Path, opts: &CsvOptions) -> Result<Inspect, String> {
    let (p, _) = load_all(path, opts)?;
    let mut inferred = vec![];
    let mut bad_values = vec![];
    for (c, h) in p.header.iter().enumerate() {
        let vals: Vec<&str> = p.rows.iter().map(|r| r[c].as_str()).collect();
        let auto = infer(&vals, opts);
        let kind = opts.types.get(h).copied().unwrap_or(auto);
        inferred.push(auto);
        bad_values.push(vals.iter().filter(|v| !convertible(v, kind, opts)).count());
    }
    Ok(Inspect {
        rows: p.rows.len(),
        bad_rows: p.bad_rows,
        header: p.header,
        inferred,
        bad_values,
    })
}

pub fn import(path: &Path, id: &str, opts: &CsvOptions, mut progress: impl FnMut(f32)) -> Result<Dataset, String> {
    let (p, files) = load_all(path, opts)?;
    progress(0.2);
    let keep: Vec<usize> = (0..p.header.len()).filter(|&c| !opts.exclude.contains(&p.header[c])).collect();
    if keep.is_empty() {
        return Err("포함할 컬럼이 없습니다.".into());
    }
    let mut columns = Vec::with_capacity(keep.len());
    for (k, &c) in keep.iter().enumerate() {
        let name = p.header[c].clone();
        let vals: Vec<&str> = p.rows.iter().map(|r| r[c].as_str()).collect();
        let kind = opts.types.get(&name).copied().unwrap_or_else(|| infer(&vals, opts));
        if !opts.ignore_errors {
            if let Some(i) = vals.iter().position(|v| !convertible(v, kind, opts)) {
                return Err(format!("'{name}' 컬럼 {}행의 값 '{}'을(를) 바꿀 수 없습니다.", i + 1, vals[i]));
            }
        }
        let data = match kind {
            ColumnKind::Numeric => ColumnData::Num(
                vals.iter()
                    .map(|v| {
                        if is_null(opts, v) {
                            f64::NAN
                        } else {
                            v.trim().parse().unwrap_or(f64::NAN)
                        }
                    })
                    .collect(),
            ),
            ColumnKind::Datetime => ColumnData::Num(
                vals.iter()
                    .map(|v| {
                        if is_null(opts, v) {
                            f64::NAN
                        } else {
                            parse_datetime(v).unwrap_or(f64::NAN)
                        }
                    })
                    .collect(),
            ),
            ColumnKind::Categorical => {
                let mut b = CatBuilder::with_capacity(vals.len());
                for v in &vals {
                    b.push(if is_null(opts, v) { None } else { Some(v) });
                }
                b.finish()
            }
        };
        columns.push(Column {
            label: name.clone(),
            name,
            kind,
            data,
        });
        progress(0.2 + 0.8 * (k + 1) as f32 / keep.len() as f32);
    }
    let name = if !opts.name.trim().is_empty() {
        opts.name.trim().to_string()
    } else if files.len() > 1 {
        path.file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| id.to_string())
    } else {
        files[0].rsplit('/').next().unwrap_or(&files[0]).to_string()
    };
    Ok(Dataset {
        id: id.into(),
        name,
        rows: p.rows.len(),
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

    #[test]
    fn options_types_exclude_and_euckr() {
        let dir = std::env::temp_dir().join("vis-csv-test2.csv");
        let (bytes, _, _) = encoding_rs::EUC_KR.encode("이름;값;버림\n가;1;x\n나;NA;y\n");
        std::fs::write(&dir, &bytes).unwrap();
        let mut o = CsvOptions {
            delimiter: b';',
            null_value: "NA".into(),
            ..Default::default()
        };
        o.types.insert("값".into(), ColumnKind::Categorical);
        o.exclude.push("버림".into());
        let d = import(&dir, "up-2", &o, |_| {}).unwrap();
        assert_eq!(d.columns.len(), 2);
        assert_eq!(d.columns[0].name, "이름");
        assert_eq!(d.columns[1].kind, ColumnKind::Categorical);
        let ins = inspect(
            &dir,
            &CsvOptions {
                delimiter: b';',
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(ins.rows, 2);
        assert_eq!(ins.inferred[1], ColumnKind::Categorical);
    }
}
