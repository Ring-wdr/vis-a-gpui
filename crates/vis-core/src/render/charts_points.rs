//! scatter · line (원본 scatter.ts, line.ts).
//! - scatter: 점마다 반지름 3 원 + 진한 1px 테두리. 같은 색 점은 픽셀 칸마다 3개까지만 겹쳐 그린다.
//!   점이 그림 영역을 30겹 넘게 덮으면 반지름을 줄인다(최소 1.5).
//! - line: legend 계열마다 x 순으로 정렬한 점을 픽셀 열마다 첫·최소·최대·끝 넷만 남겨(M4) 잇는다.
//! - metadata: 같은 픽셀 칸에 겹친 점은 첫 점만 남긴 점 목록 (툴팁의 가장 가까운 점)

use rayon::prelude::*;

use super::canvas::{Align, Canvas};
use super::frame::{self, frame_plot};
use super::meta::{AxisKind, AxisMeta, ItemMeta, PointIndex, RectF};
use super::scale::{Linear, extent, number_format, short_number, time_ticks};
use super::theme::{Style, color_or, mix};
use super::{ItemRender, Progress, series_color, visible};
use crate::analytics;
use crate::options::{LabelPosition, ShapeStyle};
use crate::query::points::PointsData;
use crate::timefmt::format_kst;

const POINT_RADIUS: f32 = 3.0;
const MAX_COVERAGE: f32 = 30.0;
const MAX_PER_CELL: u8 = 3;
const MIN_POINT_RADIUS: f32 = 1.5;
const LINE_DOTS: usize = 400;

