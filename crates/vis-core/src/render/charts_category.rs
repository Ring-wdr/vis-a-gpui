//! bar · boxplot · waterfall · histogram (원본 category.ts, waterfall.ts, histogram.ts)

use super::canvas::{Canvas, rounded_rect};
use super::frame::{self, draw_category_labels, frame_plot};
use super::meta::{AxisKind, AxisMeta, Hit, HitShape, ItemMeta, RectF};
use super::scale::{Linear, number_format, short_number};
use super::theme::{Style, color_or, mix};
use super::{ItemRender, legend_name, series_color, visible};
use crate::options::ShapeStyle;
use crate::query::category::{BoxStats, CategoryItem};
use crate::query::histogram::{Bins, HistItem};
use crate::spec::AggFn;
use crate::spec::SubType;

const GAP: f32 = 2.0;

/// 범주 인덱스 축 (확대하면 view.x가 인덱스 범위다)
struct IndexAxis {
    s: Linear,
}

impl IndexAxis {
    fn new(n: usize, view: Option<(f64, f64)>, plot: RectF) -> Self {
        let d = view.unwrap_or((-0.5, n as f64 - 0.5));
        Self {
            s: Linear::new(d, (plot.x as f64, plot.right() as f64)),
        }
    }
    fn center(&self, i: usize) -> f32 {
        self.s.map(i as f64) as f32
    }
    fn band(&self) -> f32 {
        (self.s.map(1.0) - self.s.map(0.0)).abs() as f32
    }
}

/// 값 축: 축 › 최솟값·최댓값·로그 축·눈금 개수를 따른다
fn value_axis(t: &Style, lo: f64, hi: f64, view: Option<(f64, f64)>, plot_y: (f32, f32)) -> (Linear, Vec<f64>) {
    let a = &t.o.axes.y;
    let count = frame::y_tick_count(t);
    let mut s = match view {
        Some(v) => Linear::new(v, (1.0, 0.0)),
        None => {
            let d = frame::axis_domain(a, (lo, hi));
            let s = Linear::new(d, (1.0, 0.0)).with_log(a.log);
            if a.min.is_some() || a.max.is_some() { s } else { s.nice(count) }
        }
    };
    if view.is_some() {
        s = s.with_log(a.log);
    }
    let ticks = s.ticks(count);
    s = s.with_range((plot_y.0 as f64, plot_y.1 as f64));
    (s, ticks)
}

/// 값 축 kind (로그면 Log)
fn y_kind(s: &Linear) -> AxisKind {
    if s.log { AxisKind::Log } else { AxisKind::Number }
}

fn finish_meta(meta: &mut ItemMeta, x: &Linear, kind: AxisKind, y: &Linear, plot: RectF) {
    meta.x = Some(AxisMeta {
        kind,
        domain: (x.d0, x.d1),
        range: (plot.x, plot.right()),
    });
    meta.y = Some(AxisMeta {
        kind: y_kind(y),
        domain: (y.d0, y.d1),
        range: (plot.bottom(), plot.y),
    });
}

/// 막대 칸: 범주 하나 안에서 legend 값별로 나란히 (사이 2px)
fn slots(input: &ItemRender, width: usize) -> Vec<usize> {
    (0..width).filter(|&g| visible(input, g)).collect()
}

