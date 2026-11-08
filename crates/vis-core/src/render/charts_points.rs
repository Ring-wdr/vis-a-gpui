//! scatter · line (원본 scatter.ts, line.ts).
//! - scatter: 점마다 반지름 3 원 + 진한 1px 테두리. 같은 색 점은 픽셀 칸마다 3개까지만 겹쳐 그린다.
//!   점이 그림 영역을 30겹 넘게 덮으면 반지름을 줄인다(최소 1.5).
//! - line: legend 계열마다 x 순으로 정렬한 점을 픽셀 열마다 첫·최소·최대·끝 넷만 남겨(M4) 잇는다.
//! - metadata: 같은 픽셀 칸에 겹친 점은 첫 점만 남긴 점 목록 (툴팁의 가장 가까운 점)

use rayon::prelude::*;

use super::canvas::Canvas;
use super::frame::{self, draw_title, draw_x_axis, draw_y_axis, frame_plot};
use super::meta::{AxisKind, AxisMeta, ItemMeta, PointIndex};
use super::scale::{Linear, extent, number_format, time_ticks};
use super::theme::{Theme, mix};
use super::{ItemRender, Progress, series_color, visible};
use crate::query::points::PointsData;
use crate::timefmt::{axis_format, format_kst};

const POINT_RADIUS: f32 = 3.0;
const MAX_COVERAGE: f32 = 30.0;
const MAX_PER_CELL: u8 = 3;
const MIN_POINT_RADIUS: f32 = 1.5;
const LINE_DOTS: usize = 400;

pub fn render_points(
    c: &mut Canvas,
    t: &Theme,
    input: &ItemRender,
    line: bool,
    x_time: bool,
    d: &PointsData,
    progress: Progress,
) -> ItemMeta {
    let n = d.xs.len();
    let zoomed = input.view.is_some();
    let (xd, yd) = match input.view {
        Some(v) => (v.x, v.y),
        None => (extent(d.xs.iter().copied(), x_time), extent(d.ys.iter().copied(), false)),
    };
    let mut ys = Linear::new(yd, (1.0, 0.0));
    if !zoomed {
        ys = ys.nice(5);
    }
    let y_ticks = ys.ticks(5);
    let y_fmt = number_format(&y_ticks);
    let left = if input.compact {
        0.0
    } else {
        frame::label_width(y_ticks.iter().map(|&v| y_fmt(v)))
    };
    let plot = if input.compact {
        frame::compact_plot(input.width, input.height)
    } else {
        frame_plot(input.width, input.height, left, input.legend.as_ref(), true)
    };
    let ys = Linear::new((ys.d0, ys.d1), ((plot.y + plot.h) as f64, plot.y as f64));
    let mut xs = Linear::new(xd, (plot.x as f64, (plot.x + plot.w) as f64));
    if !zoomed && !x_time {
        xs = xs.nice(5);
    }
    let x_count = ((plot.w / 90.0).floor() as usize).max(2);
    let x_ticks = if x_time {
        time_ticks(xs.d0, xs.d1, x_count)
    } else {
        xs.ticks(x_count)
    };
    let num_fmt = number_format(&x_ticks);
    let span = xs.d1 - xs.d0;
    let x_fmt = move |v: f64| if x_time { format_kst(v, axis_format(span)) } else { num_fmt(v) };

    // ── 화면 좌표 (병렬) ──
    let sx: Vec<f32> = d.xs.par_iter().map(|&v| xs.map(v) as f32).collect();
    let sy: Vec<f32> = d.ys.par_iter().map(|&v| ys.map(v) as f32).collect();
    progress(0.3);

    // ── 픽셀 칸마다 첫 점만 남긴 metadata ──
    let pw = plot.w.ceil().max(1.0) as usize;
    let ph = plot.h.ceil().max(1.0) as usize;
    let mut cell = vec![false; pw * ph];
    let mut points = PointIndex::default();
    let group = |i: usize| d.legend.as_ref().map(|l| l[i]).unwrap_or(0);
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
    if !input.compact {
        draw_y_axis(c, t, plot, |v| ys.map(v) as f32, &y_ticks, &y_fmt);
        draw_x_axis(c, t, plot, |v| xs.map(v) as f32, &x_ticks, &x_fmt);
        draw_title(c, t, &input.title, &summary);
        if let Some(l) = &input.legend {
            meta.legend = frame::draw_legend(c, t, l, plot);
            meta.legend_values = l.values.iter().cloned().zip(l.colors.iter().copied()).collect();
        }
    }
    c.set_clip(Some((plot.x, plot.y, plot.w, plot.h)));
    let groups = input.legend.as_ref().map(|l| l.values.len()).unwrap_or(1).max(1);
    if n == 0 {
        frame::draw_empty(c, t, plot);
    } else if line {
        let lw = if input.compact { 1.0 } else { 1.5 };
        // 계열마다 x 순서
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
        for (g, o) in order.iter().enumerate() {
            if o.is_empty() {
                continue;
            }
            let color = series_color(input, g);
            let pts = m4(o, &sx, &sy, plot.x);
            c.polyline_aa(&pts, color, input.alpha, lw);
            if o.len() <= LINE_DOTS && !input.compact {
                let s = c.stamp(2.5, color, None, input.alpha);
                for &i in o {
                    c.put(&s, sx[i as usize], sy[i as usize]);
                }
            }
        }
    } else {
        let area = (plot.w * plot.h).max(1.0);
        let drawable = n.min(kept * MAX_PER_CELL as usize) as f32;
        let coverage = |r: f32| drawable * std::f32::consts::PI * r * r / area;
        let radius = if input.compact {
            1.5
        } else {
            (POINT_RADIUS * (MAX_COVERAGE / coverage(POINT_RADIUS)).sqrt().min(1.0)).max(MIN_POINT_RADIUS)
        };
        let reach = radius + 1.0;
        let mut drawn = vec![0u8; pw * ph];
        for g in 0..groups {
            if !visible(input, g) {
                continue;
            }
            let color = series_color(input, g);
            let border = if input.compact {
                None
            } else {
                Some((mix(color, 0x000000, 0.25), 1.0))
            };
            let stamp = c.stamp(radius, color, border, input.alpha);
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
            progress(0.5 + 0.5 * (g + 1) as f32 / groups as f32);
        }
    }
    c.set_clip(None);
    meta.x = Some(AxisMeta {
        kind: if x_time { AxisKind::Time } else { AxisKind::Number },
        domain: (xs.d0, xs.d1),
        range: (plot.x, plot.right()),
    });
    meta.y = Some(AxisMeta {
        kind: AxisKind::Number,
        domain: (ys.d0, ys.d1),
        range: (plot.bottom(), plot.y),
    });
    meta.points = Some(points);
    meta.summary = summary;
    meta
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
