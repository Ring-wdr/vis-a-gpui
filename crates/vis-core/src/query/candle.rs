//! candle: 전체 기간을 깔끔한 시간 간격(150개 이하)으로 나눠 구간마다 OHLC를 낸다.
//! 시가/종가 = 구간 안 첫/마지막 시각의 평균값 (같은 시각에 행이 여럿이어도 치우치지 않게), 고가·저가 = 최대·최소.

use rayon::prelude::*;

use super::{Groups, par_groups};
use crate::timefmt::{DAY_MS, HOUR_MS, KST_OFFSET_MS};

const S: f64 = 1000.0;
const M: f64 = 60_000.0;
pub const INTERVALS: [f64; 27] = [
    S,
    2.0 * S,
    5.0 * S,
    10.0 * S,
    15.0 * S,
    30.0 * S,
    M,
    2.0 * M,
    5.0 * M,
    10.0 * M,
    15.0 * M,
    30.0 * M,
    HOUR_MS,
    2.0 * HOUR_MS,
    3.0 * HOUR_MS,
    6.0 * HOUR_MS,
    12.0 * HOUR_MS,
    DAY_MS,
    2.0 * DAY_MS,
    3.0 * DAY_MS,
    7.0 * DAY_MS,
    14.0 * DAY_MS,
    30.0 * DAY_MS,
    60.0 * DAY_MS,
    90.0 * DAY_MS,
    180.0 * DAY_MS,
    365.0 * DAY_MS,
];
pub const MAX_CANDLES: f64 = 150.0;

pub fn candle_interval(span: f64) -> f64 {
    INTERVALS
        .iter()
        .copied()
        .find(|&i| span / i <= MAX_CANDLES)
        .unwrap_or(365.0 * DAY_MS)
}

pub fn bucket_start(t: f64, interval: f64) -> f64 {
    ((t + KST_OFFSET_MS) / interval).floor() * interval - KST_OFFSET_MS
}

#[derive(Clone, Copy, Debug)]
pub struct Candle {
    pub t: f64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub count: u32,
}

pub struct CandleItem {
    pub candles: Vec<Candle>,
}

#[derive(Clone, Copy)]
struct Bucket {
    first_t: f64,
    first_sum: f64,
    first_n: u32,
    last_t: f64,
    last_sum: f64,
    last_n: u32,
    hi: f64,
    lo: f64,
    n: u32,
}

pub fn query_candle(groups: &Groups, ts: &[f64], v: &[f64]) -> (f64, Vec<CandleItem>) {
    let (min, max) = groups
        .rows
        .par_iter()
        .map(|&r| ts[r as usize])
        .fold(|| (f64::INFINITY, f64::NEG_INFINITY), |(a, b), t| (a.min(t), b.max(t)))
        .reduce(|| (f64::INFINITY, f64::NEG_INFINITY), |(a, b), (c, d)| (a.min(c), b.max(d)));
    if !min.is_finite() {
        return (HOUR_MS, par_groups(groups, |_, _| CandleItem { candles: vec![] }));
    }
    let interval = candle_interval((max - min).max(1.0));
    let first = bucket_start(min, interval);
    let nb = (((bucket_start(max, interval) - first) / interval).round() as usize) + 1;
    let items = par_groups(groups, |_, rows| {
        let mut b: Vec<Option<Bucket>> = vec![None; nb];
        for &r in rows {
            let t = ts[r as usize];
            let y = v[r as usize];
            let i = (((bucket_start(t, interval) - first) / interval).round() as usize).min(nb - 1);
            match &mut b[i] {
                None => {
                    b[i] = Some(Bucket {
                        first_t: t,
                        first_sum: y,
                        first_n: 1,
                        last_t: t,
                        last_sum: y,
                        last_n: 1,
                        hi: y,
                        lo: y,
                        n: 1,
                    })
                }
                Some(k) => {
                    k.n += 1;
                    k.hi = k.hi.max(y);
                    k.lo = k.lo.min(y);
                    if t < k.first_t {
                        (k.first_t, k.first_sum, k.first_n) = (t, y, 1);
                    } else if t == k.first_t {
                        k.first_sum += y;
                        k.first_n += 1;
                    }
                    if t > k.last_t {
                        (k.last_t, k.last_sum, k.last_n) = (t, y, 1);
                    } else if t == k.last_t {
                        k.last_sum += y;
                        k.last_n += 1;
                    }
                }
            }
        }
        CandleItem {
            candles: b
                .iter()
                .enumerate()
                .filter_map(|(i, k)| {
                    k.map(|k| Candle {
                        t: first + i as f64 * interval,
                        open: k.first_sum / k.first_n as f64,
                        close: k.last_sum / k.last_n as f64,
                        high: k.hi,
                        low: k.lo,
                        count: k.n,
                    })
                })
                .collect(),
        }
    });
    (interval, items)
}
