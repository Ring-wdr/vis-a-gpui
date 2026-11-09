//! 차트 속성의 분석 기능. 원본 server/render/analytics/*.ts:
//! 식(expression) · 이상치(outlier) · 예측(prediction) · 점수(score) · 분포(distribution, line 하위 차트).

use crate::options::{Combine, OutlierMethod, PredictionMethod, PredictionOptions, ScoreKind};

// ── 통계 ──

/// 평균과 표본 표준편차(n-1)
pub fn mean_sd(v: &[f64]) -> (f64, f64) {
    let n = v.len();
    if n == 0 {
        return (f64::NAN, f64::NAN);
    }
    let mean = v.iter().sum::<f64>() / n as f64;
    let ss: f64 = v.iter().map(|x| (x - mean).powi(2)).sum();
    (mean, if n > 1 { (ss / (n - 1) as f64).sqrt() } else { 0.0 })
}

fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let a = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * a);
    let y = 1.0 - ((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * (-a * a).exp();
    sign * y
}

pub fn normal_cdf(z: f64) -> f64 {
    if z == f64::INFINITY {
        return 1.0;
    }
    if z == f64::NEG_INFINITY {
        return 0.0;
    }
    0.5 * (1.0 + erf(z / std::f64::consts::SQRT_2))
}

/// 정렬된 배열의 분위수 (R type 7)
pub fn quantile_sorted(s: &[f64], p: f64) -> f64 {
    let n = s.len();
    if n == 0 {
        return f64::NAN;
    }
    let h = (n - 1) as f64 * p;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    s[lo] + (h - lo as f64) * (s[hi] - s[lo])
}

/// 표준 정규 분위수 (Acklam)
pub fn normal_quantile(p: f64) -> f64 {
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }
    let a = [
        -39.69683028665376,
        220.9460984245205,
        -275.9285104469687,
        138.357751867269,
        -30.66479806614716,
        2.506628277459239,
    ];
    let b = [
        -54.47609879822406,
        161.5858368580409,
        -155.6989798598866,
        66.80131188771972,
        -13.28068155288572,
    ];
    let c = [
        -0.007784894002430293,
        -0.3223964580411365,
        -2.400758277161838,
        -2.549732539343734,
        4.374664141464968,
        2.938163982698783,
    ];
    let d = [0.007784695709041462, 0.3224671290700398, 2.445134137142996, 3.754408661907416];
    let low = 0.02425;
    if p < low || p > 1.0 - low {
        let q = (-2.0 * (if p < low { p } else { 1.0 - p }).ln()).sqrt();
        let z =
            (((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5]) / ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0);
        return if p < low { z } else { -z };
    }
    let q = p - 0.5;
    let r = q * q;
    (((((a[0] * r + a[1]) * r + a[2]) * r + a[3]) * r + a[4]) * r + a[5]) * q
        / (((((b[0] * r + b[1]) * r + b[2]) * r + b[3]) * r + b[4]) * r + 1.0)
}

// ── 이상치 ──

/// 이상치가 아닌 범위 [lo, hi]
pub fn outlier_bounds(values: &[f64], method: OutlierMethod, k: f64) -> (f64, f64) {
    let mut f: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if f.is_empty() {
        return (f64::NEG_INFINITY, f64::INFINITY);
    }
    match method {
        OutlierMethod::Sigma => {
            let (m, sd) = mean_sd(&f);
            (m - k * sd, m + k * sd)
        }
        OutlierMethod::Iqr => {
            f.sort_by(f64::total_cmp);
            let q1 = quantile_sorted(&f, 0.25);
            let q3 = quantile_sorted(&f, 0.75);
            let iqr = q3 - q1;
            (q1 - k * iqr, q3 + k * iqr)
        }
    }
}

// ── 식 ──

#[derive(Debug, Clone)]
enum Node {
    Num(f64),
    X,
    Y,
    Neg(Box<Node>),
    Bin(char, Box<Node>, Box<Node>),
    Call(&'static str, Vec<Node>),
}

/// 해석한 식. 값이 유한하지 않으면 NaN
#[derive(Debug, Clone)]
pub struct Expr(Node);

const FUNCS: [(&str, usize); 11] = [
    ("abs", 1),
    ("log", 1),
    ("ln", 1),
    ("sqrt", 1),
    ("exp", 1),
    ("round", 1),
    ("floor", 1),
    ("ceil", 1),
    ("min", 2),
    ("max", 2),
    ("pow", 2),
];

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Id(String),
    Op(char),
    End,
}

