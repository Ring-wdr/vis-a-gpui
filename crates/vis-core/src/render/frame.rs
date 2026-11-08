//! 축이 있는 차트가 함께 쓰는 그리기 조각: 그림 영역 배치, 제목 줄, 눈금·격자, 범주 글자, legend.

use super::canvas::{Align, Canvas};
use super::meta::{LegendHit, RectF};
use super::text;
use super::theme::Theme;
use crate::spec::LegendPosition;

pub const FONT: f32 = 11.0;
pub const TITLE_FONT: f32 = 13.0;
const TITLE_H: f32 = 24.0;
const LEGEND_ROW: f32 = 18.0;
const X_LABEL_H: f32 = 20.0;
const PAD: f32 = 10.0;

/// legend 입력 (값 · 색 · 숨김)
#[derive(Clone, Debug)]
pub struct LegendInput {
    pub title: String,
    pub values: Vec<String>,
    pub colors: Vec<u32>,
    pub hidden: Vec<bool>,
    pub position: LegendPosition,
}

impl LegendInput {
    pub fn is_hidden(&self, i: usize) -> bool {
        self.hidden.get(i).copied().unwrap_or(false)
    }
    fn vertical_width(&self) -> f32 {
        let w = self.values.iter().map(|v| text::measure(v, FONT)).fold(0.0f32, f32::max);
        (w + 22.0).min(140.0)
    }
}

/// 섬네일: 여백만 조금 두고 전부 그림 영역
pub fn compact_plot(width: f32, height: f32) -> RectF {
    let pad = 4.0;
    RectF::new(pad, pad, (width - pad * 2.0).max(10.0), (height - pad * 2.0).max(10.0))
}

/// 그림 영역 배치. y 눈금 글자 폭(left_label)과 legend 위치·x 글자 줄 여부로 여백을 정한다.
pub fn frame_plot(width: f32, height: f32, left_label: f32, legend: Option<&LegendInput>, x_labels: bool) -> RectF {
    let mut top = TITLE_H;
    let mut bottom = if x_labels { X_LABEL_H + 6.0 } else { PAD };
    let mut left = PAD + left_label + if left_label > 0.0 { 6.0 } else { 0.0 };
    let mut right = PAD + 4.0;
    if let Some(l) = legend {
        match l.position {
            LegendPosition::Top => top += LEGEND_ROW,
            LegendPosition::Bottom => bottom += LEGEND_ROW,
            LegendPosition::Left => left += l.vertical_width() + 8.0,
            LegendPosition::Right => right += l.vertical_width() + 8.0,
        }
    }
    RectF::new(left, top, (width - left - right).max(10.0), (height - top - bottom).max(10.0))
}

/// 제목(왼쪽)과 요약(오른쪽)
pub fn draw_title(c: &mut Canvas, t: &Theme, title: &str, summary: &str) {
    let base = 17.0;
    let sw = if summary.is_empty() {
        0.0
    } else {
        text::measure(summary, FONT) + 12.0
    };
    c.text(summary, c.width - PAD, base, FONT, t.text_muted, Align::Right);
    let title = text::truncate(title, TITLE_FONT, (c.width - PAD * 2.0 - sw).max(10.0));
    c.text(&title, PAD, base, TITLE_FONT, t.text_primary, Align::Left);
}

/// 가로 격자와 왼쪽 눈금 글자
pub fn draw_y_axis(c: &mut Canvas, t: &Theme, plot: RectF, y: impl Fn(f64) -> f32, ticks: &[f64], fmt: &dyn Fn(f64) -> String) {
    for &v in ticks {
        let py = y(v);
        if py < plot.y - 0.5 || py > plot.bottom() + 0.5 {
            continue;
        }
        c.hline(plot.x, plot.right(), py, t.grid);
        c.text(&fmt(v), plot.x - 6.0, py + 4.0, FONT, t.text_secondary, Align::Right);
    }
}

/// 아래 눈금 글자 (겹치면 건너뛴다)
pub fn draw_x_axis(c: &mut Canvas, t: &Theme, plot: RectF, x: impl Fn(f64) -> f32, ticks: &[f64], fmt: &dyn Fn(f64) -> String) {
    c.hline(plot.x, plot.right(), plot.bottom(), t.axis);
    let mut last_right = f32::NEG_INFINITY;
    for &v in ticks {
        let px = x(v);
        if px < plot.x - 0.5 || px > plot.right() + 0.5 {
            continue;
        }
        let s = fmt(v);
        let w = text::measure(&s, FONT);
        if px - w / 2.0 < last_right + 6.0 {
            continue;
        }
        c.vline(px, plot.bottom(), plot.bottom() + 4.0, t.axis);
        c.text(&s, px, plot.bottom() + 16.0, FONT, t.text_secondary, Align::Center);
        last_right = px + w / 2.0;
    }
}

