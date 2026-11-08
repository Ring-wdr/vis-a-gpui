//! heatmap: x 범주(또는 시계열을 자동 시간 구간으로 묶은 값) × y 범주 칸마다 z를 통계 함수로 집계한다.

use rayon::prelude::*;

use super::agg::agg_by_key;
use super::{Groups, QResult, cat_code, column, par_groups};
use crate::dataset::{ColumnKind, Dataset};
use crate::spec::AggFn;
use crate::timefmt::{DAY_MS, HOUR_MS, KST_OFFSET_MS, format_utc};

const SECOND: f64 = 1000.0;
const MINUTE: f64 = 60_000.0;
const TIME_BINS: [f64; 15] = [
    SECOND,
    5.0 * SECOND,
    15.0 * SECOND,
    30.0 * SECOND,
    MINUTE,
    5.0 * MINUTE,
    15.0 * MINUTE,
    30.0 * MINUTE,
    HOUR_MS,
    3.0 * HOUR_MS,
    6.0 * HOUR_MS,
    12.0 * HOUR_MS,
    DAY_MS,
    7.0 * DAY_MS,
    30.0 * DAY_MS,
];
pub const MAX_TIME_BINS: f64 = 48.0;

fn bin_index(ms: f64, bin: f64) -> i64 {
    ((ms + KST_OFFSET_MS) / bin).floor() as i64
}

fn bin_label(index: i64, bin: f64) -> String {
    let start = index as f64 * bin - KST_OFFSET_MS;
    let fmt = if bin >= DAY_MS {
        "%m/%d"
    } else if bin >= HOUR_MS {
        "%m/%d %H시"
    } else if bin >= MINUTE {
        "%H:%M"
    } else {
        "%H:%M:%S"
    };
    format_utc(start + KST_OFFSET_MS, fmt)
}

pub struct HeatItem {
    pub xs: Vec<String>,
    pub ys: Vec<String>,
    /// [x][y]
    pub cells: Vec<Vec<Option<f64>>>,
}

pub fn query_heatmap(ds: &Dataset, groups: &Groups, x: &str, y: &str, z: &str, agg: AggFn) -> QResult<Vec<HeatItem>> {
    let xcol = column(ds, x, &[ColumnKind::Categorical, ColumnKind::Datetime])?;
    let ycol = column(ds, y, &[ColumnKind::Categorical])?;
    let zcol = column(ds, z, &[ColumnKind::Numeric])?;
    let (ycodes, ydict) = ycol.cat().unwrap();
    let zv = zcol.nums().unwrap();
    let ylen = ydict.len() as u64 + 1;
    let ylabel = |c: u64| {
        if (c as usize) < ydict.len() {
            ydict[c as usize].clone()
        } else {
            "null".into()
        }
    };

    // x 키: 범주 코드 또는 시간 구간 번호 - 최소 구간 번호
    enum XKey<'a> {
        Cat(&'a [u32], &'a [String]),
        Time(&'a [f64], f64, i64, i64),
    }
    let xkey = match xcol.cat() {
        Some((codes, dict)) => XKey::Cat(codes, dict),
        None => {
            let v = xcol.nums().unwrap();
            let (min, max) = groups
                .rows
                .par_iter()
                .map(|&r| v[r as usize])
                .fold(|| (f64::INFINITY, f64::NEG_INFINITY), |(a, b), t| (a.min(t), b.max(t)))
                .reduce(|| (f64::INFINITY, f64::NEG_INFINITY), |(a, b), (c, d)| (a.min(c), b.max(d)));
            let bin = TIME_BINS
                .iter()
                .copied()
                .find(|&b| (bin_index(max, b) - bin_index(min, b) + 1) as f64 <= MAX_TIME_BINS)
                .unwrap_or(30.0 * DAY_MS);
            let first = if min.is_finite() { bin_index(min, bin) } else { 0 };
            let last = if max.is_finite() { bin_index(max, bin) } else { 0 };
            XKey::Time(v, bin, first, last)
        }
    };
    let (xspace, time) = match &xkey {
        XKey::Cat(_, d) => (d.len() as u64 + 1, false),
        XKey::Time(_, _, a, b) => ((b - a + 1) as u64, true),
    };
    let key = |r: u32| -> u64 {
        let xk = match &xkey {
            XKey::Cat(c, d) => cat_code(c, d.len(), r) as u64,
            XKey::Time(v, bin, first, _) => (bin_index(v[r as usize], *bin) - first) as u64,
        };
        xk * ylen + cat_code(ycodes, ydict.len(), r) as u64
    };
    let xlabel = |k: u64| -> String {
        match &xkey {
            XKey::Cat(_, d) => {
                if (k as usize) < d.len() {
                    d[k as usize].clone()
                } else {
                    "null".into()
                }
            }
            XKey::Time(_, bin, first, _) => bin_label(first + k as i64, *bin),
        }
    };
    Ok(par_groups(groups, |_, rows| {
        let cells = agg_by_key(rows, key, zv, agg, Some(xspace * ylen));
        // y는 이 item에 나온 값만, 전체 정렬 순서대로
        let mut yused: Vec<u64> = cells.iter().map(|c| c.0 % ylen).collect();
        yused.sort_unstable();
        yused.dedup();
        let ypos = |y: u64| yused.binary_search(&y).unwrap();
        let mut xs_keys: Vec<u64> = if time {
            if cells.is_empty() {
                vec![]
            } else {
                let a = cells.first().unwrap().0 / ylen;
                let b = cells.iter().map(|c| c.0 / ylen).max().unwrap();
                (a..=b).collect()
            }
        } else {
            cells.iter().map(|c| c.0 / ylen).collect()
        };
        xs_keys.dedup();
        let mut grid = vec![vec![None; yused.len()]; xs_keys.len()];
        for (k, v) in cells {
            let xi = xs_keys.binary_search(&(k / ylen)).unwrap();
            grid[xi][ypos(k % ylen)] = Some(v);
        }
        HeatItem {
            xs: xs_keys.iter().map(|&k| xlabel(k)).collect(),
            ys: yused.iter().map(|&y| ylabel(y)).collect(),
            cells: grid,
        }
    }))
}
