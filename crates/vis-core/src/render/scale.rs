//! d3-scale의 선형 scale·nice·ticks와 시계열 눈금을 옮긴 것.

use crate::timefmt::{DAY_MS, HOUR_MS, KST_OFFSET_MS};

#[derive(Clone, Copy, Debug)]
pub struct Linear {
    pub d0: f64,
    pub d1: f64,
    pub r0: f64,
    pub r1: f64,
    /// 로그 축 (도메인이 양수일 때만)
    pub log: bool,
}

impl Linear {
    pub fn new(domain: (f64, f64), range: (f64, f64)) -> Self {
        Self {
            d0: domain.0,
            d1: domain.1,
            r0: range.0,
            r1: range.1,
            log: false,
        }
    }
    /// 로그 축으로. 도메인에 0 이하가 있으면 그대로 선형
    pub fn with_log(mut self, on: bool) -> Self {
        self.log = on && self.d0 > 0.0 && self.d1 > 0.0;
        self
    }
    /// 같은 도메인·로그 설정에 새 범위
    pub fn with_range(self, range: (f64, f64)) -> Self {
        Self {
            r0: range.0,
            r1: range.1,
            ..self
        }
    }
    #[inline]
    fn fwd(&self, v: f64) -> f64 {
        if self.log { v.max(1e-300).log10() } else { v }
    }
    #[inline]
    pub fn map(&self, v: f64) -> f64 {
        let (a, b) = (self.fwd(self.d0), self.fwd(self.d1));
        let span = b - a;
        let t = if span != 0.0 { (self.fwd(v) - a) / span } else { 0.5 };
        self.r0 * (1.0 - t) + self.r1 * t
    }
    #[inline]
    pub fn invert(&self, p: f64) -> f64 {
        let span = self.r1 - self.r0;
        let t = if span != 0.0 { (p - self.r0) / span } else { 0.5 };
        if self.log {
            let (a, b) = (self.d0.log10(), self.d1.log10());
            return 10f64.powf(a + (b - a) * t);
        }
        self.d0 + (self.d1 - self.d0) * t
    }
    /// d3 linear.nice(count). 로그 축이면 10의 거듭제곱으로
    pub fn nice(mut self, count: usize) -> Self {
        if self.log {
            let (lo, hi) = (self.d0.min(self.d1), self.d0.max(self.d1));
            self.d0 = 10f64.powf(lo.log10().floor());
            self.d1 = 10f64.powf(hi.log10().ceil());
            if self.d0 == self.d1 {
                self.d1 *= 10.0;
            }
            return self;
        }
        let (mut start, mut stop) = (self.d0, self.d1);
        let reverse = stop < start;
        if reverse {
            std::mem::swap(&mut start, &mut stop);
        }
        let mut prestep = None;
        for _ in 0..10 {
            let step = tick_increment(start, stop, count);
            if Some(step) == prestep {
                break;
            }
            if step > 0.0 {
                start = (start / step).floor() * step;
                stop = (stop / step).ceil() * step;
            } else if step < 0.0 {
                start = (start * step).ceil() / step;
                stop = (stop * step).floor() / step;
            } else {
                break;
            }
            prestep = Some(step);
        }
        if reverse {
            std::mem::swap(&mut start, &mut stop);
        }
        self.d0 = start;
        self.d1 = stop;
        self
    }
    pub fn ticks(&self, count: usize) -> Vec<f64> {
        let (lo, hi) = (self.d0.min(self.d1), self.d0.max(self.d1));
        if self.log {
            let (a, b) = (lo.log10().floor() as i32, hi.log10().ceil() as i32);
            let decades = (b - a).max(1);
            let mults: &[f64] = if decades <= 2 { &[1.0, 2.0, 5.0] } else { &[1.0] };
            let stride = (decades as usize).div_ceil(count.max(1)).max(1) as i32;
            let mut out = vec![];
            let mut e = a;
            while e <= b {
                for m in mults {
                    let v = m * 10f64.powi(e);
                    if v >= lo * (1.0 - 1e-9) && v <= hi * (1.0 + 1e-9) {
                        out.push(v);
                    }
                }
                e += stride;
            }
            return out;
        }
        ticks(lo, hi, count)
    }
}

const E10: f64 = 7.0710678118654755; // sqrt(50)
const E5: f64 = 3.1622776601683795; // sqrt(10)
const E2: f64 = std::f64::consts::SQRT_2;

fn tick_spec(start: f64, stop: f64, count: f64) -> (f64, f64, f64) {
    let step = (stop - start) / count.max(0.0);
    let power = step.log10().floor();
    let error = step / 10f64.powf(power);
    let factor = if error >= E10 {
        10.0
    } else if error >= E5 {
        5.0
    } else if error >= E2 {
        2.0
    } else {
        1.0
    };
    let (mut i1, mut i2, inc);
    if power < 0.0 {
        inc = 10f64.powf(-power) / factor;
        i1 = (start * inc).round();
        i2 = (stop * inc).round();
        if i1 / inc < start {
            i1 += 1.0;
        }
        if i2 / inc > stop {
            i2 -= 1.0;
        }
        (i1, i2, -inc)
    } else {
        inc = 10f64.powf(power) * factor;
        i1 = (start / inc).round();
        i2 = (stop / inc).round();
        if i1 * inc < start {
            i1 += 1.0;
        }
        if i2 * inc > stop {
            i2 -= 1.0;
        }
        (i1, i2, inc)
    }
}