pub fn render_bar(c: &mut Canvas, t: &Style, input: &ItemRender, item: &CategoryItem, agg: AggFn) -> ItemMeta {
    let o = &t.o;
    let width = item.bars.first().map(|b| b.len()).unwrap_or(1);
    let vis = slots(input, width);
    let stack = match input.sub_type {
        Some(SubType::Stack) => Some(false),
        Some(SubType::Stack100) => Some(true),
        _ => None,
    };
    // 값 › 빈 칸 값: 그 조합에 값이 없는 칸
    let cell_value = |cell: &Vec<Option<f64>>, g: usize| cell[g].or(o.value.null_numeric);
    // 쌓기: [범주][legend 값] 구간 (stack100은 범주 합계 대비 %)
    let segments: Option<Vec<Vec<Option<(f64, f64)>>>> = stack.map(|pct| {
        item.bars
            .iter()
            .map(|cell| {
                let total: f64 = vis.iter().filter_map(|&g| cell_value(cell, g)).map(f64::abs).sum();
                let (mut pos, mut neg) = (0.0, 0.0);
                let mut out = vec![None; width];
                for &g in &vis {
                    let Some(mut v) = cell_value(cell, g) else { continue };
                    if pct {
                        v = if total > 0.0 { v / total * 100.0 } else { 0.0 };
                    }
                    if v >= 0.0 {
                        out[g] = Some((pos, pos + v));
                        pos += v;
                    } else {
                        out[g] = Some((neg, neg + v));
                        neg += v;
                    }
                }
                out
            })
            .collect()
    });
    let (mut lo, mut hi) = (0.0f64, f64::NEG_INFINITY);
    match &segments {
        Some(seg) => {
            for cell in seg {
                for (a, b) in cell.iter().flatten() {
                    lo = lo.min(a.min(*b));
                    hi = hi.max(a.max(*b));
                }
            }
        }
        None => {
            for cell in &item.bars {
                for &g in &vis {
                    if let Some(v) = cell_value(cell, g) {
                        lo = lo.min(v);
                        hi = hi.max(v);
                    }
                }
            }
        }
    }
    if !hi.is_finite() {
        hi = 1.0;
    }
    hi = hi.max(0.0);
    if lo == hi {
        hi = lo + 1.0;
    }
    if o.axes.y.log && lo <= 0.0 {
        lo = 1.0f64.min(hi / 10.0);
    }
    let draft = value_axis(t, lo, hi, input.view.map(|v| v.y), (1.0, 0.0));
    let fmt = number_format(&draft.1);
    let left = if input.compact {
        0.0
    } else {
        frame::label_width(t, draft.1.iter().map(|&v| (v, fmt(v))))
    };
    let plot = if input.compact {
        frame::compact_plot(input.width, input.height)
    } else {
        frame_plot(t, input.width, input.height, left, input.legend.as_ref(), true)
    };
    let (ys, ticks) = value_axis(t, lo, hi, input.view.map(|v| v.y), (plot.bottom(), plot.y));
    let xa = IndexAxis::new(item.categories.len(), input.view.map(|v| v.x), plot);
    let mut meta = ItemMeta::new(input.width, input.height, plot);
    let kind = match stack {
        Some(false) => " · 누적",
        Some(true) => " · 100% 누적",
        None => "",
    };
    let summary = format!("{}개 범주 · {}{kind}", item.categories.len(), agg.label());
    if !input.compact {
        frame::draw_y_axis_titled(c, t, plot, |v| ys.map(v) as f32, &ticks, &fmt, &input.y_title);
        draw_category_labels(c, t, plot, &item.categories, |i| xa.center(i), xa.band());
        if let Some(l) = &input.legend {
            meta.legend = frame::draw_legend(c, t, l, plot);
        }
    }
    c.set_clip(Some((plot.x, plot.y, plot.w, plot.h)));
    if item.categories.is_empty() {
        frame::draw_empty(c, t, plot);
    }
    let band = xa.band();
    let inner = (band * 0.8).max(1.0);
    let k = if stack.is_some() { 1.0 } else { vis.len().max(1) as f32 };
    let bw = ((inner - GAP * (k - 1.0)) / k).max(1.0);
    let base_v = if ys.log { ys.d0.min(ys.d1) } else { 0.0 };
    let base = ys.map(base_v) as f32;
    let radius = if input.compact { 0.0 } else { o.shape.corner_radius.min(bw / 2.0) };
    let mut labels = vec![];
    for (i, cell) in item.bars.iter().enumerate() {
        let x0 = xa.center(i) - inner / 2.0;
        if x0 > plot.right() || x0 + inner < plot.x {
            continue;
        }
        for (s, &g) in vis.iter().enumerate() {
            let Some(v) = cell_value(cell, g) else { continue };
            let (x, y_a, y_b) = match &segments {
                Some(seg) => {
                    let Some((a, b)) = seg[i][g] else { continue };
                    (x0, ys.map(a) as f32, ys.map(b) as f32)
                }
                None => (x0 + s as f32 * (bw + GAP), base, ys.map(v) as f32),
            };
            let (top, h) = (y_a.min(y_b), (y_a - y_b).abs());
            let focused = o.focus.enabled && o.focus.hit(i as f64, v);
            let color = if focused {
                color_or(&o.focus.color, 0xe34948)
            } else if o.focus.enabled && o.focus.legend_single_color {
                t.others_color()
            } else {
                series_color(input, g)
            };
            // 쌓은 막대는 맨 위 조각만 둥글게
            let r = if stack.is_some() { 0.0 } else { radius };
            if let Some(p) = rounded_rect(x, top, bw, h.max(0.5), r) {
                fill_shape(c, t, &p, color, input.alpha);
            }
            if o.point_label.visible && !input.compact {
                let lv = match (&segments, stack) {
                    (Some(seg), Some(true)) => seg[i][g].map(|(a, b)| b - a).unwrap_or(v),
                    _ => v,
                };
                labels.push((x + bw / 2.0, if stack.is_some() { top + h / 2.0 } else { top }, lv));
            }
            let mut lines = vec![item.categories[i].clone()];
            if let Some(n) = legend_name(input, g) {
                lines.push(n);
            }
            lines.push(format!("{}: {}", agg.label(), short_number(v)));
            if let (Some(seg), Some(true)) = (&segments, stack) {
                if let Some((a, b)) = seg[i][g] {
                    lines.push(format!("비율 {:.1}%", b - a));
                }
            }
            meta.hits.push(Hit {
                shape: HitShape::Rect(RectF::new(x, top, bw, h.max(2.0))),
                lines,
                color: Some(color),
            });
            // 선택 판정: 막대 끝
            meta.anchors.push((meta.hits.len() - 1, (x + bw / 2.0, y_b)));
        }
    }
    for (x, y, v) in labels {
        let s = super::charts_points::label_text(&o.point_label, v);
        super::charts_points::draw_label(c, t, &s, x, y);
    }
    c.set_clip(None);
    finish_meta(&mut meta, &xa.s, AxisKind::Index, &ys, plot);
    meta.summary = summary;
    meta
}

