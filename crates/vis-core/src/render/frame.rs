//! 축이 있는 차트가 함께 쓰는 그리기 조각: 그림 영역 배치, 제목 줄, 눈금·격자, 범주 글자, legend.
//! 차트 속성(제목 · 축 · 범례 · 패널 구역)을 따른다 (원본 frame.ts, axis-style.ts, legend.ts, style.ts).

use super::canvas::{Align, Canvas};
use super::meta::{LegendHit, RectF};
use super::text;
use super::theme::{Style, color_or};
use crate::options::{AxisOptions, TextAlign};
use crate::spec::LegendPosition;

pub const FONT: f32 = 11.0;
pub const TITLE_FONT: f32 = 13.0;
const LEGEND_ROW: f32 = 18.0;
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
    fn vertical_width(&self, px: f32) -> f32 {
        let w = self
            .values
            .iter()
            .map(|v| text::measure(v, px))
            .fold(text::measure(&self.title, px), f32::max);
        (w + 22.0).min(140.0 * px / FONT)
    }
}

// ── 글자 크기 ──

pub fn title_px(t: &Style) -> f32 {
    t.font(t.o.title.font_size, TITLE_FONT)
}
fn summary_px(t: &Style) -> f32 {
    t.font(t.o.title.font_size - 1.0, FONT)
}
pub fn legend_px(t: &Style) -> f32 {
    t.font(t.o.legend.font_size, FONT)
}
pub fn tick_px(t: &Style, a: &AxisOptions) -> f32 {
    t.font(a.label.font_size, FONT)
}

/// 제목 줄 높이 (제목·요약을 모두 끄면 0)
pub fn header_h(t: &Style) -> f32 {
    let o = &t.o.title;
    if !o.visible && !o.summary.visible {
        return t.o.panel.margin.top + 6.0;
    }
    let mut h = t.o.panel.margin.top + title_px(t) + 11.0;
    if o.visible && !o.subtitle.is_empty() {
        h += t.font(o.font_size - 2.0, FONT) + 4.0;
    }
    h
}

fn legend_visible<'a>(t: &Style, legend: Option<&'a LegendInput>) -> Option<&'a LegendInput> {
    legend.filter(|_| t.o.legend.visible && !t.compact)
}

/// y 축 왼쪽 여백 (눈금 글자 폭 + 축 제목)
fn y_axis_w(t: &Style, left_label: f32) -> f32 {
    let y = &t.o.axes.y;
    if left_label <= 0.0 || !y.visible {
        return 0.0;
    }
    let mut w = if y.label.visible { left_label + 6.0 } else { 4.0 };
    if y.title.visible {
        w += tick_px(t, y) + 6.0;
    }
    w
}

/// x 축 아래 여백 (눈금 글자 높이 + 기울임 + 축 제목)
fn x_axis_h(t: &Style) -> f32 {
    let x = &t.o.axes.x;
    if !x.visible {
        return PAD;
    }
    let px = tick_px(t, x);
    let mut h = if x.label.visible { px + 15.0 } else { 6.0 };
    if x.label.visible && x.label.angle.abs() >= 1.0 {
        // 기울인 글자는 길이만큼 더 내려간다 (어림 60px)
        h += 60.0f32.min(px * 5.0) * x.label.angle.to_radians().sin().abs();
    }
    if x.title.visible {
        h += px + 6.0;
    }
    h
}

/// 섬네일: 여백만 조금 두고 전부 그림 영역
pub fn compact_plot(width: f32, height: f32) -> RectF {
    let pad = 4.0;
    RectF::new(pad, pad, (width - pad * 2.0).max(10.0), (height - pad * 2.0).max(10.0))
}