fn tokenize(src: &str) -> Result<Vec<(Tok, usize)>, String> {
    let cs: Vec<char> = src.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                i += 1;
            }
            if i < cs.len() && (cs[i] == 'e' || cs[i] == 'E') {
                let save = i;
                i += 1;
                if i < cs.len() && (cs[i] == '+' || cs[i] == '-') {
                    i += 1;
                }
                if i < cs.len() && cs[i].is_ascii_digit() {
                    while i < cs.len() && cs[i].is_ascii_digit() {
                        i += 1;
                    }
                } else {
                    i = save;
                }
            }
            let s: String = cs[start..i].iter().collect();
            let v: f64 = s.parse().map_err(|_| format!("숫자가 잘못되었습니다 ({}번째 글자)", start + 1))?;
            out.push((Tok::Num(v), start));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            out.push((Tok::Id(cs[start..i].iter().collect::<String>().to_lowercase()), start));
        } else if "+-*/^(),".contains(c) {
            out.push((Tok::Op(c), i));
            i += 1;
        } else {
            return Err(format!("쓸 수 없는 글자 '{c}' ({}번째 글자)", i + 1));
        }
    }
    out.push((Tok::End, cs.len()));
    Ok(out)
}

struct Parser {
    t: Vec<(Tok, usize)>,
    pos: usize,
}

impl Parser {
    fn is_op(&self, c: char) -> bool {
        self.t[self.pos].0 == Tok::Op(c)
    }
    fn expect(&mut self, c: char) -> Result<(), String> {
        if !self.is_op(c) {
            return Err(format!("'{c}'가 필요합니다 ({}번째 글자)", self.t[self.pos].1 + 1));
        }
        self.pos += 1;
        Ok(())
    }
    fn expr(&mut self) -> Result<Node, String> {
        let mut l = self.term()?;
        while self.is_op('+') || self.is_op('-') {
            let Tok::Op(op) = self.t[self.pos].0 else { unreachable!() };
            self.pos += 1;
            l = Node::Bin(op, Box::new(l), Box::new(self.term()?));
        }
        Ok(l)
    }
    fn term(&mut self) -> Result<Node, String> {
        let mut l = self.unary()?;
        while self.is_op('*') || self.is_op('/') {
            let Tok::Op(op) = self.t[self.pos].0 else { unreachable!() };
            self.pos += 1;
            l = Node::Bin(op, Box::new(l), Box::new(self.unary()?));
        }
        Ok(l)
    }
    fn unary(&mut self) -> Result<Node, String> {
        if self.is_op('-') {
            self.pos += 1;
            return Ok(Node::Neg(Box::new(self.unary()?)));
        }
        if self.is_op('+') {
            self.pos += 1;
            return self.unary();
        }
        let base = self.primary()?;
        if self.is_op('^') {
            self.pos += 1;
            return Ok(Node::Bin('^', Box::new(base), Box::new(self.unary()?)));
        }
        Ok(base)
    }
    fn primary(&mut self) -> Result<Node, String> {
        let (tok, at) = self.t[self.pos].clone();
        match tok {
            Tok::Num(v) => {
                self.pos += 1;
                Ok(Node::Num(v))
            }
            Tok::Op('(') => {
                self.pos += 1;
                let e = self.expr()?;
                self.expect(')')?;
                Ok(e)
            }
            Tok::Id(id) => {
                self.pos += 1;
                match id.as_str() {
                    "x" => return Ok(Node::X),
                    "y" => return Ok(Node::Y),
                    _ => {}
                }
                let Some(&(name, arity)) = FUNCS.iter().find(|f| f.0 == id) else {
                    return Err(format!("모르는 이름 '{id}'"));
                };
                self.expect('(')?;
                let mut args = vec![self.expr()?];
                while self.is_op(',') {
                    self.pos += 1;
                    args.push(self.expr()?);
                }
                self.expect(')')?;
                if args.len() != arity {
                    return Err(format!("{name}은(는) 인자 {arity}개가 필요합니다"));
                }
                Ok(Node::Call(name, args))
            }
            Tok::End => Err("식이 끝나지 않았습니다".into()),
            Tok::Op(c) => Err(format!("'{c}'를 쓸 수 없는 자리입니다 ({}번째 글자)", at + 1)),
        }
    }
}