pub fn tick_increment(start: f64, stop: f64, count: usize) -> f64 {
    tick_spec(start, stop, count as f64).2
}

/// d3-array ticks
pub fn ticks(start: f64, stop: f64, count: usize) -> Vec<f64> {
    if !(start.is_finite() && stop.is_finite()) || count == 0 {
        return vec![];
    }
    if start == stop {
        return vec![start];
    }
    let (i1, i2, inc) = tick_spec(start, stop, count as f64);
    if i2 < i1 {
        return vec![];
    }
    let n = (i2 - i1 + 1.0) as usize;
    if n > 10_000 {
        return vec![];
    }
    (0..n)
        .map(|i| if inc < 0.0 { (i1 + i as f64) / -inc } else { (i1 + i as f64) * inc })
        .collect()
}

/// 숫자 눈금 글자: 눈금 간격에 맞춘 소수 자리 (d3 tickFormat과 비슷하게)
pub fn number_format(ticks: &[f64]) -> impl Fn(f64) -> String + use<> {
    let step = if ticks.len() >= 2 { (ticks[1] - ticks[0]).abs() } else { 1.0 };
    let digits = if step > 0.0 && step < 1.0 {
        (-(step.log10().floor())) as usize
    } else {
        0
    };
    let digits = digits.min(10);
    move |v: f64| {
        let a = v.abs();
        if a >= 1e6 && digits == 0 {
            let s = format!("{:.1}M", v / 1e6);
            return s.replace(".0M", "M");
        }
        if a >= 1e4 && digits == 0 {
            // 천 단위 구분
            let s = format!("{:.0}", v);
            let (sign, num) = if let Some(n) = s.strip_prefix('-') {
                ("-", n)
            } else {
                ("", s.as_str())
            };
            let mut out = String::new();
            for (i, ch) in num.chars().enumerate() {
                if i > 0 && (num.len() - i) % 3 == 0 {
                    out.push(',');
                }
                out.push(ch);
            }
            return format!("{sign}{out}");
        }
        format!("{v:.digits$}")
    }
}

/// 짧은 수 (막대 위 값 글자)
pub fn short_number(v: f64) -> String {
    let a = v.abs();
    if !v.is_finite() {
        return "-".into();
    }
    if a >= 1e9 {
        format!("{:.1}B", v / 1e9)
    } else if a >= 1e6 {
        format!("{:.1}M", v / 1e6)
    } else if a >= 1e4 {
        format!("{:.1}k", v / 1e3)
    } else if a >= 100.0 || v.fract() == 0.0 {
        format!("{v:.0}")
    } else if a >= 1.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.3}")
    }
}

const TIME_STEPS: [f64; 20] = [
    1.0,
    5.0,
    15.0,
    30.0,
    100.0,
    250.0,
    500.0,
    1000.0,
    5000.0,
    15_000.0,
    30_000.0,
    60_000.0,
    300_000.0,
    900_000.0,
    1_800_000.0,
    HOUR_MS,
    3.0 * HOUR_MS,
    6.0 * HOUR_MS,
    12.0 * HOUR_MS,
    DAY_MS,
];

/// 시계열 눈금: 보기 좋은 간격을 KST 경계에 맞춘다 (d3 scaleUtc.ticks 대신)
pub fn time_ticks(d0: f64, d1: f64, count: usize) -> Vec<f64> {
    let span = d1 - d0;
    if !(span > 0.0) || count == 0 {
        return vec![(d0 + d1) / 2.0];
    }
    let target = span / count as f64;
    let step = TIME_STEPS
        .iter()
        .copied()
        .chain([
            2.0 * DAY_MS,
            7.0 * DAY_MS,
            14.0 * DAY_MS,
            30.0 * DAY_MS,
            90.0 * DAY_MS,
            365.0 * DAY_MS,
        ])
        .find(|&s| s >= target)
        .unwrap_or(365.0 * DAY_MS);
    let first = ((d0 + KST_OFFSET_MS) / step).ceil() * step - KST_OFFSET_MS;
    let mut out = vec![];
    let mut t = first;
    while t <= d1 && out.len() < 1000 {
        out.push(t);
        t += step;
    }
    if out.is_empty() {
        out.push((d0 + d1) / 2.0);
    }
    out
}

/// 값 범위. 비었으면 [0, 1], 값이 하나면 앞뒤로 벌린다
pub fn extent(values: impl Iterator<Item = f64>, time: bool) -> (f64, f64) {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in values {
        if v.is_finite() {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    if !lo.is_finite() {
        return (0.0, 1.0);
    }
    if lo == hi {
        let pad = if time { 60_000.0 } else { 1.0 };
        return (lo - pad, hi + pad);
    }
    (lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn d3_ticks() {
        assert_eq!(ticks(0.0, 1.0, 5), vec![0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);
        assert_eq!(ticks(0.0, 10.0, 5), vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0]);
        let s = Linear::new((0.13, 9.7), (0.0, 1.0)).nice(5);
        assert_eq!((s.d0, s.d1), (0.0, 10.0));
    }
}
