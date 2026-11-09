//! 차트 위 영역(사각형·자유형)과 도형. 원본 server/render/region.ts, shapes.ts.
//! 위치는 데이터 좌표라 확대·이동해도 같은 데이터를 가리킨다. 축이 없는 차트(pie·radar·sankey 등)는
//! 그림 영역 픽셀과 1:1인 고정 좌표(x = 왼쪽에서의 거리, y = 아래에서의 거리)를 데이터 좌표로 쓴다.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Region {
    Rect { x0: f64, y0: f64, x1: f64, y1: f64 },
    Lasso { points: Vec<(f64, f64)> },
}

impl Region {
    /// 점 (x, y)가 영역 안에 있는지. 자유형은 시작점과 끝점을 이어 닫힌 다각형으로 본다
    pub fn contains(&self, x: f64, y: f64) -> bool {
        match self {
            Region::Rect { x0, y0, x1, y1 } => x >= x0.min(*x1) && x <= x0.max(*x1) && y >= y0.min(*y1) && y <= y0.max(*y1),
            Region::Lasso { points } => {
                let mut inside = false;
                let n = points.len();
                if n < 3 {
                    return false;
                }
                let mut j = n - 1;
                for i in 0..n {
                    let (xi, yi) = points[i];
                    let (xj, yj) = points[j];
                    if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                        inside = !inside;
                    }
                    j = i;
                }
                inside
            }
        }
    }

    /// 경계 상자로 먼저 거르는 판정 함수 (점이 많을 때)
    pub fn tester(&self) -> impl Fn(f64, f64) -> bool + '_ {
        let (mut x0, mut x1, mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        match self {
            Region::Rect {
                x0: a,
                y0: b,
                x1: c,
                y1: d,
            } => {
                x0 = a.min(*c);
                x1 = a.max(*c);
                y0 = b.min(*d);
                y1 = b.max(*d);
            }
            Region::Lasso { points } => {
                for &(px, py) in points {
                    x0 = x0.min(px);
                    x1 = x1.max(px);
                    y0 = y0.min(py);
                    y1 = y1.max(py);
                }
            }
        }
        move |x, y| x >= x0 && x <= x1 && y >= y0 && y <= y1 && self.contains(x, y)
    }

    /// 화면 좌표 꼭짓점 (그리기용)
    pub fn project(&self, px: &dyn Fn(f64) -> f32, py: &dyn Fn(f64) -> f32) -> Vec<(f32, f32)> {
        match self {
            Region::Rect { x0, y0, x1, y1 } => vec![(px(*x0), py(*y0)), (px(*x1), py(*y0)), (px(*x1), py(*y1)), (px(*x0), py(*y1))],
            Region::Lasso { points } => points.iter().map(|&(x, y)| (px(x), py(y))).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Shape {
    Rect { x0: f64, y0: f64, x1: f64, y1: f64 },
    Line { x0: f64, y0: f64, x1: f64, y1: f64 },
    Freehand { points: Vec<(f64, f64)> },
    Text { x: f64, y: f64, text: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ShapeTool {
    #[default]
    Rect,
    Line,
    Freehand,
    Text,
}

impl Shape {
    pub fn is_valid(&self) -> bool {
        let fin = |v: &[f64]| v.iter().all(|x| x.is_finite());
        match self {
            Shape::Rect { x0, y0, x1, y1 } | Shape::Line { x0, y0, x1, y1 } => fin(&[*x0, *y0, *x1, *y1]),
            Shape::Freehand { points } => (2..=5000).contains(&points.len()) && points.iter().all(|p| fin(&[p.0, p.1])),
            Shape::Text { x, y, text } => fin(&[*x, *y]) && !text.trim().is_empty() && text.chars().count() <= 200,
        }
    }
}

/// 이름 붙인 영역 (즐겨찾기 · 새 범례)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedRegion {
    pub name: String,
    pub region: Region,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains() {
        let r = Region::Rect {
            x0: 2.0,
            y0: 2.0,
            x1: 0.0,
            y1: 0.0,
        };
        assert!(r.contains(1.0, 1.0) && !r.contains(3.0, 1.0));
        let l = Region::Lasso {
            points: vec![(0.0, 0.0), (4.0, 0.0), (0.0, 4.0)],
        };
        assert!(l.contains(1.0, 1.0) && !l.contains(3.0, 3.0));
        let t = l.tester();
        assert!(t(1.0, 1.0) && !t(5.0, 0.0));
    }
}