fn eval(n: &Node, x: f64, y: f64) -> f64 {
    match n {
        Node::Num(v) => *v,
        Node::X => x,
        Node::Y => y,
        Node::Neg(a) => -eval(a, x, y),
        Node::Bin(op, a, b) => {
            let (a, b) = (eval(a, x, y), eval(b, x, y));
            match op {
                '+' => a + b,
                '-' => a - b,
                '*' => a * b,
                '/' => a / b,
                _ => a.powf(b),
            }
        }
        Node::Call(f, args) => {
            let a = eval(&args[0], x, y);
            let b = args.get(1).map(|n| eval(n, x, y)).unwrap_or(0.0);
            match *f {
                "abs" => a.abs(),
                "log" => a.log10(),
                "ln" => a.ln(),
                "sqrt" => a.sqrt(),
                "exp" => a.exp(),
                // JS Math.round: .5는 위로
                "round" => (a + 0.5).floor(),
                "floor" => a.floor(),
                "ceil" => a.ceil(),
                "min" => a.min(b),
                "max" => a.max(b),
                _ => a.powf(b),
            }
        }
    }
}

impl Expr {
    /// 빈 식이면 Ok(None)
    pub fn compile(src: &str) -> Result<Option<Expr>, String> {
        if src.trim().is_empty() {
            return Ok(None);
        }
        let mut p = Parser { t: tokenize(src)?, pos: 0 };
        let n = p.expr()?;
        if p.t[p.pos].0 != Tok::End {
            return Err(format!("식 뒤에 남은 글자가 있습니다 ({}번째 글자)", p.t[p.pos].1 + 1));
        }
        Ok(Some(Expr(n)))
    }
    pub fn eval(&self, x: f64, y: f64) -> f64 {
        let v = eval(&self.0, x, y);
        if v.is_finite() { v } else { f64::NAN }
    }
}

// ── 예측 ──

pub const MAX_FIT_POINTS: usize = 2000;

#[derive(Clone, Debug)]
pub struct Prediction {
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
    pub lower: Option<Vec<f64>>,
    pub upper: Option<Vec<f64>>,
    pub description: String,
}

struct Fit {
    sse: f64,
    count: usize,
    f: Box<dyn Fn(usize) -> f64>,
}

/// 같은 폭 x 구간마다 평균 한 점으로 줄인다. xs는 정렬되어 있어야 한다
pub fn downsample(xs: &[f64], ys: &[f64], max: usize) -> (Vec<f64>, Vec<f64>) {
    let n = xs.len().min(ys.len());
    if n <= max {
        return (xs[..n].to_vec(), ys[..n].to_vec());
    }
    let x0 = xs[0];
    let span = xs[n - 1] - x0;
    if span <= 0.0 {
        let size = n.div_ceil(max);
        let mut ox = vec![];
        let mut oy = vec![];
        let mut i = 0;
        while i < n {
            let end = (i + size).min(n);
            ox.push(x0);
            oy.push(ys[i..end].iter().sum::<f64>() / (end - i) as f64);
            i += size;
        }
        return (ox, oy);
    }
    let mut sx = vec![0.0; max];
    let mut sy = vec![0.0; max];
    let mut cnt = vec![0u32; max];
    for i in 0..n {
        let b = ((((xs[i] - x0) / span) * max as f64).floor() as usize).min(max - 1);
        sx[b] += xs[i];
        sy[b] += ys[i];
        cnt[b] += 1;
    }
    let (mut ox, mut oy) = (vec![], vec![]);
    for b in 0..max {
        if cnt[b] > 0 {
            ox.push(sx[b] / cnt[b] as f64);
            oy.push(sy[b] / cnt[b] as f64);
        }
    }
    (ox, oy)
}

fn median_step(xs: &[f64]) -> f64 {
    let mut d: Vec<f64> = xs.windows(2).map(|w| w[1] - w[0]).filter(|d| *d > 0.0).collect();
    if d.is_empty() {
        return 1.0;
    }
    d.sort_by(f64::total_cmp);
    d[d.len() / 2]
}