/// 그림 영역 배치. y 눈금 글자 폭(left_label)과 legend 위치·x 글자 줄 여부로 여백을 정한다
pub fn frame_plot(t: &Style, width: f32, height: f32, left_label: f32, legend: Option<&LegendInput>, x_labels: bool) -> RectF {
    let m = &t.o.panel.margin;
    let mut top = header_h(t);
    let mut bottom = m.bottom + if x_labels { x_axis_h(t) } else { PAD };
    let mut left = m.left + PAD + y_axis_w(t, left_label);
    let mut right = m.right + PAD + 4.0;
    if let Some(l) = legend_visible(t, legend) {
        let px = legend_px(t);
        match l.position {
            LegendPosition::Top => top += LEGEND_ROW * px / FONT,
            LegendPosition::Bottom => bottom += LEGEND_ROW * px / FONT,
            LegendPosition::Left => left += l.vertical_width(px) + 8.0,
            LegendPosition::Right => right += l.vertical_width(px) + 8.0,
        }
    }
    RectF::new(left, top, (width - left - right).max(10.0), (height - top - bottom).max(10.0))
}

/// 제목(정렬 · 굵게 · 부제목)과 요약(오른쪽). 렌더러가 다 그린 뒤 render()가 한 번 부른다
pub fn draw_title(c: &mut Canvas, t: &Style, title: &str, summary: &str) {
    let o = &t.o.title;
    let m = &t.o.panel.margin;
    let px = title_px(t);
    let base = m.top + px + 4.0;
    let (l, r) = (m.left + PAD, c.width - m.right - PAD);
    let sum_px = summary_px(t);
    let sw = if o.summary.visible && !summary.is_empty() {
        let sc = color_or(&o.summary.color, t.text_muted);
        c.text(summary, r, base, sum_px, sc, Align::Right);
        text::measure(summary, sum_px) + 12.0
    } else {
        0.0
    };
    if !o.visible {
        return;
    }
    let text_ = if o.text.is_empty() { title } else { &o.text };
    let avail = (r - l - sw).max(10.0);
    let s = text::truncate(text_, px, avail);
    let w = text::measure(&s, px);
    let x = match o.align {
        TextAlign::Left => l,
        TextAlign::Center => (l + (r - l) / 2.0 - w / 2.0).max(l),
        TextAlign::Right => (r - sw - w).max(l),
    };
    let col = color_or(&o.color, t.text_primary);
    if o.bold {
        c.text_bold(&s, x, base, px, col, Align::Left);
    } else {
        c.text(&s, x, base, px, col, Align::Left);
    }
    if !o.subtitle.is_empty() {
        let spx = t.font(o.font_size - 2.0, FONT);
        let s = text::truncate(&o.subtitle, spx, avail);
        let sw2 = text::measure(&s, spx);
        let x = match o.align {
            TextAlign::Left => l,
            TextAlign::Center => (l + (r - l) / 2.0 - sw2 / 2.0).max(l),
            TextAlign::Right => (r - sw - sw2).max(l),
        };
        c.text(
            &s,
            x,
            base + spx + 4.0,
            spx,
            color_or(&o.subtitle_color, t.text_secondary),
            Align::Left,
        );
    }
}

fn fmt_with(decimals: Option<u32>, v: f64, fmt: &dyn Fn(f64) -> String) -> String {
    match decimals {
        Some(d) => format!("{:.*}", d as usize, v),
        None => fmt(v),
    }
}

/// 시계열 눈금 형식: 축 › 날짜 형식, 비우면 범위에 맞춰 자동
pub fn date_fmt(t: &Style, span: f64) -> &str {
    let f = &t.o.axes.x.date_format;
    if f.trim().is_empty() {
        crate::timefmt::axis_format(span)
    } else {
        f
    }
}

/// x 눈금 개수 (축 › 눈금 개수를 바꿨으면 그 값, 아니면 폭에 맞춰)
pub fn x_tick_count(t: &Style, plot: RectF) -> usize {
    let c = t.o.axes.x.tick.count as usize;
    if c != 5 {
        c.max(1)
    } else {
        ((plot.w / 90.0).floor() as usize).max(2)
    }
}

