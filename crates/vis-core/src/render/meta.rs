//! 툴팁·확대·legend 조작에 쓰는 metadata. 원본은 차트 종류마다 모양이 달랐는데(ScatterMeta, CategoryMeta …),
//! 여기서는 "가리킬 수 있는 도형 + 미리 만든 툴팁 글자"로 통일했다. 점 차트만 가장 가까운 점 찾기를 쓴다.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectF {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AxisKind {
    Number,
    Time,
    /// 범주 인덱스 (0, 1, 2 …)
    Index,
}

/// 데이터 값 ↔ 픽셀. 확대 영역을 데이터 범위로 바꿀 때 쓴다.
#[derive(Clone, Copy, Debug)]
pub struct AxisMeta {
    pub kind: AxisKind,
    pub domain: (f64, f64),
    /// domain.0, domain.1이 놓인 픽셀
    pub range: (f32, f32),
}

impl AxisMeta {
    pub fn invert(&self, p: f32) -> f64 {
        let t = if self.range.1 != self.range.0 {
            (p - self.range.0) / (self.range.1 - self.range.0)
        } else {
            0.5
        };
        self.domain.0 + (self.domain.1 - self.domain.0) * t as f64
    }
}

#[derive(Clone, Debug)]
pub enum HitShape {
    Rect(RectF),
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
    },
    Polygon(Vec<(f32, f32)>),
    /// 고리 조각 (pie): 중심, 안·바깥 반지름, 시작·끝 각도(라디안, 12시 기준 시계 방향)
    Arc {
        cx: f32,
        cy: f32,
        r0: f32,
        r1: f32,
        a0: f32,
        a1: f32,
    },
}

impl HitShape {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        match self {
            HitShape::Rect(r) => r.contains(x, y),
            HitShape::Circle { cx, cy, r } => (x - cx).powi(2) + (y - cy).powi(2) <= r * r,
            HitShape::Polygon(p) => point_in_polygon(p, x, y),
            HitShape::Arc { cx, cy, r0, r1, a0, a1 } => {
                let (dx, dy) = (x - cx, y - cy);
                let d = (dx * dx + dy * dy).sqrt();
                if d < *r0 || d > *r1 {
                    return false;
                }
                let mut a = dx.atan2(-dy);
                if a < 0.0 {
                    a += std::f32::consts::TAU;
                }
                a >= *a0 && a <= *a1
            }
        }
    }
}

pub fn point_in_polygon(p: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut inside = false;
    let n = p.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = p[i];
        let (xj, yj) = p[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub shape: HitShape,
    pub lines: Vec<String>,
    /// legend 값 번호 (있으면 툴팁에 색 표시)
    pub color: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct LegendHit {
    pub rect: RectF,
    pub value: String,
    pub hidden: bool,
}

/// 점 차트: 픽셀 칸마다 한 점만 남긴 점 목록과 칸 색인
#[derive(Clone, Debug, Default)]
pub struct PointIndex {
    pub px: Vec<f32>,
    pub py: Vec<f32>,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub id: Vec<u32>,
    pub g: Vec<u8>,
    buckets: HashMap<(i32, i32), Vec<u32>>,
}

const BUCKET: f32 = 16.0;

impl PointIndex {
    pub fn push(&mut self, px: f32, py: f32, x: f64, y: f64, id: u32, g: u8) {
        let i = self.px.len() as u32;
        self.px.push(px);
        self.py.push(py);
        self.x.push(x);
        self.y.push(y);
        self.id.push(id);
        self.g.push(g);
        self.buckets
            .entry(((px / BUCKET) as i32, (py / BUCKET) as i32))
            .or_default()
            .push(i);
    }
    pub fn len(&self) -> usize {
        self.px.len()
    }
    pub fn is_empty(&self) -> bool {
        self.px.is_empty()
    }
    /// max_dist 픽셀 안에서 가장 가까운 점
    pub fn nearest(&self, x: f32, y: f32, max_dist: f32) -> Option<usize> {
        let (bx, by) = ((x / BUCKET) as i32, (y / BUCKET) as i32);
        let reach = (max_dist / BUCKET).ceil() as i32;
        let mut best: Option<(usize, f32)> = None;
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                if let Some(list) = self.buckets.get(&(bx + dx, by + dy)) {
                    for &i in list {
                        let i = i as usize;
                        let d = (self.px[i] - x).powi(2) + (self.py[i] - y).powi(2);
                        if d <= max_dist * max_dist && best.is_none_or(|b| d < b.1) {
                            best = Some((i, d));
                        }
                    }
                }
            }
        }
        best.map(|b| b.0)
    }
}

#[derive(Clone, Debug)]
pub struct ItemMeta {
    pub width: f32,
    pub height: f32,
    pub plot: RectF,
    pub x: Option<AxisMeta>,
    pub y: Option<AxisMeta>,
    pub points: Option<PointIndex>,
    pub hits: Vec<Hit>,
    pub legend: Vec<LegendHit>,
    /// 제목 줄 오른쪽 요약 (점 수 등)
    pub summary: String,
    /// legend 값 목록과 색 (점 툴팁의 legend 이름)
    pub legend_values: Vec<(String, u32)>,
}

impl ItemMeta {
    pub fn new(width: f32, height: f32, plot: RectF) -> Self {
        Self {
            width,
            height,
            plot,
            x: None,
            y: None,
            points: None,
            hits: vec![],
            legend: vec![],
            summary: String::new(),
            legend_values: vec![],
        }
    }

    /// 가리킨 도형 (마지막에 그린 것이 위)
    pub fn hit(&self, x: f32, y: f32) -> Option<&Hit> {
        self.hits.iter().rev().find(|h| h.shape.contains(x, y))
    }

    pub fn legend_at(&self, x: f32, y: f32) -> Option<&LegendHit> {
        self.legend.iter().find(|l| l.rect.contains(x, y))
    }
}