fn grid(count: usize) -> Vec<f64> {
    (0..count).map(|i| 0.05 + 0.9 * i as f64 / (count - 1) as f64).collect()
}

fn candidates(fixed: Option<f64>, count: usize) -> Vec<f64> {
    fixed.map(|v| vec![v]).unwrap_or_else(|| grid(count))
}

fn damp_sum(phi: f64, h: usize) -> f64 {
    if phi == 1.0 {
        return h as f64;
    }
    let (mut s, mut p) = (0.0, 1.0);
    for _ in 0..h {
        p *= phi;
        s += p;
    }
    s
}

fn sma(ys: &[f64], w: usize) -> Option<Fit> {
    if w == 0 || ys.len() < w {
        return None;
    }
    let mut sum: f64 = ys[..w].iter().sum();
    let (mut sse, mut count) = (0.0, 0);
    for t in w..ys.len() {
        let e = ys[t] - sum / w as f64;
        sse += e * e;
        count += 1;
        sum += ys[t] - ys[t - w];
    }
    let last = sum / w as f64;
    Some(Fit {
        sse,
        count,
        f: Box::new(move |_| last),
    })
}

fn ses(ys: &[f64], a: f64) -> Option<Fit> {
    if ys.len() < 2 {
        return None;
    }
    let mut level = ys[0];
    let mut sse = 0.0;
    for &y in &ys[1..] {
        let e = y - level;
        sse += e * e;
        level += a * e;
    }
    Some(Fit {
        sse,
        count: ys.len() - 1,
        f: Box::new(move |_| level),
    })
}

fn holt(ys: &[f64], a: f64, b: f64, mul: bool, phi: f64) -> Option<Fit> {
    if ys.len() < 3 {
        return None;
    }
    let mut level = ys[0];
    let mut slope = if mul { ys[1] / ys[0] } else { ys[1] - ys[0] };
    let mut sse = 0.0;
    for &y in &ys[1..] {
        let pred = if mul { level * slope.powf(phi) } else { level + phi * slope };
        let e = y - pred;
        sse += e * e;
        let prev = level;
        level = a * y + (1.0 - a) * pred;
        slope = if mul {
            b * (level / prev) + (1.0 - b) * slope.powf(phi)
        } else {
            b * (level - prev) + (1.0 - b) * phi * slope
        };
    }
    Some(Fit {
        sse,
        count: ys.len() - 1,
        f: Box::new(move |h| {
            if mul {
                level * slope.powf(damp_sum(phi, h))
            } else {
                level + damp_sum(phi, h) * slope
            }
        }),
    })
}

#[allow(clippy::too_many_arguments)]
fn holt_winters(ys: &[f64], a: f64, b: f64, g: f64, m: usize, mul_t: bool, mul_s: bool, phi: f64) -> Option<Fit> {
    let n = ys.len();
    if m < 2 || n < 2 * m {
        return None;
    }
    let mean1: f64 = ys[..m].iter().sum::<f64>() / m as f64;
    let mean2: f64 = ys[m..2 * m].iter().sum::<f64>() / m as f64;
    let mut level = mean1;
    let mut slope = if mul_t {
        (mean2 / mean1).powf(1.0 / m as f64)
    } else {
        (mean2 - mean1) / m as f64
    };
    let mut season: Vec<f64> = (0..m).map(|i| if mul_s { ys[i] / mean1 } else { ys[i] - mean1 }).collect();
    let (mut sse, mut count) = (0.0, 0);
    for t in m..n {
        let s = season[t % m];
        let base = if mul_t { level * slope.powf(phi) } else { level + phi * slope };
        let pred = if mul_s { base * s } else { base + s };
        let e = ys[t] - pred;
        sse += e * e;
        count += 1;
        let prev = level;
        level = a * (if mul_s { ys[t] / s } else { ys[t] - s }) + (1.0 - a) * base;
        slope = if mul_t {
            b * (level / prev) + (1.0 - b) * slope.powf(phi)
        } else {
            b * (level - prev) + (1.0 - b) * phi * slope
        };
        season[t % m] = g * (if mul_s { ys[t] / level } else { ys[t] - level }) + (1.0 - g) * s;
    }
    Some(Fit {
        sse,
        count,
        f: Box::new(move |h| {
            let base = if mul_t {
                level * slope.powf(damp_sum(phi, h))
            } else {
                level + damp_sum(phi, h) * slope
            };
            let s = season[(n + h - 1) % m];
            if mul_s { base * s } else { base + s }
        }),
    })
}