pub fn y_tick_count(t: &Style) -> usize {
    (t.o.axes.y.tick.count as usize).max(1)
}

/// 값 축 범위: 축 › 최솟값·최댓값이 있으면 그 값
pub fn axis_domain(a: &AxisOptions, auto: (f64, f64)) -> (f64, f64) {
    let lo = a.min.unwrap_or(auto.0);
    let hi = a.max.unwrap_or(auto.1);
    if hi > lo { (lo, hi) } else { auto }
}

fn axis_title_color(t: &Style, a: &AxisOptions) -> u32 {
    color_or(&a.title.color, color_or(&a.label.color, t.text_secondary))
}

/// y 축 제목 (세로)
fn draw_y_title(c: &mut Canvas, t: &Style, plot: RectF, title: &str) {
    let y = &t.o.axes.y;
    if !y.visible || !y.title.visible {
        return;
    }
    let s = if y.title.text.is_empty() { title } else { &y.title.text };
    let px = tick_px(t, y);
    let x = t.o.panel.margin.left + PAD + px / 2.0;
    let s = text::truncate(s, px, plot.h);
    c.text_vertical(&s, x, plot.y + plot.h / 2.0, px, axis_title_color(t, y));
}

/// x 축 제목 (아래 가운데)
fn draw_x_title(c: &mut Canvas, t: &Style, plot: RectF, title: &str) {
    let x = &t.o.axes.x;
    if !x.visible || !x.title.visible {
        return;
    }
    let s = if x.title.text.is_empty() { title } else { &x.title.text };
    let px = tick_px(t, x);
    let base = c.height - t.o.panel.margin.bottom - PAD + 2.0;
    let s = text::truncate(s, px, plot.w);
    c.text(&s, plot.x + plot.w / 2.0, base, px, axis_title_color(t, x), Align::Center);
}

/// 가로 격자와 왼쪽 눈금 글자 (+ y 축 선·눈금·제목)
pub fn draw_y_axis(c: &mut Canvas, t: &Style, plot: RectF, y: impl Fn(f64) -> f32, ticks: &[f64], fmt: &dyn Fn(f64) -> String) {
    draw_y_axis_titled(c, t, plot, y, ticks, fmt, "");
}

pub fn draw_y_axis_titled(
    c: &mut Canvas,
    t: &Style,
    plot: RectF,
    y: impl Fn(f64) -> f32,
    ticks: &[f64],
    fmt: &dyn Fn(f64) -> String,
    title: &str,
) {
    let a = &t.o.axes.y;
    let px = tick_px(t, a);
    let label_c = color_or(&a.label.color, t.text_secondary);
    let grid_c = color_or(&a.grid.color, t.grid);
    let tick_c = color_or(&a.tick.color, t.axis);
    for &v in ticks {
        let py = y(v);
        if py < plot.y - 0.5 || py > plot.bottom() + 0.5 {
            continue;
        }
        if a.grid.visible && a.grid.width > 0.0 {
            c.line(plot.x, py, plot.right(), py, grid_c, a.grid.width);
        }
        if !a.visible {
            continue;
        }
        if a.tick.visible && a.tick.length > 0.0 {
            c.line(plot.x - a.tick.length, py, plot.x, py, tick_c, 1.0);
        }
        if a.label.visible {
            let s = fmt_with(a.decimals, v, fmt);
            c.text_rotated(
                &s,
                plot.x - 6.0 - a.tick.length.max(0.0),
                py + px * 0.36,
                px,
                label_c,
                Align::Right,
                a.label.angle,
            );
        }
    }
    if a.visible && a.line.visible && a.line.width > 0.0 {
        c.line(plot.x, plot.y, plot.x, plot.bottom(), color_or(&a.line.color, t.axis), a.line.width);
    }
    draw_y_title(c, t, plot, title);
}

