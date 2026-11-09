//! 범주 x × 숫자 y 집계: bar, boxplot과 그 집계를 쓰는 pie·radar·treemap·waterfall·funnel.
//! 원본 category-data.ts / polar-data.ts / funnel-data.ts의 규칙을 따른다.

use rayon::prelude::*;

use super::agg::{agg_by_key, percentile_cont};
use super::{Groups, LegendCodes, QResult, cat_code, column, par_groups};
use crate::dataset::{ColumnKind, Dataset};
use crate::spec::{AggFn, ChartType};

pub const OTHERS: &str = "기타";
const OUTLIER_SAMPLE: usize = 200;
pub const PIE_TOP: usize = 8;
pub const FUNNEL_MAX: usize = 8;
pub const RADAR_MAX: usize = 12;
pub const RADAR_MIN: usize = 3;

#[derive(Clone, Debug)]
pub struct BoxStats {
    pub n: u32,
    pub mean: f64,
    pub q1: f64,
    pub median: f64,
    pub q3: f64,
    pub lo: f64,
    pub hi: f64,
    pub outlier_count: u32,
    pub outliers: Vec<f64>,
    /// boxplot dot 하위 차트: 원값 표본 (고르게 최대 DOT_SAMPLE개)
    pub dots: Vec<f64>,
}

pub const DOT_SAMPLE: usize = 200;

/// item 하나의 [범주][legend 값] 칸
#[derive(Clone, Debug, Default)]
pub struct CategoryItem {
    pub categories: Vec<String>,
    /// 범주별 행 수 (pie·funnel 비율 등)
    pub bars: Vec<Vec<Option<f64>>>,
    pub boxes: Vec<Vec<Option<BoxStats>>>,
    /// 마지막 범주가 '기타' 묶음인지
    pub has_others: bool,
}

pub struct CategoryQuery<'a> {
    pub ds: &'a Dataset,
    pub chart: ChartType,
    pub x: &'a str,
    pub y: &'a str,
    pub agg: AggFn,
    pub legend: Option<&'a str>,
}

/// 범주 집계. groups는 이미 필터·split을 거친 행이다 (y 값이 있는 행만).
pub fn query_category(q: &CategoryQuery, groups: &Groups) -> QResult<(Vec<CategoryItem>, Option<Vec<String>>)> {
    let xcol = column(q.ds, q.x, &[ColumnKind::Categorical])?;
    let ycol = column(q.ds, q.y, &[ColumnKind::Numeric])?;
    let (xcodes, xdict) = xcol.cat().unwrap();
    let y = ycol.nums().unwrap();
    let legend = match q.legend {
        Some(l) => Some(LegendCodes::of(q.ds, l)?),
        None => None,
    };
    let lw = legend.as_ref().map(|l| l.values.len().max(1)).unwrap_or(1) as u64;
    let xlen = xdict.len();
    let label = |c: u64| -> String {
        if (c as usize) < xlen {
            xdict[c as usize].clone()
        } else {
            "null".into()
        }
    };
    let key = |r: u32| -> u64 {
        let xc = cat_code(xcodes, xlen, r) as u64;
        xc * lw + legend.as_ref().map(|l| l.at(r) as u64).unwrap_or(0)
    };
    let space = Some((xlen as u64 + 1) * lw);
    let lkey = |r: u32| legend.as_ref().map(|l| l.at(r) as u64).unwrap_or(0);

    let items = if q.chart == ChartType::Boxplot {
        par_groups(groups, |_, rows| {
            let mut pairs: Vec<(u64, f64)> = rows.iter().map(|&r| (key(r), y[r as usize])).collect();
            if pairs.len() > 100_000 {
                pairs.par_sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
            } else {
                pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
            }
            let mut item = CategoryItem::default();
            let mut i = 0;
            let mut vals = Vec::new();
            while i < pairs.len() {
                let k = pairs[i].0;
                let mut j = i;
                vals.clear();
                while j < pairs.len() && pairs[j].0 == k {
                    vals.push(pairs[j].1);
                    j += 1;
                }
                let (xc, lc) = (k / lw, (k % lw) as usize);
                let name = label(xc);
                if item.categories.last() != Some(&name) {
                    item.categories.push(name);
                    item.boxes.push(vec![None; lw as usize]);
                }
                item.boxes.last_mut().unwrap()[lc] = Some(box_stats(&vals));
                i = j;
            }
            item
        })
    } else {
        par_groups(groups, |_, rows| {
            let cells = agg_by_key(rows, key, y, q.agg, space);
            let mut item = CategoryItem::default();
            for (k, v) in cells {
                let (xc, lc) = (k / lw, (k % lw) as usize);
                let name = label(xc);
                if item.categories.last() != Some(&name) {
                    item.categories.push(name);
                    item.bars.push(vec![None; lw as usize]);
                }
                item.bars.last_mut().unwrap()[lc] = Some(v);
            }
            match q.chart {
                ChartType::Pie => top_with_others(item, rows, PIE_TOP + 1, true, xcodes, xdict, y, q.agg, lw, &lkey),
                ChartType::Funnel => top_with_others(item, rows, FUNNEL_MAX, false, xcodes, xdict, y, q.agg, lw, &lkey),
                ChartType::Radar => radar_axes(item),
                _ => item,
            }
        })
    };
    Ok((items, legend.map(|l| l.values)))
}