/// 모양 › 적용(채우기·테두리)과 테두리 굵기·색을 따라 도형을 칠한다
pub(crate) fn fill_shape(c: &mut Canvas, t: &Style, p: &tiny_skia::Path, color: u32, alpha: f32) {
    let o = &t.o.shape;
    if o.style != ShapeStyle::Border {
        c.fill_path(p, color, alpha);
    }
    let bw = if o.border_width > 0.0 {
        o.border_width
    } else if o.style == ShapeStyle::Border {
        1.5
    } else {
        0.0
    };
    if bw > 0.0 && o.style != ShapeStyle::Brush {
        let bc = if o.style == ShapeStyle::Border { color } else { t.border_of(color) };
        c.stroke_path(p, bc, alpha.max(0.6), bw, None);
    }
}

pub fn render_boxplot(c: &mut Canvas, t: &Style, input: &ItemRender, item: &CategoryItem) -> ItemMeta {
    let dots = input.sub_type == Some(SubType::Dot);
    let width = item.boxes.first().map(|b| b.len()).unwrap_or(1);
    let vis = slots(input, width);
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for cell in &item.boxes {
        for &g in &vis {
            if let Some(b) = &cell[g] {
                lo = lo.min(b.lo);
                hi = hi.max(b.hi);
                for &o in &b.outliers {
                    lo = lo.min(o);
                    hi = hi.max(o);
                }
            }
        }
    }
    if !lo.is_finite() {
        (lo, hi) = (0.0, 1.0);
    }
    if lo == hi {
        (lo, hi) = (lo - 1.0, hi + 1.0);
    }
    let draft = value_axis(t, lo, hi, input.view.map(|v| v.y), (1.0, 0.0));
    let fmt = number_format(&draft.1);
    let left = if input.compact {
        0.0
    } else {
        frame::label_width(t, draft.1.iter().map(|&v| (v, fmt(v))))
    };
    let plot = if input.compact {
        frame::compact_plot(input.width, input.height)
    } else {
        frame_plot(t, input.width, input.height, left, input.legend.as_ref(), true)
    };
    let (ys, ticks) = value_axis(t, lo, hi, input.view.map(|v| v.y), (plot.bottom(), plot.y));
    let xa = IndexAxis::new(item.categories.len(), input.view.map(|v| v.x), plot);
    let mut meta = ItemMeta::new(input.width, input.height, plot);
    let n: u32 = item.boxes.iter().flatten().flatten().map(|b| b.n).sum();
    let summary = format!("{} 행", super::charts_points::fmt_int(n as usize));
    if !input.compact {
        frame::draw_y_axis_titled(c, t, plot, |v| ys.map(v) as f32, &ticks, &fmt, &input.y_title);
        draw_category_labels(c, t, plot, &item.categories, |i| xa.center(i), xa.band());
        if let Some(l) = &input.legend {
            meta.legend = frame::draw_legend(c, t, l, plot);
        }
    }
    c.set_clip(Some((plot.x, plot.y, plot.w, plot.h)));
    if item.categories.is_empty() {
        frame::draw_empty(c, t, plot);
    }
    let band = xa.band();
    let inner = (band * 0.7).max(1.0);
    let k = vis.len().max(1) as f32;
    let bw = ((inner - GAP * (k - 1.0)) / k).max(1.0);
    for (i, cell) in item.boxes.iter().enumerate() {
        let x0 = xa.center(i) - inner / 2.0;
        if x0 > plot.right() || x0 + inner < plot.x {
            continue;
        }
        for (s, &g) in vis.iter().enumerate() {
            let Some(b) = &cell[g] else { continue };
            let x = x0 + s as f32 * (bw + GAP);
            let cx = x + bw / 2.0;
            let color = series_color(input, g);
            let dark = mix(color, 0x000000, 0.3);
            let (yq1, yq3, ymed) = (ys.map(b.q1) as f32, ys.map(b.q3) as f32, ys.map(b.median) as f32);
            let (ylo, yhi) = (ys.map(b.lo) as f32, ys.map(b.hi) as f32);
            c.line(cx, yhi, cx, yq3, dark, 1.0);
            c.line(cx, yq1, cx, ylo, dark, 1.0);
            c.line(cx - bw / 4.0, yhi, cx + bw / 4.0, yhi, dark, 1.0);
            c.line(cx - bw / 4.0, ylo, cx + bw / 4.0, ylo, dark, 1.0);
            let o = &t.o.shape;
            if o.style != ShapeStyle::Border {
                c.fill_rect(x, yq3, bw, (yq1 - yq3).max(1.0), color, input.alpha * 0.85);
            }
            if o.style != ShapeStyle::Brush {
                let bc = if o.style == ShapeStyle::Border { color } else { t.border_of(color) };
                c.stroke_rect(
                    x,
                    yq3,
                    bw,
                    (yq1 - yq3).max(1.0),
                    bc,
                    if o.border_width > 0.0 { o.border_width } else { 1.0 },
                );
            }
            // dot: 원값 표본을 상자 너비 안에 흩어 찍는다 (황금비 수열이라 다시 그려도 같다)
            if dots && !b.dots.is_empty() {
                let dc = if o.style == ShapeStyle::Border {
                    color
                } else {
                    mix(color, 0x000000, 0.35)
                };
                let r = if t.o.size.point_auto { 1.6 } else { t.o.size.point_fixed / 2.0 };
                let st = c.stamp(r, dc, None, input.alpha);
                let spread = (bw / 2.0 - r).max(0.0);
                for (k, &v) in b.dots.iter().enumerate() {
                    let jitter = ((k as f32 * 0.618_034) % 1.0) * 2.0 - 1.0;
                    c.put(&st, cx + jitter * spread, ys.map(v) as f32);
                }
            }
            c.line(x, ymed, x + bw, ymed, color_or(&o.median_color, 0x0b0b0b), 2.0);
            meta.anchors.push((meta.hits.len(), (cx, ymed)));
            if !input.compact {
                let st = c.stamp(2.0, color, Some((dark, 0.8)), 0.8);
                for &o in &b.outliers {
                    c.put(&st, cx, ys.map(o) as f32);
                }
            }
            let mut lines = vec![item.categories[i].clone()];
            if let Some(n) = legend_name(input, g) {
                lines.push(n);
            }
            lines.extend(box_lines(b));
            meta.hits.push(Hit {
                shape: HitShape::Rect(RectF::new(x, yhi.min(yq3), bw, (ylo - yhi).abs().max(4.0))),
                lines,
                color: Some(color),
            });
        }
    }
    c.set_clip(None);
    finish_meta(&mut meta, &xa.s, AxisKind::Index, &ys, plot);
    meta.summary = summary;
    meta
}