/// 아래 눈금 글자 (겹치면 건너뛴다) + x 축 선·격자·눈금
pub fn draw_x_axis(c: &mut Canvas, t: &Style, plot: RectF, x: impl Fn(f64) -> f32, ticks: &[f64], fmt: &dyn Fn(f64) -> String) {
    draw_x_axis_titled(c, t, plot, x, ticks, fmt, "");
}

pub fn draw_x_axis_titled(
    c: &mut Canvas,
    t: &Style,
    plot: RectF,
    x: impl Fn(f64) -> f32,
    ticks: &[f64],
    fmt: &dyn Fn(f64) -> String,
    title: &str,
) {
    let a = &t.o.axes.x;
    let px = tick_px(t, a);
    let label_c = color_or(&a.label.color, t.text_secondary);
    let tick_c = color_or(&a.tick.color, t.axis);
    // 시계열 값(epoch ms)에는 소수 자리를 쓰지 않는다
    let decimals = a.decimals.filter(|_| ticks.iter().all(|v| v.abs() < 1e11));
    let mut last_right = f32::NEG_INFINITY;
    for &v in ticks {
        let ppx = x(v);
        if ppx < plot.x - 0.5 || ppx > plot.right() + 0.5 {
            continue;
        }
        if a.grid.visible && a.grid.width > 0.0 {
            c.line(ppx, plot.y, ppx, plot.bottom(), color_or(&a.grid.color, t.grid), a.grid.width);
        }
        if !a.visible {
            continue;
        }
        if a.tick.visible && a.tick.length > 0.0 {
            c.line(ppx, plot.bottom(), ppx, plot.bottom() + a.tick.length, tick_c, 1.0);
        }
        if !a.label.visible {
            continue;
        }
        let s = fmt_with(decimals, v, fmt);
        let w = text::measure(&s, px);
        let angled = a.label.angle.abs() >= 1.0;
        let footprint = if angled { px * 1.4 } else { w };
        if ppx - footprint / 2.0 < last_right + 6.0 {
            continue;
        }
        let base = plot.bottom() + a.tick.length + px + 5.0;
        if angled {
            let align = if a.label.angle > 0.0 { Align::Left } else { Align::Right };
            c.text_rotated(&s, ppx, base, px, label_c, align, a.label.angle);
        } else {
            c.text(&s, ppx, base, px, label_c, Align::Center);
        }
        last_right = ppx + footprint / 2.0;
    }
    if a.visible && a.line.visible && a.line.width > 0.0 {
        c.line(
            plot.x,
            plot.bottom(),
            plot.right(),
            plot.bottom(),
            color_or(&a.line.color, t.axis),
            a.line.width,
        );
    }
    draw_x_title(c, t, plot, title);
}

/// 범주 축 글자: 인덱스 i의 가운데에 단다. 겹치면 건너뛴다
pub fn draw_category_labels(c: &mut Canvas, t: &Style, plot: RectF, labels: &[String], center: impl Fn(usize) -> f32, band: f32) {
    let a = &t.o.axes.x;
    if a.visible && a.line.visible && a.line.width > 0.0 {
        c.line(
            plot.x,
            plot.bottom(),
            plot.right(),
            plot.bottom(),
            color_or(&a.line.color, t.axis),
            a.line.width,
        );
    }
    if !a.visible || !a.label.visible {
        return;
    }
    let px = tick_px(t, a);
    let label_c = color_or(&a.label.color, t.text_secondary);
    let angled = a.label.angle.abs() >= 1.0;
    let mut last_right = f32::NEG_INFINITY;
    for (i, l) in labels.iter().enumerate() {
        let ppx = center(i);
        if ppx < plot.x - 1.0 || ppx > plot.right() + 1.0 {
            continue;
        }
        if a.grid.visible && a.grid.width > 0.0 {
            c.line(ppx, plot.y, ppx, plot.bottom(), color_or(&a.grid.color, t.grid), a.grid.width);
        }
        let max_w = if angled { 90.0 } else { (band - 4.0).clamp(28.0, 120.0) };
        let s = text::truncate(l, px, max_w);
        let w = text::measure(&s, px);
        let footprint = if angled { px * 1.4 } else { w };
        if ppx - footprint / 2.0 < last_right + 4.0 {
            continue;
        }
        let base = plot.bottom() + a.tick.length + px + 5.0;
        if a.tick.visible && a.tick.length > 0.0 {
            c.line(
                ppx,
                plot.bottom(),
                ppx,
                plot.bottom() + a.tick.length,
                color_or(&a.tick.color, t.axis),
                1.0,
            );
        }
        if angled {
            let align = if a.label.angle > 0.0 { Align::Left } else { Align::Right };
            c.text_rotated(&s, ppx, base, px, label_c, align, a.label.angle);
        } else {
            c.text(&s, ppx, base, px, label_c, Align::Center);
        }
        last_right = ppx + footprint / 2.0;
    }
}

