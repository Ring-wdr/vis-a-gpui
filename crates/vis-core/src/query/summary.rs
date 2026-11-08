//! 필터 패널의 컬럼 요약, 데이터 표용 행 조회.

use rayon::prelude::*;

use super::{QResult, column, select_rows};
use crate::dataset::{ColumnData, Dataset, NULL_CODE};
use crate::spec::Filter;

#[derive(Clone, Debug)]
pub enum ColumnSummary {
    Range { min: f64, max: f64, nulls: usize },
    Values { values: Vec<(String, usize)>, nulls: usize },
}

pub fn column_summary(ds: &Dataset, name: &str) -> QResult<ColumnSummary> {
    let c = column(ds, name, &[])?;
    Ok(match &c.data {
        ColumnData::Num(v) => {
            let (min, max, nulls) = v
                .par_chunks(1 << 16)
                .map(|ch| {
                    let mut a = (f64::INFINITY, f64::NEG_INFINITY, 0usize);
                    for &x in ch {
                        if x.is_nan() {
                            a.2 += 1;
                        } else {
                            a.0 = a.0.min(x);
                            a.1 = a.1.max(x);
                        }
                    }
                    a
                })
                .reduce(
                    || (f64::INFINITY, f64::NEG_INFINITY, 0),
                    |a, b| (a.0.min(b.0), a.1.max(b.1), a.2 + b.2),
                );
            ColumnSummary::Range { min, max, nulls }
        }
        ColumnData::Cat { codes, dict } => {
            let n = dict.len();
            let counts = codes
                .par_chunks(1 << 16)
                .map(|ch| {
                    let mut c = vec![0usize; n + 1];
                    for &x in ch {
                        c[if x == NULL_CODE { n } else { x as usize }] += 1;
                    }
                    c
                })
                .reduce(
                    || vec![0usize; n + 1],
                    |mut a, b| {
                        a.iter_mut().zip(b).for_each(|(x, y)| *x += y);
                        a
                    },
                );
            ColumnSummary::Values {
                values: dict.iter().cloned().zip(counts[..n].iter().copied()).collect(),
                nulls: counts[n],
            }
        }
    })
}

/// legend 값 목록 (전체 데이터 기준, 정렬)
pub fn legend_values(ds: &Dataset, name: &str) -> QResult<Vec<String>> {
    let c = column(ds, name, &[crate::dataset::ColumnKind::Categorical])?;
    Ok(c.cat().unwrap().1.to_vec())
}

/// 필터를 건 행 한 페이지 (데이터 표)
pub fn page_rows(ds: &Dataset, filters: &[Filter], offset: usize, limit: usize) -> QResult<(usize, Vec<u32>)> {
    let rows = select_rows(ds, filters, &[])?;
    let total = rows.len();
    Ok((total, rows.into_iter().skip(offset).take(limit).collect()))
}

/// split 조합 하나의 행 한 페이지
pub fn item_rows(
    ds: &Dataset,
    split_by: &[String],
    split_values: &[String],
    filters: &[Filter],
    offset: usize,
    limit: usize,
) -> QResult<(usize, Vec<u32>)> {
    let mut f = filters.to_vec();
    let mut want: Vec<(&[f64], f64)> = vec![];
    for (name, value) in split_by.iter().zip(split_values) {
        let c = column(ds, name, &[])?;
        match &c.data {
            ColumnData::Cat { dict, .. } => {
                // 이 값만 남기기 = 나머지 값을 빼기
                f.push(Filter::Exclude {
                    column: name.clone(),
                    values: dict.iter().filter(|d| *d != value).cloned().collect(),
                });
            }
            ColumnData::Num(v) => {
                let x = if c.kind == crate::dataset::ColumnKind::Datetime {
                    crate::timefmt::parse_datetime(value)
                        .map(|t| t - crate::timefmt::KST_OFFSET_MS)
                        .unwrap_or(f64::NAN)
                } else {
                    value.parse().unwrap_or(f64::NAN)
                };
                want.push((v, x));
            }
        }
    }
    let rows = select_rows(ds, &f, &[])?;
    let rows: Vec<u32> = rows
        .into_iter()
        .filter(|&r| want.iter().all(|(v, x)| v[r as usize] == *x))
        .collect();
    let total = rows.len();
    Ok((total, rows.into_iter().skip(offset).take(limit).collect()))
}
