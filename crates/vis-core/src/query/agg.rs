//! 통계 함수 집계 도우미. `group by 키 → agg(y)`를 키(u64) 단위로 계산한다.

use rayon::prelude::*;

use crate::spec::AggFn;

#[derive(Clone, Copy)]
pub struct Acc {
    pub n: u32,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
}

impl Default for Acc {
    fn default() -> Self {
        Self {
            n: 0,
            sum: 0.0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
        }
    }
}

impl Acc {
    #[inline]
    pub fn add(&mut self, v: f64) {
        self.n += 1;
        self.sum += v;
        if v < self.min {
            self.min = v;
        }
        if v > self.max {
            self.max = v;
        }
    }
    pub fn value(&self, agg: AggFn) -> f64 {
        match agg {
            AggFn::Avg => self.sum / self.n as f64,
            AggFn::Sum => self.sum,
            AggFn::Count => self.n as f64,
            AggFn::Min => self.min,
            AggFn::Max => self.max,
            AggFn::Median => f64::NAN,
        }
    }
}

/// percentile_cont: 정렬된 값에서 p 위치를 선형 보간
pub fn percentile_cont(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let pos = p * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
    }
}

/// 키별 집계. 결과는 키 순서로 정렬돼 있다. `space`는 키가 [0, space) 안이면 넘겨 밀집 배열을 쓴다.
pub fn agg_by_key(rows: &[u32], key: impl Fn(u32) -> u64 + Sync, y: &[f64], agg: AggFn, space: Option<u64>) -> Vec<(u64, f64)> {
    if agg == AggFn::Median {
        let mut pairs: Vec<(u64, f64)> = rows.par_iter().map(|&r| (key(r), y[r as usize])).collect();
        pairs.par_sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut out = Vec::new();
        let mut i = 0;
        while i < pairs.len() {
            let k = pairs[i].0;
            let mut j = i;
            while j < pairs.len() && pairs[j].0 == k {
                j += 1;
            }
            let vals: Vec<f64> = pairs[i..j].iter().map(|p| p.1).collect();
            out.push((k, percentile_cont(&vals, 0.5)));
            i = j;
        }
        return out;
    }
    let accs = acc_by_key(rows, key, y, space);
    accs.into_iter().map(|(k, a)| (k, a.value(agg))).collect()
}

/// 키별 누적값 (median 제외)
pub fn acc_by_key(rows: &[u32], key: impl Fn(u32) -> u64 + Sync, y: &[f64], space: Option<u64>) -> Vec<(u64, Acc)> {
    match space {
        Some(s) if s <= 1 << 20 => {
            let s = s as usize;
            let merge = |mut a: Vec<Acc>, b: Vec<Acc>| {
                for (x, y) in a.iter_mut().zip(b) {
                    if y.n > 0 {
                        x.n += y.n;
                        x.sum += y.sum;
                        x.min = x.min.min(y.min);
                        x.max = x.max.max(y.max);
                    }
                }
                a
            };
            let dense = if rows.len() > 200_000 && s <= 1 << 14 {
                rows.par_chunks(1 << 16)
                    .map(|chunk| {
                        let mut d = vec![Acc::default(); s];
                        for &r in chunk {
                            d[key(r) as usize].add(y[r as usize]);
                        }
                        d
                    })
                    .reduce(|| vec![Acc::default(); s], merge)
            } else {
                let mut d = vec![Acc::default(); s];
                for &r in rows {
                    d[key(r) as usize].add(y[r as usize]);
                }
                d
            };
            dense
                .into_iter()
                .enumerate()
                .filter(|(_, a)| a.n > 0)
                .map(|(k, a)| (k as u64, a))
                .collect()
        }
        _ => {
            let mut map: std::collections::HashMap<u64, Acc> = std::collections::HashMap::new();
            for &r in rows {
                map.entry(key(r)).or_default().add(y[r as usize]);
            }
            let mut v: Vec<(u64, Acc)> = map.into_iter().collect();
            v.sort_unstable_by_key(|p| p.0);
            v
        }
    }
}