/// 왼쪽 범주 글자 (heatmap·gantt의 행)
pub fn draw_row_labels(c: &mut Canvas, t: &Style, plot: RectF, labels: &[String], center: impl Fn(usize) -> f32, max_w: f32) {
    let a = &t.o.axes.y;
    if !a.visible || !a.label.visible {
        return;
    }
    let px = tick_px(t, a);
    let label_c = color_or(&a.label.color, t.text_secondary);
    let mut last = f32::NEG_INFINITY;
    for (i, l) in labels.iter().enumerate() {
        let py = center(i);
        if py < plot.y - 1.0 || py > plot.bottom() + 1.0 || py - last < px + 1.0 {
            continue;
        }
        let s = text::truncate(l, px, max_w);
        c.text(&s, plot.x - 6.0, py + px * 0.36, px, label_c, Align::Right);
        last = py;
    }
}

/// y 눈금 글자 폭 (소수 자리·글자 크기 반영)
pub fn label_width(t: &Style, labels: impl Iterator<Item = (f64, String)>) -> f32 {
    let a = &t.o.axes.y;
    let px = tick_px(t, a);
    labels
        .map(|(v, l)| text::measure(&fmt_with(a.decimals, v, &|_| l.clone()), px))
        .fold(10.0, f32::max)
}

/// 행 글자 폭 (heatmap·gantt)
pub fn row_label_width(t: &Style, labels: impl Iterator<Item = String>) -> f32 {
    let px = tick_px(t, &t.o.axes.y);
    labels.map(|l| text::measure(&l, px)).fold(10.0, f32::max)
}