/// 범주 축 글자: 인덱스 i의 가운데에 단다. 겹치면 건너뛴다
pub fn draw_category_labels(c: &mut Canvas, t: &Theme, plot: RectF, labels: &[String], center: impl Fn(usize) -> f32, band: f32) {
    c.hline(plot.x, plot.right(), plot.bottom(), t.axis);
    let mut last_right = f32::NEG_INFINITY;
    for (i, l) in labels.iter().enumerate() {
        let px = center(i);
        if px < plot.x - 1.0 || px > plot.right() + 1.0 {
            continue;
        }
        let s = text::truncate(l, FONT, (band - 4.0).max(28.0).min(120.0));
        let w = text::measure(&s, FONT);
        if px - w / 2.0 < last_right + 4.0 {
            continue;
        }
        c.text(&s, px, plot.bottom() + 16.0, FONT, t.text_secondary, Align::Center);
        last_right = px + w / 2.0;
    }
}

/// 왼쪽 범주 글자 (heatmap·gantt의 행)
pub fn draw_row_labels(c: &mut Canvas, t: &Theme, plot: RectF, labels: &[String], center: impl Fn(usize) -> f32, max_w: f32) {
    let mut last = f32::NEG_INFINITY;
    for (i, l) in labels.iter().enumerate() {
        let py = center(i);
        if py < plot.y - 1.0 || py > plot.bottom() + 1.0 || py - last < 12.0 {
            continue;
        }
        let s = text::truncate(l, FONT, max_w);
        c.text(&s, plot.x - 6.0, py + 4.0, FONT, t.text_secondary, Align::Right);
        last = py;
    }
}

pub fn label_width(labels: impl Iterator<Item = String>) -> f32 {
    labels.map(|l| text::measure(&l, FONT)).fold(10.0, f32::max)
}

/// legend 상자. 위·아래는 가로 한 줄 `● 값 ● 값 … +N`, 왼쪽·오른쪽은 세로 목록
pub fn draw_legend(c: &mut Canvas, t: &Theme, l: &LegendInput, plot: RectF) -> Vec<LegendHit> {
    let mut hits = vec![];
    let r = 4.5;
    let item = |c: &mut Canvas, x: f32, y: f32, i: usize, max_w: f32, hits: &mut Vec<LegendHit>| -> f32 {
        let hidden = l.is_hidden(i);
        let color = l.colors[i];
        if hidden {
            let mut pb = tiny_skia::PathBuilder::new();
            pb.push_circle(x + r, y, r - 0.5);
            if let Some(p) = pb.finish() {
                c.stroke_path(&p, color, 1.0, 1.2, None);
            }
        } else {
            c.circle(x + r, y, r, color, 1.0);
        }
        let s = text::truncate(&l.values[i], FONT, max_w);
        let w = text::measure(&s, FONT);
        c.text(
            &s,
            x + r * 2.0 + 4.0,
            y + 4.0,
            FONT,
            if hidden { t.text_muted } else { t.text_secondary },
            Align::Left,
        );
        let total = r * 2.0 + 4.0 + w;
        hits.push(LegendHit {
            rect: RectF::new(x - 2.0, y - 8.0, total + 4.0, 16.0),
            value: l.values[i].clone(),
            hidden,
        });
        total
    };
    match l.position {
        LegendPosition::Top | LegendPosition::Bottom => {
            let y = if l.position == LegendPosition::Top {
                TITLE_H + 6.0
            } else {
                c.height - 10.0
            };
            let mut x = plot.x;
            let limit = plot.right();
            for i in 0..l.values.len() {
                let w = r * 2.0 + 4.0 + text::measure(&l.values[i], FONT).min(120.0);
                let rest = l.values.len() - i;
                if x + w > limit && i > 0 {
                    c.text(&format!("+{rest}"), x, y + 4.0, FONT, t.text_muted, Align::Left);
                    break;
                }
                x += item(c, x, y, i, 120.0, &mut hits) + 12.0;
            }
        }
        LegendPosition::Left | LegendPosition::Right => {
            let w = l.vertical_width();
            let x = if l.position == LegendPosition::Left {
                PAD
            } else {
                c.width - PAD - w
            };
            let mut y = plot.y + 6.0;
            for i in 0..l.values.len() {
                if y > plot.bottom() - 4.0 {
                    c.text(&format!("+{}", l.values.len() - i), x, y + 4.0, FONT, t.text_muted, Align::Left);
                    break;
                }
                item(c, x, y, i, w - 16.0, &mut hits);
                y += 18.0;
            }
        }
    }
    hits
}

/// 데이터 없음 글자
pub fn draw_empty(c: &mut Canvas, t: &Theme, plot: RectF) {
    c.text(
        "데이터 없음",
        plot.x + plot.w / 2.0,
        plot.y + plot.h / 2.0,
        FONT + 1.0,
        t.text_muted,
        Align::Center,
    );
}