fn best<P: Copy>(params: &[P], run: impl Fn(P) -> Option<Fit>) -> Option<(Fit, P)> {
    let mut found: Option<(Fit, P)> = None;
    for &p in params {
        if let Some(f) = run(p)
            && f.sse.is_finite()
            && found.as_ref().is_none_or(|b| f.sse < b.0.sse)
        {
            found = Some((f, p));
        }
    }
    found
}

fn fmt2(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// 예측. xs는 정렬되어 있어야 한다. 점이 모자라면 None
pub fn predict(raw_x: &[f64], raw_y: &[f64], o: &PredictionOptions) -> Option<Prediction> {
    let (xs, ys) = downsample(raw_x, raw_y, MAX_FIT_POINTS);
    if ys.is_empty() || o.period < 1 {
        return None;
    }
    let positive = ys.iter().all(|v| *v > 0.0);
    let mut notes = vec![];
    let mut kind_of = |k: Combine, what: &str| {
        if k == Combine::Multiplicative && !positive {
            notes.push(format!("{what} 덧셈으로 (0 이하 값)"));
            false
        } else {
            k == Combine::Multiplicative
        }
    };
    let phi = if o.damped { 0.98 } else { 1.0 };
    let (fit, name) = match o.method {
        PredictionMethod::Sma => {
            let w = o.window as usize;
            (sma(&ys, w), format!("이동 평균 (n={w})"))
        }
        PredictionMethod::Ses => {
            let r = best(&candidates(o.alpha, 19), |a| ses(&ys, a));
            let a = r.as_ref().map(|r| r.1).unwrap_or(0.0);
            (r.map(|r| r.0), format!("단순 지수 평활 (α={})", fmt2(a)))
        }
        PredictionMethod::Des => {
            let mul = kind_of(o.trend, "추세");
            let combos: Vec<(f64, f64)> = candidates(o.alpha, 10)
                .into_iter()
                .flat_map(|a| candidates(o.beta, 10).into_iter().map(move |b| (a, b)))
                .collect();
            let r = best(&combos, |(a, b)| holt(&ys, a, b, mul, phi));
            let (a, b) = r.as_ref().map(|r| r.1).unwrap_or((0.0, 0.0));
            (r.map(|r| r.0), format!("이중 지수 평활 (α={}, β={})", fmt2(a), fmt2(b)))
        }
        PredictionMethod::HoltWinters => {
            let m = o.season_length as usize;
            let mul_t = kind_of(o.trend, "추세");
            let mul_s = kind_of(o.seasonal, "계절");
            let mut combos = vec![];
            for a in candidates(o.alpha, 8) {
                for b in candidates(o.beta, 5) {
                    for g in candidates(o.gamma, 8) {
                        combos.push((a, b, g));
                    }
                }
            }
            let r = best(&combos, |(a, b, g)| holt_winters(&ys, a, b, g, m, mul_t, mul_s, phi));
            let (a, b, g) = r.as_ref().map(|r| r.1).unwrap_or((0.0, 0.0, 0.0));
            (
                r.map(|r| r.0),
                format!("Holt-Winters (α={}, β={}, γ={}, m={m})", fmt2(a), fmt2(b), fmt2(g)),
            )
        }
    };
    let fit = fit?;
    let step = median_step(&xs);
    let last_x = *xs.last().unwrap();
    let rmse = if fit.count > 0 { (fit.sse / fit.count as f64).sqrt() } else { 0.0 };
    let mut out = Prediction {
        xs: vec![],
        ys: vec![],
        lower: o.show_interval.then(Vec::new),
        upper: o.show_interval.then(Vec::new),
        description: std::iter::once(name).chain(notes).collect::<Vec<_>>().join(" · "),
    };
    for h in 1..=o.period as usize {
        let y = (fit.f)(h);
        out.xs.push(last_x + step * h as f64);
        out.ys.push(if y.is_finite() { y } else { f64::NAN });
        if let (Some(lo), Some(hi)) = (out.lower.as_mut(), out.upper.as_mut()) {
            let half = 1.96 * rmse * (h as f64).sqrt();
            lo.push(y - half);
            hi.push(y + half);
        }
    }
    Some(out)
}

// ── 점수 ──

pub fn rscore(xs: &[f64], ys: &[f64]) -> Option<f64> {
    let n = xs.len().min(ys.len());
    if n < 2 {
        return None;
    }
    let mx = xs[..n].iter().sum::<f64>() / n as f64;
    let my = ys[..n].iter().sum::<f64>() / n as f64;
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (dx, dy) = (xs[i] - mx, ys[i] - my);
        sxy += dx * dy;
        sxx += dx * dx;
        syy += dy * dy;
    }
    if sxx == 0.0 || syy == 0.0 {
        return None;
    }
    Some((sxy / (sxx * syy).sqrt()).clamp(-1.0, 1.0))
}