pub fn render_points(
    c: &mut Canvas,
    t: &Style,
    input: &ItemRender,
    line: bool,
    x_time: bool,
    d: &PointsData,
    progress: Progress,
) -> ItemMeta {
    let o = &t.o;
    let n = d.xs.len();
    let zoomed = input.view.is_some();
    let group = |i: usize| d.legend.as_ref().map(|l| l[i]).unwrap_or(0);
    // 예측 (점 차트, 계열마다 x 순)
    let groups = input.legend.as_ref().map(|l| l.values.len()).unwrap_or(1).max(1);
    let mut order: Vec<Vec<u32>> = vec![vec![]; groups];
    for i in 0..n {
        let g = group(i) as usize;
        if visible(input, g) {
            order[g.min(groups - 1)].push(i as u32);
        }
    }
    order
        .par_iter_mut()
        .for_each(|o| o.sort_by(|&a, &b| d.xs[a as usize].total_cmp(&d.xs[b as usize])));
    let predictions: Vec<(usize, analytics::Prediction)> = if o.prediction.enabled && !input.compact {
        order
            .iter()
            .enumerate()
            .filter(|(_, o)| !o.is_empty())
            .filter_map(|(g, ord)| {
                let xs: Vec<f64> = ord.iter().map(|&i| d.xs[i as usize]).collect();
                let ys: Vec<f64> = ord.iter().map(|&i| d.ys[i as usize]).collect();
                analytics::predict(&xs, &ys, &o.prediction).map(|p| (g, p))
            })
            .collect()
    } else {
        vec![]
    };
    let (auto_x, auto_y) = {
        let mut ex = extent(d.xs.iter().copied(), x_time);
        let mut ey = extent(d.ys.iter().copied(), false);
        for (_, p) in &predictions {
            ex.1 = ex.1.max(p.xs.iter().copied().fold(f64::NEG_INFINITY, f64::max));
            for v in p.ys.iter().chain(p.lower.iter().flatten()).chain(p.upper.iter().flatten()) {
                if v.is_finite() {
                    ey = (ey.0.min(*v), ey.1.max(*v));
                }
            }
        }
        (ex, ey)
    };
    let (xd, yd) = match input.view {
        Some(v) => (v.x, v.y),
        None => (frame::axis_domain(&o.axes.x, auto_x), frame::axis_domain(&o.axes.y, auto_y)),
    };
    let ylog = o.axes.y.log;
    let xlog = o.axes.x.log && !x_time;
    let mut ys = Linear::new(yd, (1.0, 0.0)).with_log(ylog);
    if !zoomed && o.axes.y.min.is_none() && o.axes.y.max.is_none() {
        ys = ys.nice(frame::y_tick_count(t));
    }
    let y_ticks = ys.ticks(frame::y_tick_count(t));
    let y_fmt = number_format(&y_ticks);
    let left = if input.compact {
        0.0
    } else {
        frame::label_width(t, y_ticks.iter().map(|&v| (v, y_fmt(v))))
    };
    let plot = if input.compact {
        frame::compact_plot(input.width, input.height)
    } else {
        frame_plot(t, input.width, input.height, left, input.legend.as_ref(), true)
    };
    let ys = ys.with_range(((plot.y + plot.h) as f64, plot.y as f64));
    let mut xs = Linear::new(xd, (plot.x as f64, (plot.x + plot.w) as f64)).with_log(xlog);
    if !zoomed && !x_time && o.axes.x.min.is_none() && o.axes.x.max.is_none() {
        xs = xs.nice(5);
    }
    let x_count = frame::x_tick_count(t, plot);
    let x_ticks = if x_time {
        time_ticks(xs.d0, xs.d1, x_count)
    } else {
        xs.ticks(x_count)
    };
    let num_fmt = number_format(&x_ticks);
    let span = xs.d1 - xs.d0;
    let date = frame::date_fmt(t, span).to_string();
    let x_fmt = move |v: f64| if x_time { format_kst(v, &date) } else { num_fmt(v) };

    // ── 화면 좌표 (병렬) ──
    let sx: Vec<f32> = d.xs.par_iter().map(|&v| xs.map(v) as f32).collect();
    let sy: Vec<f32> = d.ys.par_iter().map(|&v| ys.map(v) as f32).collect();
    progress(0.3);

    // ── 픽셀 칸마다 첫 점만 남긴 metadata ──
    let pw = plot.w.ceil().max(1.0) as usize;
    let ph = plot.h.ceil().max(1.0) as usize;
    let mut cell = vec![false; pw * ph];
    let mut points = PointIndex::default();
    let mut kept = 0usize;
    for i in 0..n {
        let g = group(i);
        if !visible(input, g as usize) {
            continue;
        }
        let ix = (sx[i] - plot.x).round();
        let iy = (sy[i] - plot.y).round();
        if ix >= 0.0 && iy >= 0.0 && (ix as usize) < pw && (iy as usize) < ph {
            let k = iy as usize * pw + ix as usize;
            if !cell[k] {
                cell[k] = true;
                kept += 1;
                if !input.compact {
                    points.push(sx[i].round(), sy[i].round(), d.xs[i], d.ys[i], d.ids[i], g);
                }
            }
        }
    }
    progress(0.5);

    let shown = (0..n).filter(|&i| visible(input, group(i) as usize)).count();
    let summary = format!("{} pts", fmt_int(shown));
    let mut meta = ItemMeta::new(input.width, input.height, plot);
    if let Some(sel) = &input.selection {
        let test = sel.tester();
        meta.selected = Some(
            (0..n)
                .filter(|&i| visible(input, group(i) as usize) && test(d.xs[i], d.ys[i]))
                .count(),
        );
    }
    if !input.compact {
        frame::draw_y_axis_titled(c, t, plot, |v| ys.map(v) as f32, &y_ticks, &y_fmt, &input.y_title);
        frame::draw_x_axis_titled(c, t, plot, |v| xs.map(v) as f32, &x_ticks, &x_fmt, &input.x_title);
        if let Some(l) = &input.legend {
            meta.legend = frame::draw_legend(c, t, l, plot);
            meta.legend_values = l.values.iter().cloned().zip(l.colors.iter().copied()).collect();
        }
    }
    c.set_clip(Some((plot.x, plot.y, plot.w, plot.h)));
    let focus = &o.focus;
    let focus_c = color_or(&focus.color, 0xe34948);
    // 강조 › 나머지는 한 가지 색
    let color_of = |g: usize| {
        if focus.enabled && focus.legend_single_color {
            t.others_color()
        } else {
            series_color(input, g)
        }
    };
    let fill_style = o.shape.style;
    let border_w = if o.shape.border_width > 0.0 { o.shape.border_width } else { 1.0 };
    let mk_stamp = |c: &Canvas, radius: f32, color: u32, compact: bool| {
        let fill = (fill_style != ShapeStyle::Border).then_some(color);
        let border = match fill_style {
            ShapeStyle::Brush => None,
            ShapeStyle::Border => Some((color, border_w)),
            ShapeStyle::Both if compact => None,
            ShapeStyle::Both => Some((t.border_of(color), border_w)),
        };
        c.stamp_shape(radius, o.shape.kind, fill, border, input.alpha)
    };
    if n == 0 {
        frame::draw_empty(c, t, plot);
    } else if line {
        let lw = if input.compact {
            o.size.line_width * 2.0 / 3.0
        } else {
            o.size.line_width
        };
        for (g, ord) in order.iter().enumerate() {
            if ord.is_empty() {
                continue;
            }
            let color = color_of(g);
            let pts = m4(ord, &sx, &sy, plot.x);
            c.polyline_aa(&pts, color, input.alpha, lw);
            if ord.len() <= LINE_DOTS && !input.compact {
                let r = if o.size.point_auto { 2.5 } else { o.size.point_fixed };
                let st = mk_stamp(c, r, color, false);
                for &i in ord {
                    c.put(&st, sx[i as usize], sy[i as usize]);
                }
            }
        }
    } else {
        let area = (plot.w * plot.h).max(1.0);
        let drawable = n.min(kept * MAX_PER_CELL as usize) as f32;
        let coverage = |r: f32| drawable * std::f32::consts::PI * r * r / area;
        let radius = if input.compact {
            1.5
        } else if o.size.point_auto {
            (POINT_RADIUS * (MAX_COVERAGE / coverage(POINT_RADIUS)).sqrt().min(1.0))
                .max(MIN_POINT_RADIUS)
                .clamp(o.size.point_min.min(o.size.point_max), o.size.point_max.max(o.size.point_min))
        } else {
            o.size.point_fixed
        };
        let reach = radius + 1.0;
        let mut drawn = vec![0u8; pw * ph];
        for g in 0..groups {
            if !visible(input, g) {
                continue;
            }
            let color = color_of(g);
            let stamp = mk_stamp(c, radius, color, input.compact);
            drawn.fill(0);
            for i in 0..n {
                if d.legend.is_some() && group(i) as usize != g {
                    continue;
                }
                let ix = (sx[i] - plot.x).round();
                let iy = (sy[i] - plot.y).round();
                if ix >= 0.0 && iy >= 0.0 && (ix as usize) < pw && (iy as usize) < ph {
                    let k = iy as usize * pw + ix as usize;
                    if drawn[k] == MAX_PER_CELL {
                        continue;
                    }
                    drawn[k] += 1;
                } else if ix < -reach || iy < -reach || ix >= pw as f32 + reach || iy >= ph as f32 + reach {
                    continue;
                }
                c.put(&stamp, sx[i], sy[i]);
            }
            progress(0.5 + 0.4 * (g + 1) as f32 / groups as f32);
        }
    }
    // 강조: 조건에 맞는 점을 강조 색·모양·크기로 위에 다시 찍는다
    if focus.enabled && n > 0 {
        let st = c.stamp_shape(
            if input.compact { 2.0 } else { focus.size / 2.0 },
            focus.shape,
            Some(focus_c),
            Some((mix(focus_c, 0x000000, 0.3), 1.0)),
            1.0,
        );
        for i in 0..n {
            if visible(input, group(i) as usize) && focus.hit(d.xs[i], d.ys[i]) {
                c.put(&st, sx[i], sy[i]);
            }
        }
    }
    // 예측 선 (점선)과 신뢰 구간 띠
    for (g, p) in &predictions {
        let color = series_color(input, *g);
        if let (Some(lo), Some(hi)) = (&p.lower, &p.upper) {
            let mut band: Vec<(f32, f32)> = p.xs.iter().zip(hi).map(|(&x, &y)| (xs.map(x) as f32, ys.map(y) as f32)).collect();
            band.extend(p.xs.iter().zip(lo).rev().map(|(&x, &y)| (xs.map(x) as f32, ys.map(y) as f32)));
            if let Some(path) = super::canvas::polygon(&band) {
                c.fill_path(&path, color, 0.15);
            }
        }
        let last = order[*g].last().map(|&i| (sx[i as usize], sy[i as usize]));
        let pts: Vec<(f32, f32)> = last
            .into_iter()
            .chain(
                p.xs.iter()
                    .zip(&p.ys)
                    .filter(|(_, y)| y.is_finite())
                    .map(|(&x, &y)| (xs.map(x) as f32, ys.map(y) as f32)),
            )
            .collect();
        if let Some(path) = super::canvas::polyline(&pts) {
            c.stroke_path(&path, color, 1.0, o.size.line_width.max(1.0), Some(&[5.0, 4.0]));
        }
    }
    // 점 라벨: y 값 (보이는 점이 많으면 픽셀 칸마다 하나만)
    if o.point_label.visible && !input.compact {
        draw_point_labels(c, t, &points, plot);
    }
    c.set_clip(None);
    if !input.compact {
        // 예측 설명 (그림 영역 왼쪽 아래)
        if o.prediction.description {
            let mut y = plot.bottom() - 6.0;
            for (_, p) in predictions.iter().rev() {
                c.text(&p.description, plot.x + 6.0, y, t.font(10.0, 10.0), t.text_muted, Align::Left);
                y -= 14.0;
            }
        }
        // 점수 상자 (그림 영역 오른쪽 위)
        if o.score.visible && !o.score.kinds.is_empty() {
            let vis: Vec<usize> = (0..n).filter(|&i| visible(input, group(i) as usize)).collect();
            let xv: Vec<f64> = vis.iter().map(|&i| d.xs[i]).collect();
            let yv: Vec<f64> = vis.iter().map(|&i| d.ys[i]).collect();
            let gv: Option<Vec<u8>> = d.legend.as_ref().map(|l| vis.iter().map(|&i| l[i]).collect());
            let lines = analytics::compute_scores(&o.score.kinds, &xv, &yv, gv.as_deref(), o.score.ucl, o.score.lcl);
            draw_score_box(c, t, plot, &lines);
        }
    }
    meta.x = Some(AxisMeta {
        kind: if x_time {
            AxisKind::Time
        } else if xs.log {
            AxisKind::Log
        } else {
            AxisKind::Number
        },
        domain: (xs.d0, xs.d1),
        range: (plot.x, plot.right()),
    });
    meta.y = Some(AxisMeta {
        kind: if ys.log { AxisKind::Log } else { AxisKind::Number },
        domain: (ys.d0, ys.d1),
        range: (plot.bottom(), plot.y),
    });
    meta.points = Some(points);
    meta.summary = summary;
    meta
}