/// legend 상자. 위·아래는 가로 한 줄 `제목 ● 값 ● 값 … +N`, 왼쪽·오른쪽은 세로 목록
pub fn draw_legend(c: &mut Canvas, t: &Style, l: &LegendInput, plot: RectF) -> Vec<LegendHit> {
    let mut hits = vec![];
    if !t.o.legend.visible {
        return hits;
    }
    let ls = &t.o.legend;
    let px = legend_px(t);
    let k = px / FONT;
    let r = 4.5 * k;
    let font_c = color_or(&ls.font_color, t.text_secondary);
    let title_c = color_or(&ls.title_color, t.text_muted);
    let order: Vec<usize> = if ls.reverse {
        (0..l.values.len()).rev().collect()
    } else {
        (0..l.values.len()).collect()
    };
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
        let s = text::truncate(&l.values[i], px, max_w);
        let w = text::measure(&s, px);
        c.text(
            &s,
            x + r * 2.0 + 4.0,
            y + px * 0.36,
            px,
            if hidden { t.text_muted } else { font_c },
            Align::Left,
        );
        let total = r * 2.0 + 4.0 + w;
        hits.push(LegendHit {
            rect: RectF::new(x - 2.0, y - 8.0 * k, total + 4.0, 16.0 * k),
            value: l.values[i].clone(),
            hidden,
        });
        total
    };
    let m = &t.o.panel.margin;
    let bounds;
    match l.position {
        LegendPosition::Top | LegendPosition::Bottom => {
            let y = if l.position == LegendPosition::Top {
                header_h(t) + 6.0 * k
            } else {
                c.height - m.bottom - 10.0 * k
            };
            let mut x = plot.x;
            let limit = plot.right();
            if !l.title.is_empty() {
                let s = text::truncate(&l.title, px, 100.0);
                c.text(&s, x, y + px * 0.36, px, title_c, Align::Left);
                x += text::measure(&s, px) + 8.0;
            }
            let x_start = plot.x;
            for (n, &i) in order.iter().enumerate() {
                let w = r * 2.0 + 4.0 + text::measure(&l.values[i], px).min(120.0 * k);
                let rest = order.len() - n;
                if x + w > limit && n > 0 {
                    c.text(&format!("+{rest}"), x, y + px * 0.36, px, t.text_muted, Align::Left);
                    x += 20.0;
                    break;
                }
                x += item(c, x, y, i, 120.0 * k, &mut hits) + 12.0;
            }
            bounds = RectF::new(x_start - 6.0, y - 9.0 * k, (x - x_start).max(0.0), 18.0 * k);
        }
        LegendPosition::Left | LegendPosition::Right => {
            let w = l.vertical_width(px);
            let x = if l.position == LegendPosition::Left {
                m.left + PAD
            } else {
                c.width - m.right - PAD - w
            };
            let mut y = plot.y + 6.0 * k;
            if !l.title.is_empty() {
                let s = text::truncate(&l.title, px, w);
                c.text(&s, x, y + px * 0.36, px, title_c, Align::Left);
                y += 18.0 * k;
            }
            let y_start = plot.y;
            for (n, &i) in order.iter().enumerate() {
                if y > plot.bottom() - 4.0 {
                    c.text(&format!("+{}", order.len() - n), x, y + px * 0.36, px, t.text_muted, Align::Left);
                    y += 18.0 * k;
                    break;
                }
                item(c, x, y, i, w - 16.0, &mut hits);
                y += 18.0 * k;
            }
            bounds = RectF::new(x - 6.0, y_start - 2.0, w + 8.0, y - y_start - 4.0);
        }
    }
    if ls.border && bounds.w > 0.0 && bounds.h > 0.0 {
        c.stroke_rect(bounds.x, bounds.y, bounds.w, bounds.h, color_or(&ls.border_color, t.grid), 1.0);
    }
    hits
}

/// 데이터 없음 글자
pub fn draw_empty(c: &mut Canvas, t: &Style, plot: RectF) {
    c.text(
        "데이터 없음",
        plot.x + plot.w / 2.0,
        plot.y + plot.h / 2.0,
        t.font(12.0, FONT + 1.0),
        t.text_muted,
        Align::Center,
    );
}

/// 패널 배경 (그라데이션이면 위 → 아래로 옅어진다)
pub fn draw_panel(c: &mut Canvas, t: &Style) {
    let bg = t.surface;
    c.fill(bg);
    if t.o.panel.gradient && !t.compact {
        let dark = super::theme::luminance(bg) < 0.5;
        let end = super::theme::mix(bg, if dark { 0xffffff } else { 0x000000 }, 0.08);
        let bands = 48;
        let h = c.height / bands as f32;
        for i in 0..bands {
            let col = super::theme::mix(bg, end, i as f32 / (bands - 1) as f32);
            c.fill_rect(0.0, i as f32 * h, c.width, h + 0.6, col, 1.0);
        }
    }
}

/// 패널 테두리
pub fn draw_panel_border(c: &mut Canvas, t: &Style) {
    if t.o.panel.border && !t.compact {
        c.stroke_rect(
            0.5,
            0.5,
            c.width - 1.0,
            c.height - 1.0,
            color_or(&t.o.panel.border_color, t.axis),
            1.0,
        );
    }
}
