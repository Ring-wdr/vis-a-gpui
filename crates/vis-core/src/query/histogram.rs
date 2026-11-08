//! histogram: x 숫자 컬럼 분포. 구간은 (필터를 건) 전체 범위에서 한 번 정해 모든 item이 같은 구간을 쓴다.

use rayon::prelude::*;

use super::{Groups, LegendCodes, par_groups};

pub const TARGET_BINS: f64 = 30.0;
const NICE_FACTORS: [f64; 5] = [1.0, 2.0, 2.5, 5.0, 10.0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bins {
    pub lo: f64,
    pub step: f64,
    pub count: usize,
}

impl Bins {
    pub fn hi(&self) -> f64 {
        self.lo + self.step * self.count as f64
    }
    #[inline]
    pub fn index(&self, v: f64) -> usize {
        let i = ((v - self.lo) / self.step).floor();
        (i.max(0.0) as usize).min(self.count - 1)
    }
    pub fn edge(&self, i: usize) -> f64 {
        let v = self.lo + self.step * i as f64;
        let digits = (-(self.step.log10().floor()) + 2.0).clamp(0.0, 15.0) as i32;
        let p = 10f64.powi(digits);
        (v * p).round() / p
    }
}

/// 원본 histogramBins와 같은 규칙
pub fn histogram_bins(min: f64, max: f64) -> Bins {
    if !min.is_finite() || !max.is_finite() {
        return Bins {
            lo: 0.0,
            step: 1.0,
            count: 1,
        };
    }
    if min == max {
        let step = if min == 0.0 { 1.0 } else { 10f64.powf(min.abs().log10().floor()) };
        return Bins {
            lo: min - step / 2.0,
            step,
            count: 1,
        };
    }
    let raw = (max - min) / TARGET_BINS;
    let power = 10f64.powf(raw.log10().floor());
    let mut best: Option<Bins> = None;
    for f in NICE_FACTORS {
        let step = f * power;
        let lo = (min / step).floor() * step;
        let count = (((max - lo) / step - 1e-9).ceil() as usize).max(1);
        let better = match best {
            None => true,
            Some(b) => (count as f64 - TARGET_BINS).abs() <= (b.count as f64 - TARGET_BINS).abs(),
        };
        if better {
            best = Some(Bins { lo, step, count });
        }
    }
    best.unwrap()
}

pub struct HistItem {
    /// [구간][legend] 개수
    pub counts: Vec<Vec<u32>>,
}

pub fn query_histogram(groups: &Groups, x: &[f64], legend: Option<&LegendCodes>) -> (Bins, Vec<HistItem>) {
    let (min, max) = groups
        .rows
        .par_iter()
        .map(|&r| x[r as usize])
        .fold(|| (f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)))
        .reduce(|| (f64::INFINITY, f64::NEG_INFINITY), |(a, b), (c, d)| (a.min(c), b.max(d)));
    let bins = histogram_bins(min, max);
    let lw = legend.map(|l| l.values.len().max(1)).unwrap_or(1);
    let items = par_groups(groups, |_, rows| {
        let count = |rows: &[u32]| {
            let mut c = vec![0u32; bins.count * lw];
            for &r in rows {
                let b = bins.index(x[r as usize]);
                let l = legend.map(|l| l.at(r) as usize).unwrap_or(0);
                c[b * lw + l] += 1;
            }
            c
        };
        let flat = if rows.len() > 200_000 {
            rows.par_chunks(1 << 16).map(count).reduce(
                || vec![0u32; bins.count * lw],
                |mut a, b| {
                    a.iter_mut().zip(b).for_each(|(x, y)| *x += y);
                    a
                },
            )
        } else {
            count(rows)
        };
        HistItem {
            counts: flat.chunks(lw).map(|c| c.to_vec()).collect(),
        }
    });
    (bins, items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice() {
        let b = histogram_bins(0.0, 100.0);
        assert_eq!(b.step, 5.0);
        assert_eq!(b.count, 20);
        assert_eq!(b.index(100.0), 19);
    }
}