fn box_stats(sorted: &[f64]) -> BoxStats {
    let n = sorted.len();
    let mean = sorted.iter().sum::<f64>() / n as f64;
    let q1 = percentile_cont(sorted, 0.25);
    let median = percentile_cont(sorted, 0.5);
    let q3 = percentile_cont(sorted, 0.75);
    let low = q1 - 1.5 * (q3 - q1);
    let high = q3 + 1.5 * (q3 - q1);
    let lo = sorted.iter().copied().find(|&v| v >= low).unwrap_or(q1);
    let hi = sorted.iter().rev().copied().find(|&v| v <= high).unwrap_or(q3);
    let mut outliers = Vec::new();
    let mut count = 0;
    for &v in sorted {
        if v < low || v > high {
            count += 1;
            if outliers.len() < OUTLIER_SAMPLE {
                outliers.push(v);
            }
        }
    }
    BoxStats {
        n: n as u32,
        mean,
        q1,
        median,
        q3,
        lo,
        hi,
        outlier_count: count,
        outliers,
        dots: if n <= DOT_SAMPLE {
            sorted.to_vec()
        } else {
            (0..DOT_SAMPLE).map(|k| sorted[k * (n - 1) / (DOT_SAMPLE - 1)]).collect()
        },
    }
}

/// 값 큰 순으로 max - 1개만 남기고 나머지 행은 '기타' 하나로 다시 집계한다 (평균·중앙값도 맞게).
/// positive면 0 이하 값은 뺀다 (pie).
#[allow(clippy::too_many_arguments)]
fn top_with_others(
    item: CategoryItem,
    rows: &[u32],
    max: usize,
    positive: bool,
    xcodes: &[u32],
    xdict: &[String],
    y: &[f64],
    agg: AggFn,
    lw: u64,
    lkey: &(dyn Fn(u32) -> u64 + Sync),
) -> CategoryItem {
    // legend가 있으면(pie sunburst) 범주 값은 legend 값별 몫의 합이고, 칸 전체를 그대로 가져간다
    let mut ranked: Vec<(String, f64, Vec<Option<f64>>)> = item
        .categories
        .into_iter()
        .zip(item.bars)
        .filter_map(|(c, cell)| {
            let v = if lw > 1 {
                let s: f64 = cell.iter().flatten().filter(|v| !positive || **v > 0.0).sum();
                (s != 0.0).then_some(s)
            } else {
                cell[0]
            };
            v.map(|v| (c, v, cell))
        })
        .filter(|(_, v, _)| v.is_finite() && (!positive || *v > 0.0))
        .collect();
    // 큰 순, 같으면 이름 순 (안정 정렬이라 원래 순서가 이름 순이다)
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut out = CategoryItem::default();
    if ranked.len() <= max {
        for (c, _, cell) in ranked {
            out.categories.push(c);
            out.bars.push(cell);
        }
        return out;
    }
    let keep: std::collections::HashSet<String> = ranked[..max - 1].iter().map(|r| r.0.clone()).collect();
    for (c, _, cell) in ranked.iter().take(max - 1) {
        out.categories.push(c.clone());
        out.bars.push(cell.clone());
    }
    let rest: Vec<u32> = rows
        .iter()
        .copied()
        .filter(|&r| {
            let c = xcodes[r as usize];
            let name = if (c as usize) < xdict.len() {
                xdict[c as usize].as_str()
            } else {
                "null"
            };
            !keep.contains(name)
        })
        .filter(|&r| !positive || y[r as usize] > 0.0 || agg == AggFn::Count)
        .collect();
    let mut cell = vec![None; lw as usize];
    for (k, v) in agg_by_key(&rest, lkey, y, agg, Some(lw)) {
        if !positive || v > 0.0 {
            cell[k as usize] = Some(v);
        }
    }
    if cell.iter().any(|v| v.is_some()) {
        out.categories.push(OTHERS.into());
        out.bars.push(cell);
        out.has_others = true;
    }
    out
}

/// radar: 축이 RADAR_MAX를 넘으면 계열 값의 절댓값 합이 큰 축만 남기고 원래 순서를 지킨다.
fn radar_axes(item: CategoryItem) -> CategoryItem {
    if item.categories.len() <= RADAR_MAX {
        return item;
    }
    let mut score: Vec<(usize, f64)> = item
        .bars
        .iter()
        .enumerate()
        .map(|(i, cell)| (i, cell.iter().flatten().map(|v| v.abs()).sum()))
        .collect();
    score.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut keep: Vec<usize> = score[..RADAR_MAX].iter().map(|s| s.0).collect();
    keep.sort();
    CategoryItem {
        categories: keep.iter().map(|&i| item.categories[i].clone()).collect(),
        bars: keep.iter().map(|&i| item.bars[i].clone()).collect(),
        boxes: vec![],
        has_others: false,
    }
}