/// 점 라벨 글자 (점 라벨 구역): 앞·뒤 글자, 소수 자리
pub(crate) fn label_text(o: &crate::options::PointLabelOptions, v: f64) -> String {
    let body = match o.decimals {
        Some(d) => format!("{:.*}", d as usize, v),
        None => short_number(v),
    };
    format!("{}{}{}", o.prefix, body, o.suffix)
}

/// 라벨 하나: 위치(위·가운데·아래) · 각도 · 상자
pub(crate) fn draw_label(c: &mut Canvas, t: &Style, s: &str, x: f32, y: f32) {
    let o = &t.o.point_label;
    let px = t.font(o.font_size, 10.0);
    let base = match o.position {
        LabelPosition::Top => y - 6.0,
        LabelPosition::Center => y + px * 0.36,
        LabelPosition::Bottom => y + px + 6.0,
    };
    if o.label_box.visible && o.angle.abs() < 1.0 {
        let w = super::text::measure(s, px);
        let (bx, by, bw, bh) = (x - w / 2.0 - 3.0, base - px - 1.0, w + 6.0, px + 5.0);
        c.fill_rect(bx, by, bw, bh, color_or(&o.label_box.background, 0xffffff), 0.9);
        c.stroke_rect(bx, by, bw, bh, color_or(&o.label_box.border, t.axis), 1.0);
    }
    c.text_rotated(s, x, base, px, color_or(&o.color, t.text_secondary), Align::Center, o.angle);
}

