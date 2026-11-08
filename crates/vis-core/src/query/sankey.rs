//! sankey: x 값(출발) → y 값(도착) 흐름을 z의 통계 함수로 집계한다.
//! 출발 노드는 상위 7개 + 기타, 도착 노드는 상위 11개 + 기타로 묶고, '기타'는 원래 행 단위로 다시 집계한다.

use rayon::prelude::*;

use super::agg::agg_by_key;
use super::{Groups, cat_code, par_groups};
use crate::query::category::OTHERS;
use crate::spec::AggFn;

pub const MAX_SOURCES: usize = 8;
pub const MAX_TARGETS: usize = 12;

pub struct SankeyItem {
    pub sources: Vec<String>,
    pub targets: Vec<String>,
    /// (출발, 도착, 값)
    pub flows: Vec<(usize, usize, f64)>,
}

/// 전체 데이터에서 흐름(합)이 큰 순서로 노드를 줄 세워 상한까지 남긴다. 반환: 코드 → 노드 번호, 노드 이름
fn rank_nodes(rows: &[u32], codes: &[u32], dict: &[String], z: &[f64], max: usize) -> (Vec<usize>, Vec<String>) {
    let n = dict.len() + 1;
    let totals = rows
        .par_chunks(1 << 16)
        .map(|chunk| {
            let mut t = vec![0f64; n];
            for &r in chunk {
                t[cat_code(codes, dict.len(), r) as usize] += z[r as usize].abs();
            }
            t
        })
        .reduce(
            || vec![0f64; n],
            |mut a, b| {
                a.iter_mut().zip(b).for_each(|(x, y)| *x += y);
                a
            },
        );
    let mut order: Vec<usize> = (0..n).filter(|&i| totals[i] > 0.0).collect();
    order.sort_by(|&a, &b| totals[b].total_cmp(&totals[a]));
    let name = |i: usize| if i < dict.len() { dict[i].clone() } else { "null".into() };
    let mut map = vec![usize::MAX; n];
    let mut names = Vec::new();
    if order.len() <= max {
        for (slot, &i) in order.iter().enumerate() {
            map[i] = slot;
            names.push(name(i));
        }
    } else {
        for (slot, &i) in order.iter().take(max - 1).enumerate() {
            map[i] = slot;
            names.push(name(i));
        }
        names.push(OTHERS.into());
        for &i in &order[max - 1..] {
            map[i] = max - 1;
        }
    }
    (map, names)
}

#[allow(clippy::too_many_arguments)]
pub fn query_sankey(
    groups: &Groups,
    xcodes: &[u32],
    xdict: &[String],
    ycodes: &[u32],
    ydict: &[String],
    z: &[f64],
    agg: AggFn,
) -> Vec<SankeyItem> {
    let (smap, snames) = rank_nodes(&groups.rows, xcodes, xdict, z, MAX_SOURCES);
    let (tmap, tnames) = rank_nodes(&groups.rows, ycodes, ydict, z, MAX_TARGETS);
    let tn = tnames.len().max(1) as u64;
    par_groups(groups, |_, rows| {
        let rows: Vec<u32> = rows
            .iter()
            .copied()
            .filter(|&r| {
                smap[cat_code(xcodes, xdict.len(), r) as usize] != usize::MAX
                    && tmap[cat_code(ycodes, ydict.len(), r) as usize] != usize::MAX
            })
            .collect();
        let key =
            |r: u32| smap[cat_code(xcodes, xdict.len(), r) as usize] as u64 * tn + tmap[cat_code(ycodes, ydict.len(), r) as usize] as u64;
        let cells = agg_by_key(&rows, key, z, agg, Some(snames.len() as u64 * tn));
        SankeyItem {
            sources: snames.clone(),
            targets: tnames.clone(),
            flows: cells
                .into_iter()
                .filter(|(_, v)| v.is_finite() && *v > 0.0)
                .map(|(k, v)| ((k / tn) as usize, (k % tn) as usize, v))
                .collect(),
        }
    })
}