fn box_lines(b: &BoxStats) -> Vec<String> {
    vec![
        format!("n {}", b.n),
        format!("최대(수염) {}", short_number(b.hi)),
        format!("Q3 {}", short_number(b.q3)),
        format!("중앙값 {}", short_number(b.median)),
        format!("Q1 {}", short_number(b.q1)),
        format!("최소(수염) {}", short_number(b.lo)),
        format!("평균 {} · 이상치 {}", short_number(b.mean), b.outlier_count),
    ]
}

pub fn render_waterfall(c: &mut Canvas, t: &Style, input: &ItemRender, item: &CategoryItem) -> ItemMeta {
    let vals: Vec<f64> = item.bars.iter().map(|b| b[0].unwrap_or(0.0)).collect();
    let mut cum = 0.0;
    let mut steps = vec![];
    let (mut lo, mut hi) = (0.0f64, 0.0f64);
    for &v in &vals {
        let start = cum;
        cum += v;
        steps.push((start, cum));
        lo = lo.min(cum);
        hi = hi.max(cum);
    }
    if lo == hi {
        hi = lo + 1.0;
    }
    let mut cats = item.categories.clone();
    cats.push("합계".into());
    let draft = value_axis(t, lo, hi, input.view.map(|v| v.y), (1.0, 0.0));
    let fmt = number_format(&draft.1);
    let left = if input.compact {
        0.0
    } else {
        frame::label_width(t, draft.1.iter().map(|&v| (v, fmt(v))))
    };
    let plot = if input.compact {
        frame::compact_plot(input.width, input.height)
    } else {
        frame_plot(t, input.width, input.height, left, input.legend.as_ref(), true)
    };
    let (ys, ticks) = value_axis(t, lo, hi, input.view.map(|v| v.y), (plot.bottom(), plot.y));
    let xa = IndexAxis::new(cats.len(), input.view.map(|v| v.x), plot);
    let mut meta = ItemMeta::new(input.width, input.height, plot);
    let summary = format!("누적 {}", short_number(cum));
    if !input.compact {
        frame::draw_y_axis_titled(c, t, plot, |v| ys.map(v) as f32, &ticks, &fmt, &input.y_title);
        draw_category_labels(c, t, plot, &cats, |i| xa.center(i), xa.band());
        if let Some(l) = &input.legend {
            meta.legend = frame::draw_legend(c, t, l, plot);
        }
    }
    c.set_clip(Some((plot.x, plot.y, plot.w, plot.h)));
    let bw = (xa.band() * 0.7).max(1.0);
    let mut bars: Vec<(usize, f64, f64, usize)> = steps
        .iter()
        .enumerate()
        .map(|(i, &(a, b))| (i, a, b, if b >= a { 0 } else { 1 }))
        .collect();
    bars.push((cats.len() - 1, 0.0, cum, 2));
    let mut prev_end: Option<(f32, f32)> = None;
    for &(i, a, b, kind) in &bars {
        let x = xa.center(i) - bw / 2.0;
        let (ya, yb) = (ys.map(a) as f32, ys.map(b) as f32);
        if let Some((px, py)) = prev_end {
            if !input.compact {
                c.stroke_path(
                    &super::canvas::polyline(&[(px, py), (x, py)]).unwrap(),
                    t.text_muted,
                    1.0,
                    t.o.size.line_width,
                    Some(&[3.0, 3.0]),
                );
            }
        }
        prev_end = Some((x + bw, yb));
        if !visible(input, kind) {
            continue;
        }
        let color = series_color(input, kind);
        let top = ya.min(yb);
        let h = (ya - yb).abs().max(1.0);
        if let Some(p) = rounded_rect(
            x,
            top,
            bw,
            h,
            if input.compact {
                0.0
            } else {
                t.o.shape.corner_radius.min(bw / 4.0)
            },
        ) {
            fill_shape(c, t, &p, color, input.alpha);
        }
        if t.o.point_label.visible && !input.compact {
            let s = super::charts_points::label_text(&t.o.point_label, b - a);
            super::charts_points::draw_label(c, t, &s, x + bw / 2.0, top);
        }
        let lines = vec![
            cats[i].clone(),
            format!("값 {}", short_number(b - a)),
            format!("누적 {}", short_number(b)),
        ];
        meta.hits.push(Hit {
            shape: HitShape::Rect(RectF::new(x, top, bw, h.max(3.0))),
            lines,
            color: Some(color),
        });
    }
    c.set_clip(None);
    finish_meta(&mut meta, &xa.s, AxisKind::Index, &ys, plot);
    meta.summary = summary;
    meta
}