fn draw_point_labels(c: &mut Canvas, t: &Style, points: &PointIndex, plot: RectF) {
    // 너무 많으면 글자끼리 겹치므로 40px 칸마다 하나
    let mut seen = std::collections::HashSet::new();
    for i in 0..points.len() {
        let key = (((points.px[i] - plot.x) / 40.0) as i32, ((points.py[i] - plot.y) / 16.0) as i32);
        if !seen.insert(key) || seen.len() > 400 {
            continue;
        }
        let s = label_text(&t.o.point_label, points.y[i]);
        draw_label(c, t, &s, points.px[i], points.py[i]);
    }
}

/// 점수 상자 (점수 구역): 오른쪽 위에 줄마다 한 점수
pub(crate) fn draw_score_box(c: &mut Canvas, t: &Style, plot: RectF, lines: &[String]) {
    let o = &t.o.score;
    let px = t.font(o.font_size, 11.0);
    let w = lines.iter().map(|l| super::text::measure(l, px)).fold(0.0, f32::max) + 12.0;
    let h = lines.len() as f32 * (px + 4.0) + 8.0;
    let (x, y) = (plot.right() - w - 6.0, plot.y + 6.0);
    c.fill_rect(x, y, w, h, color_or(&o.back_color, 0xffffff), 0.92);
    if o.border_width > 0.0 {
        c.stroke_rect(x, y, w, h, color_or(&o.border_color, t.axis), o.border_width);
    }
    for (k, l) in lines.iter().enumerate() {
        c.text(
            l,
            x + 6.0,
            y + 4.0 + (k as f32 + 1.0) * (px + 4.0) - 3.0,
            px,
            color_or(&o.font_color, t.text_primary),
            Align::Left,
        );
    }
}

/// 픽셀 열마다 첫 점·최솟값·최댓값·끝 점만 남긴다 (M4). order는 x 순서
fn m4(order: &[u32], sx: &[f32], sy: &[f32], left: f32) -> Vec<(f32, f32)> {
    let mut out = Vec::with_capacity(order.len().min(8192));
    let mut i = 0;
    while i < order.len() {
        let col = (sx[order[i] as usize] - left).floor() as i64;
        let first = order[i] as usize;
        let (mut lo, mut hi, mut last) = (first, first, first);
        let mut j = i + 1;
        while j < order.len() && (sx[order[j] as usize] - left).floor() as i64 == col {
            let k = order[j] as usize;
            if sy[k] < sy[lo] {
                lo = k;
            }
            if sy[k] > sy[hi] {
                hi = k;
            }
            last = k;
            j += 1;
        }
        let mut picks = [first, lo, hi, last];
        picks[1..3].sort_by(|a, b| sx[*a].total_cmp(&sx[*b]));
        let mut prev = usize::MAX;
        for p in picks {
            if p != prev {
                out.push((sx[p], sy[p]));
                prev = p;
            }
        }
        i = j;
    }
    out
}

pub fn fmt_int(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}