pub fn fscore(ys: &[f64], groups: &[u8]) -> Option<f64> {
    let n = ys.len().min(groups.len());
    let mut sums = std::collections::BTreeMap::<u8, (f64, usize)>::new();
    let mut total = 0.0;
    for i in 0..n {
        let e = sums.entry(groups[i]).or_default();
        e.0 += ys[i];
        e.1 += 1;
        total += ys[i];
    }
    let k = sums.len();
    if k < 2 || n <= k {
        return None;
    }
    let grand = total / n as f64;
    let ssb: f64 = sums.values().map(|(s, c)| *c as f64 * (s / *c as f64 - grand).powi(2)).sum();
    let ssw: f64 = (0..n)
        .map(|i| {
            let (s, c) = sums[&groups[i]];
            (ys[i] - s / c as f64).powi(2)
        })
        .sum();
    if ssw == 0.0 {
        return None;
    }
    Some(ssb / (k - 1) as f64 / (ssw / (n - k) as f64))
}

pub fn zscore(ys: &[f64], ucl: Option<f64>, lcl: Option<f64>) -> Option<f64> {
    if ucl.is_none() && lcl.is_none() || ys.is_empty() {
        return None;
    }
    let (mean, sd) = mean_sd(ys);
    let hi = ucl.unwrap_or(f64::INFINITY);
    let lo = lcl.unwrap_or(f64::NEG_INFINITY);
    if sd == 0.0 {
        return Some(if mean >= lo && mean <= hi { 1.0 } else { 0.0 });
    }
    Some((normal_cdf((hi - mean) / sd) - normal_cdf((lo - mean) / sd)).max(0.0))
}

pub fn cpk(ys: &[f64], ucl: Option<f64>, lcl: Option<f64>) -> Option<f64> {
    if ucl.is_none() && lcl.is_none() {
        return None;
    }
    let (mean, sd) = mean_sd(ys);
    if ys.len() < 2 || sd == 0.0 {
        return None;
    }
    let mut sides = vec![];
    if let Some(u) = ucl {
        sides.push((u - mean) / (3.0 * sd));
    }
    if let Some(l) = lcl {
        sides.push((mean - l) / (3.0 * sd));
    }
    sides.into_iter().reduce(f64::min)
}

pub fn score_label(kind: ScoreKind) -> &'static str {
    match kind {
        ScoreKind::Rscore => "r",
        ScoreKind::Fscore => "F",
        ScoreKind::Zscore => "z",
        ScoreKind::Cpk => "Cpk",
    }
}

/// 점수 글자. 값이 없으면 —, 큰 값은 유효 숫자 3자리
pub fn format_score(kind: ScoreKind, v: Option<f64>) -> String {
    let l = score_label(kind);
    match v.filter(|v| v.is_finite()) {
        None => format!("{l} = —"),
        Some(v) => {
            let s = if v.abs() >= 100.0 {
                format!("{v:.0}")
            } else {
                let digits = 3 - 1 - v.abs().log10().floor().max(-10.0) as i32;
                let d = digits.max(0) as usize;
                let s = format!("{v:.d$}");
                if s.contains('.') {
                    s.trim_end_matches('0').trim_end_matches('.').to_string()
                } else {
                    s
                }
            };
            format!("{l} = {s}")
        }
    }
}

