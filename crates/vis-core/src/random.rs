//! 시드 기반 난수. vis-a-lot `server/data/random.ts`(mulberry32 + Box–Muller)를 그대로 옮겼다.
//! 같은 시드면 브라우저 버전과 같은 수열을 만든다.

pub struct Random {
    state: u32,
    spare: Option<f64>,
}

impl Random {
    pub fn new(seed: u32) -> Self {
        Self { state: seed, spare: None }
    }

    /// [0, 1) 균등분포 (mulberry32)
    #[inline]
    pub fn next(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x6d2b79f5);
        let mut t = self.state;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f64) / 4294967296.0
    }

    /// [min, max)
    #[inline]
    pub fn range(&mut self, min: f64, max: f64) -> f64 {
        min + self.next() * (max - min)
    }

    /// [min, max] 정수
    #[inline]
    pub fn int(&mut self, min: i64, max: i64) -> i64 {
        (min as f64 + self.next() * ((max - min + 1) as f64)).floor() as i64
    }

    /// 표준정규분포 (Box–Muller, 남는 값을 다음에 쓴다)
    #[inline]
    pub fn normal(&mut self) -> f64 {
        if let Some(v) = self.spare.take() {
            return v;
        }
        let mut u = 0.0;
        while u == 0.0 {
            u = self.next();
        }
        let v = self.next();
        let r = (-2.0 * u.ln()).sqrt();
        self.spare = Some(r * (2.0 * std::f64::consts::PI * v).sin());
        r * (2.0 * std::f64::consts::PI * v).cos()
    }
}

/// 문자열에서 하위 시드를 만든다 (FNV-1a 변형). JS의 charCodeAt처럼 UTF-16 코드 단위로 훑는다.
pub fn derive_seed(seed: u32, key: &str) -> u32 {
    let mut h = seed ^ 0x811c9dc5;
    for unit in key.encode_utf16() {
        h = (h ^ unit as u32).wrapping_mul(0x01000193);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let mut a = Random::new(42);
        let mut b = Random::new(42);
        for _ in 0..100 {
            assert_eq!(a.next(), b.next());
        }
        let x = Random::new(1).next();
        assert!((0.0..1.0).contains(&x));
    }
}
