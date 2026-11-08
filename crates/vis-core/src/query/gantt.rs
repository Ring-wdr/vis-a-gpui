//! gantt: y 범주(작업 행)마다 시간순으로, legend 값이 같은 채로 이어지는 행들을 막대 하나로 묶는다
//! (gaps-and-islands). legend가 없으면 행마다 처음~마지막 시각 막대 하나.

use super::{Groups, LegendCodes, cat_code, par_groups};

#[derive(Clone, Copy, Debug)]
pub struct GanttBar {
    pub row: u32,
    pub legend: u32,
    pub start: f64,
    pub end: f64,
    pub count: u32,
}

pub struct GanttItem {
    pub rows: Vec<String>,
    pub bars: Vec<GanttBar>,
}

pub fn query_gantt(groups: &Groups, ts: &[f64], ycodes: &[u32], ydict: &[String], legend: Option<&LegendCodes>) -> Vec<GanttItem> {
    let ylen = ydict.len();
    par_groups(groups, |_, rows| {
        let mut sorted: Vec<(u32, f64, u32)> = rows
            .iter()
            .map(|&r| (cat_code(ycodes, ylen, r), ts[r as usize], legend.map(|l| l.at(r)).unwrap_or(0)))
            .collect();
        sorted.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut labels = Vec::new();
        let mut bars: Vec<GanttBar> = Vec::new();
        let mut i = 0;
        while i < sorted.len() {
            let y = sorted[i].0;
            let row = labels.len() as u32;
            labels.push(if (y as usize) < ylen {
                ydict[y as usize].clone()
            } else {
                "null".into()
            });
            let mut j = i;
            while j < sorted.len() && sorted[j].0 == y {
                j += 1;
            }
            let seg = &sorted[i..j];
            if legend.is_none() {
                bars.push(GanttBar {
                    row,
                    legend: 0,
                    start: seg[0].1,
                    end: seg[seg.len() - 1].1,
                    count: seg.len() as u32,
                });
            } else {
                let first_bar = bars.len();
                let mut k = 0;
                while k < seg.len() {
                    let l = seg[k].2;
                    let mut e = k;
                    while e < seg.len() && seg[e].2 == l {
                        e += 1;
                    }
                    bars.push(GanttBar {
                        row,
                        legend: l,
                        start: seg[k].1,
                        end: seg[e - 1].1,
                        count: (e - k) as u32,
                    });
                    k = e;
                }
                // 상태는 다음 구간이 시작될 때까지 이어진 것으로 본다
                for b in first_bar..bars.len().saturating_sub(1) {
                    bars[b].end = bars[b + 1].start;
                }
            }
            i = j;
        }
        GanttItem { rows: labels, bars }
    })
}