pub fn compute_scores(
    kinds: &[ScoreKind],
    xs: &[f64],
    ys: &[f64],
    groups: Option<&[u8]>,
    ucl: Option<f64>,
    lcl: Option<f64>,
) -> Vec<String> {
    kinds
        .iter()
        .map(|&k| {
            let v = match k {
                ScoreKind::Rscore => rscore(xs, ys),
                ScoreKind::Fscore => groups.and_then(|g| fscore(ys, g)),
                ScoreKind::Zscore => zscore(ys, ucl, lcl),
                ScoreKind::Cpk => cpk(ys, ucl, lcl),
            };
            format_score(k, v)
        })
        .collect()
}

// ── 분포 (line 하위 차트) ──

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Distribution {
    Ecdf,
    Ncdf,
    Probability,
}

impl Distribution {
    pub fn y_title(self) -> &'static str {
        match self {
            Distribution::Ecdf => "누적 비율",
            Distribution::Ncdf => "정규 누적 확률",
            Distribution::Probability => "정규 분위수 (z)",
        }
    }
}

/// 점을 분포 좌표로 바꾼다: x = 원래 y 값, y = 누적 비율·정규 누적 확률·정규 분위수. 계열마다 따로 계산한다
pub fn to_distribution(kind: Distribution, ys: &[f64], legend: Option<&[u8]>) -> Vec<(usize, f64, f64)> {
    let mut series: std::collections::BTreeMap<u8, Vec<usize>> = Default::default();
    for i in 0..ys.len() {
        series.entry(legend.map(|l| l[i]).unwrap_or(0)).or_default().push(i);
    }
    let mut out = Vec::with_capacity(ys.len());
    for (_, mut rows) in series {
        rows.sort_by(|&a, &b| ys[a].total_cmp(&ys[b]).then(a.cmp(&b)));
        let n = rows.len();
        let values: Vec<f64> = rows.iter().map(|&i| ys[i]).collect();
        let (mean, sd) = mean_sd(&values);
        for (k, &i) in rows.iter().enumerate() {
            let v = ys[i];
            let y = match kind {
                Distribution::Ecdf => (k + 1) as f64 / n as f64,
                Distribution::Ncdf => {
                    if sd > 0.0 {
                        normal_cdf((v - mean) / sd)
                    } else if v < mean {
                        0.0
                    } else if v > mean {
                        1.0
                    } else {
                        0.5
                    }
                }
                Distribution::Probability => normal_quantile((k as f64 + 0.5) / n as f64),
            };
            out.push((i, v, y));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::ChartOptions;

    #[test]
    fn expressions() {
        let e = Expr::compile("log(y) + x * 2 ^ 2").unwrap().unwrap();
        assert!((e.eval(1.0, 100.0) - 6.0).abs() < 1e-9);
        assert!(Expr::compile("foo(x)").is_err());
        assert!(Expr::compile("").unwrap().is_none());
        let e = Expr::compile("-x^2").unwrap().unwrap();
        assert_eq!(e.eval(3.0, 0.0), -9.0);
        assert!(Expr::compile("max(x, y)").unwrap().unwrap().eval(1.0, 2.0) == 2.0);
    }

    #[test]
    fn scores_and_predict() {
        let xs: Vec<f64> = (0..50).map(|i| i as f64).collect();
        let ys: Vec<f64> = xs.iter().map(|x| 2.0 * x + 1.0).collect();
        assert!((rscore(&xs, &ys).unwrap() - 1.0).abs() < 1e-12);
        assert_eq!(format_score(ScoreKind::Rscore, Some(0.98765)), "r = 0.988");
        let mut o = ChartOptions::default().prediction;
        o.method = PredictionMethod::Des;
        let p = predict(&xs, &ys, &o).unwrap();
        assert_eq!(p.xs.len(), 10);
        assert!((p.ys[0] - 101.0).abs() < 1.0);
        let z = zscore(&ys, Some(1e9), Some(-1e9)).unwrap();
        assert!(z > 0.999);
    }

    #[test]
    fn distribution() {
        let ys = [3.0, 1.0, 2.0];
        let d = to_distribution(Distribution::Ecdf, &ys, None);
        assert_eq!(d.iter().map(|p| p.2).collect::<Vec<_>>(), vec![1.0 / 3.0, 2.0 / 3.0, 1.0]);
        assert!(normal_quantile(0.5).abs() < 1e-9);
    }
}