pub fn render_histogram(c: &mut Canvas, t: &Style, input: &ItemRender, bins: Bins, item: &HistItem) -> ItemMeta {
    let width = item.counts.first().map(|b| b.len()).unwrap_or(1);
    let vis = slots(input, width);
    let max = item.counts.iter().flat_map(|c| vis.iter().map(|&g| c[g])).max().unwrap_or(0) as f64;
    let draft = value_axis(t, 0.0, max.max(1.0), input.view.map(|v| v.y), (1.0, 0.0));
    let fmt = number_format(&draft.1);
    let left = if input.compact {
        0.0
    } else {
        frame::label_width(t, draft.1.iter().map(|&v| (v, fmt(v))))
    };
    let plot = if input.compact {
        frame::compact_plot(input.width, input.height)
    } else {
        frame_plot(t, input.width, input.height, left, input.legend.as_ref(), true)
    };
    let (ys, ticks) = value_axis(t, 0.0, max.max(1.0), input.view.map(|v| v.y), (plot.bottom(), plot.y));
    let xd = input.view.map(|v| v.x).unwrap_or((bins.lo, bins.hi()));
    let xs = Linear::new(xd, (plot.x as f64, plot.right() as f64));
    let x_ticks = xs.ticks(((plot.w / 90.0) as usize).max(2));
    let xfmt = number_format(&x_ticks);
    let total: u32 = item.counts.iter().flat_map(|c| vis.iter().map(|&g| c[g])).sum();
    let summary = format!("{} 행 · 구간 {}", super::charts_points::fmt_int(total as usize), bins.count);
    let mut meta = ItemMeta::new(input.width, input.height, plot);
    if !input.compact {
        frame::draw_y_axis_titled(c, t, plot, |v| ys.map(v) as f32, &ticks, &fmt, &input.y_title);
        frame::draw_x_axis_titled(c, t, plot, |v| xs.map(v) as f32, &x_ticks, &xfmt, &input.x_title);
        if let Some(l) = &input.legend {
            meta.legend = frame::draw_legend(c, t, l, plot);
        }
    }
    c.set_clip(Some((plot.x, plot.y, plot.w, plot.h)));
    let k = vis.len().max(1) as f32;
    let base = ys.map(0.0) as f32;
    for (b, cell) in item.counts.iter().enumerate() {
        let x0 = xs.map(bins.edge(b)) as f32 + 0.5;
        let x1 = xs.map(bins.edge(b + 1)) as f32 - 0.5;
        if x1 < plot.x || x0 > plot.right() {
            continue;
        }
        let w = ((x1 - x0) - GAP * (k - 1.0)) / k;
        for (s, &g) in vis.iter().enumerate() {
            let v = cell[g];
            if v == 0 {
                continue;
            }
            let x = x0 + s as f32 * (w + GAP);
            let y = ys.map(v as f64) as f32;
            let color = series_color(input, g);
            // 기본 모양(채우기만, 테두리 0)이면 경로 없이 사각형으로 (구간이 수천 개라 빠른 길)
            if t.o.shape.style != ShapeStyle::Border && t.o.shape.border_width <= 0.0 {
                c.fill_rect(x, y, w.max(1.0), base - y, color, input.alpha);
            } else if let Some(p) = rounded_rect(x, y, w.max(1.0), (base - y).max(0.5), 0.0) {
                fill_shape(c, t, &p, color, input.alpha);
            }
            if t.o.point_label.visible && !input.compact {
                let s = super::charts_points::label_text(&t.o.point_label, v as f64);
                super::charts_points::draw_label(c, t, &s, x + w / 2.0, y);
            }
            let mut lines = vec![format!("{} ~ {}", short_number(bins.edge(b)), short_number(bins.edge(b + 1)))];
            if let Some(n) = legend_name(input, g) {
                lines.push(n);
            }
            lines.push(format!("개수 {v}"));
            meta.hits.push(Hit {
                shape: HitShape::Rect(RectF::new(x, y, w.max(2.0), base - y)),
                lines,
                color: Some(color),
            });
        }
    }
    c.set_clip(None);
    finish_meta(&mut meta, &xs, AxisKind::Number, &ys, plot);
    meta.summary = summary;
    meta
}
