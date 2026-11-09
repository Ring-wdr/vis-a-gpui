//! heatmap · candle · gantt · treemap · funnel · sankey

use tiny_skia::PathBuilder;

use super::canvas::{Align, Canvas, polygon};
use super::frame::{self, FONT, draw_category_labels, draw_row_labels, frame_plot};
use super::meta::{AxisKind, AxisMeta, Hit, HitShape, ItemMeta, RectF};
use super::scale::{Linear, extent, number_format, short_number, time_ticks};
use super::text;
use super::theme::{OTHERS_COLOR, Style, luminance, mix, palette, sequential};
use super::{ItemRender, legend_name, series_color, visible};
use crate::query::candle::CandleItem;
use crate::query::category::{CategoryItem, OTHERS};
use crate::query::gantt::GanttItem;
use crate::query::heatmap::HeatItem;
use crate::query::sankey::SankeyItem;
use crate::timefmt::format_kst;

fn ink_on(c: u32) -> u32 {
    if luminance(c) > 0.6 { 0x0b0b0b } else { 0xffffff }
}

pub fn render_heatmap(c: &mut Canvas, t: &Style, input: &ItemRender, item: &HeatItem) -> ItemMeta {
    let left = if input.compact {
        0.0
    } else {
        frame::row_label_width(t, item.ys.iter().cloned()).min(90.0)
    };
    let plot = if input.compact {
        frame::compact_plot(input.width, input.height)
    } else {
        frame_plot(t, input.width, input.height, left, None, true)
    };
    let (lo, hi) = extent(item.cells.iter().flatten().flatten().copied(), false);
    let mut meta = ItemMeta::new(input.width, input.height, plot);
    let summary = format!(
        "{} × {} · {} ~ {}",
        item.xs.len(),
        item.ys.len(),
        short_number(lo),
        short_number(hi)
    );
    let nx = item.xs.len().max(1) as f32;
    let ny = item.ys.len().max(1) as f32;
    let cw = plot.w / nx;
    let ch = plot.h / ny;
    if !input.compact {
        draw_category_labels(c, t, plot, &item.xs, |i| plot.x + cw * (i as f32 + 0.5), cw);
        draw_row_labels(c, t, plot, &item.ys, |j| plot.y + ch * (j as f32 + 0.5), left);
    }
    if item.xs.is_empty() {
        frame::draw_empty(c, t, plot);
    }
    // 팔레트 › 그라데이션을 쓰면 시작 → (중간) → 끝 색으로 값을 칠한다
    let pal = &t.o.palette;
    let scale = |f: f32| {
        if pal.gradient {
            let g = super::theme::gradient_palette(pal, 64);
            g[((f.clamp(0.0, 1.0) * 63.0).round() as usize).min(63)]
        } else {
            sequential(f)
        }
    };
    for (i, col) in item.cells.iter().enumerate() {
        for (j, v) in col.iter().enumerate() {
            // 값 › 빈 칸 값
            let Some(v) = v.or(t.o.value.null_numeric) else { continue };
            let v = &v;
            let f = ((v - lo) / (hi - lo)) as f32;
            let color = scale(f);
            let r = RectF::new(plot.x + cw * i as f32, plot.y + ch * j as f32, cw, ch);
            c.fill_rect(
                r.x,
                r.y,
                (r.w - if cw > 4.0 { 1.0 } else { 0.0 }).max(0.5),
                (r.h - if ch > 4.0 { 1.0 } else { 0.0 }).max(0.5),
                color,
                1.0,
            );
            if !input.compact && cw > 34.0 && ch > 14.0 {
                c.text(
                    &if t.o.point_label.visible {
                        super::charts_points::label_text(&t.o.point_label, *v)
                    } else {
                        short_number(*v)
                    },
                    r.x + cw / 2.0,
                    r.y + ch / 2.0 + 4.0,
                    FONT - 1.0,
                    ink_on(color),
                    Align::Center,
                );
            }
            meta.hits.push(Hit {
                shape: HitShape::Rect(r),
                lines: vec![format!("{} · {}", item.xs[i], item.ys[j]), format!("값 {}", short_number(*v))],
                color: Some(color),
            });
        }
    }
    meta.summary = summary;
    meta
}

pub fn render_candle(c: &mut Canvas, t: &Style, input: &ItemRender, item: &CandleItem) -> ItemMeta {
    let cs = &item.candles;
    let interval = if cs.len() >= 2 { cs[1].t - cs[0].t } else { 3_600_000.0 };
    let (xd, yd) = match input.view {
        Some(v) => (v.x, v.y),
        None => (
            extent(cs.iter().flat_map(|k| [k.t, k.t + interval]), true),
            extent(cs.iter().flat_map(|k| [k.low, k.high]), false),
        ),
    };
    let ya = &t.o.axes.y;
    let yd = if input.view.is_none() { frame::axis_domain(ya, yd) } else { yd };
    let mut ys = Linear::new(yd, (1.0, 0.0)).with_log(ya.log);
    if input.view.is_none() && ya.min.is_none() && ya.max.is_none() {
        ys = ys.nice(frame::y_tick_count(t));
    }
    let ticks = ys.ticks(frame::y_tick_count(t));
    let fmt = number_format(&ticks);
    let left = if input.compact {
        0.0
    } else {
        frame::label_width(t, ticks.iter().map(|&v| (v, fmt(v))))
    };
    let plot = if input.compact {
        frame::compact_plot(input.width, input.height)
    } else {
        frame_plot(t, input.width, input.height, left, input.legend.as_ref(), true)
    };
    let ys = ys.with_range((plot.bottom() as f64, plot.y as f64));
    let xs = Linear::new(xd, (plot.x as f64, plot.right() as f64));
    let x_ticks = time_ticks(xs.d0, xs.d1, frame::x_tick_count(t, plot));
    let span = xs.d1 - xs.d0;
    let mut meta = ItemMeta::new(input.width, input.height, plot);
    let summary = format!("{}개 구간", cs.len());
    if !input.compact {
        frame::draw_y_axis_titled(c, t, plot, |v| ys.map(v) as f32, &ticks, &fmt, &input.y_title);
        let date = frame::date_fmt(t, span).to_string();
        frame::draw_x_axis_titled(
            c,
            t,
            plot,
            |v| xs.map(v) as f32,
            &x_ticks,
            &|v| format_kst(v, &date),
            &input.x_title,
        );
        if let Some(l) = &input.legend {
            meta.legend = frame::draw_legend(c, t, l, plot);
        }
    }
    c.set_clip(Some((plot.x, plot.y, plot.w, plot.h)));
    if cs.is_empty() {
        frame::draw_empty(c, t, plot);
    }
    let bw = ((xs.map(interval) - xs.map(0.0)) as f32 * 0.7).max(1.0);
    for k in cs {
        let up = k.close >= k.open;
        let kind = if up { 0 } else { 1 };
        if !visible(input, kind) {
            continue;
        }
        let color = series_color(input, kind);
        let cx = xs.map(k.t + interval / 2.0) as f32;
        if cx < plot.x - bw || cx > plot.right() + bw {
            continue;
        }
        c.line(cx, ys.map(k.high) as f32, cx, ys.map(k.low) as f32, color, t.o.size.line_width);
        let (yo, yc) = (ys.map(k.open) as f32, ys.map(k.close) as f32);
        c.fill_rect(cx - bw / 2.0, yo.min(yc), bw, (yo - yc).abs().max(1.0), color, input.alpha);
        if t.o.point_label.visible && !input.compact {
            let s = super::charts_points::label_text(&t.o.point_label, k.close);
            super::charts_points::draw_label(c, t, &s, cx, yo.min(yc));
        }
        meta.hits.push(Hit {
            shape: HitShape::Rect(RectF::new(
                cx - bw / 2.0 - 1.0,
                ys.map(k.high) as f32,
                bw + 2.0,
                ((ys.map(k.low) - ys.map(k.high)) as f32).max(4.0),
            )),
            lines: vec![
                format_kst(k.t, "%m/%d %H:%M"),
                format!("시가 {} · 종가 {}", short_number(k.open), short_number(k.close)),
                format!("고가 {} · 저가 {}", short_number(k.high), short_number(k.low)),
                format!("개수 {}", k.count),
            ],
            color: Some(color),
        });
    }
    c.set_clip(None);
    meta.x = Some(AxisMeta {
        kind: AxisKind::Time,
        domain: (xs.d0, xs.d1),
        range: (plot.x, plot.right()),
    });
    meta.y = Some(AxisMeta {
        kind: AxisKind::Number,
        domain: (ys.d0, ys.d1),
        range: (plot.bottom(), plot.y),
    });
    meta.summary = summary;
    meta
}

pub fn render_gantt(c: &mut Canvas, t: &Style, input: &ItemRender, item: &GanttItem) -> ItemMeta {
    let n = item.rows.len();
    let (xd, yd) = match input.view {
        Some(v) => (v.x, v.y),
        None => (
            extent(item.bars.iter().flat_map(|b| [b.start, b.end]), true),
            (-0.5, n as f64 - 0.5),
        ),
    };
    let left = if input.compact {
        0.0
    } else {
        frame::row_label_width(t, item.rows.iter().cloned()).min(90.0)
    };
    let plot = if input.compact {
        frame::compact_plot(input.width, input.height)
    } else {
        frame_plot(t, input.width, input.height, left, input.legend.as_ref(), true)
    };
    let xs = Linear::new(xd, (plot.x as f64, plot.right() as f64));
    let ys = Linear::new(yd, (plot.y as f64, plot.bottom() as f64));
    let x_ticks = time_ticks(xs.d0, xs.d1, frame::x_tick_count(t, plot));
    let span = xs.d1 - xs.d0;
    let mut meta = ItemMeta::new(input.width, input.height, plot);
    let summary = format!("{}행 · {}구간", n, item.bars.len());
    let band = (ys.map(1.0) - ys.map(0.0)).abs() as f32;
    if !input.compact {
        let date = frame::date_fmt(t, span).to_string();
        frame::draw_x_axis_titled(
            c,
            t,
            plot,
            |v| xs.map(v) as f32,
            &x_ticks,
            &|v| format_kst(v, &date),
            &input.x_title,
        );
        draw_row_labels(c, t, plot, &item.rows, |j| ys.map(j as f64) as f32, left);
        if let Some(l) = &input.legend {
            meta.legend = frame::draw_legend(c, t, l, plot);
        }
    }
    c.set_clip(Some((plot.x, plot.y, plot.w, plot.h)));
    if item.bars.is_empty() {
        frame::draw_empty(c, t, plot);
    }
    let bh = (band * 0.7).max(1.0);
    // 같은 행에서 픽셀 칸 안에 시작하고 끝나는 짧은 구간은 앞 막대에 합쳐 그린다
    let mut pending: Option<(u32, u32, f32, f32, u32)> = None; // row, legend, x0, x1, count
    let flush = |c: &mut Canvas, meta: &mut ItemMeta, p: (u32, u32, f32, f32, u32)| {
        let (row, g, x0, x1, count) = p;
        let color = series_color(input, g as usize);
        let cy = ys.map(row as f64) as f32;
        if let Some(p) = super::canvas::rounded_rect(
            x0,
            cy - bh / 2.0,
            (x1 - x0).max(1.0),
            bh,
            t.o.shape.corner_radius.min(bh / 4.0).min((x1 - x0) / 2.0).max(0.0),
        ) {
            super::charts_category::fill_shape(c, t, &p, color, input.alpha);
        }
        if meta.hits.len() < 30_000 {
            let mut lines = vec![item.rows[row as usize].clone()];
            if let Some(n) = legend_name(input, g as usize) {
                lines.push(n);
            }
            lines.push(format!(
                "{} ~ {}",
                format_kst(xs.invert(x0 as f64), "%m/%d %H:%M"),
                format_kst(xs.invert(x1 as f64), "%m/%d %H:%M")
            ));
            lines.push(format!("행 {count}"));
            meta.hits.push(Hit {
                shape: HitShape::Rect(RectF::new(x0, cy - bh / 2.0, (x1 - x0).max(2.0), bh)),
                lines,
                color: Some(color),
            });
        }
    };
    for b in &item.bars {
        if !visible(input, b.legend as usize) {
            continue;
        }
        let cy = ys.map(b.row as f64) as f32;
        if cy < plot.y - band || cy > plot.bottom() + band {
            continue;
        }
        let x0 = xs.map(b.start) as f32;
        let x1 = xs.map(b.end) as f32;
        if x1 < plot.x || x0 > plot.right() {
            continue;
        }
        match &mut pending {
            Some(p) if p.0 == b.row && p.1 == b.legend && x0 - p.3 < 1.0 => {
                p.3 = p.3.max(x1);
                p.4 += b.count;
            }
            Some(p) if p.0 == b.row && x1 - x0 < 1.0 && x0.floor() == p.3.floor() => {
                p.3 = p.3.max(x1);
                p.4 += b.count;
            }
            _ => {
                if let Some(p) = pending.take() {
                    flush(c, &mut meta, p);
                }
                pending = Some((b.row, b.legend, x0, x1, b.count));
            }
        }
    }
    if let Some(p) = pending.take() {
        flush(c, &mut meta, p);
    }
    c.set_clip(None);
    meta.x = Some(AxisMeta {
        kind: AxisKind::Time,
        domain: (xs.d0, xs.d1),
        range: (plot.x, plot.right()),
    });
    meta.y = Some(AxisMeta {
        kind: AxisKind::Index,
        domain: (ys.d0, ys.d1),
        range: (plot.y, plot.bottom()),
    });
    meta.summary = summary;
    meta
}

// ── treemap (squarified) ──────────────────────────────────────────────

fn squarify(values: &[f64], r: RectF) -> Vec<RectF> {
    let total: f64 = values.iter().sum();
    let mut out = vec![RectF::default(); values.len()];
    if total <= 0.0 || values.is_empty() {
        return out;
    }
    let scale = (r.w * r.h) as f64 / total;
    let areas: Vec<f64> = values.iter().map(|v| v * scale).collect();
    let mut rect = r;
    let mut i = 0;
    while i < areas.len() {
        let short = rect.w.min(rect.h) as f64;
        let worst = |row: &[f64]| -> f64 {
            let s: f64 = row.iter().sum();
            let mx = row.iter().cloned().fold(0.0, f64::max);
            let mn = row.iter().cloned().fold(f64::INFINITY, f64::min);
            ((short * short * mx) / (s * s)).max((s * s) / (short * short * mn))
        };
        let mut j = i + 1;
        while j < areas.len() && worst(&areas[i..j + 1]) <= worst(&areas[i..j]) {
            j += 1;
        }
        let row = &areas[i..j];
        let s: f64 = row.iter().sum();
        if rect.w >= rect.h {
            let w = (s / rect.h as f64) as f32;
            let mut y = rect.y;
            for (k, a) in row.iter().enumerate() {
                let h = (*a / w as f64) as f32;
                out[i + k] = RectF::new(rect.x, y, w, h);
                y += h;
            }
            rect = RectF::new(rect.x + w, rect.y, (rect.w - w).max(0.0), rect.h);
        } else {
            let h = (s / rect.w as f64) as f32;
            let mut x = rect.x;
            for (k, a) in row.iter().enumerate() {
                let w = (*a / h as f64) as f32;
                out[i + k] = RectF::new(x, rect.y, w, h);
                x += w;
            }
            rect = RectF::new(rect.x, rect.y + h, rect.w, (rect.h - h).max(0.0));
        }
        i = j;
    }
    out
}

pub fn render_treemap(c: &mut Canvas, t: &Style, input: &ItemRender, item: &CategoryItem) -> ItemMeta {
    let plot = if input.compact {
        frame::compact_plot(input.width, input.height)
    } else {
        frame_plot(t, input.width, input.height, 0.0, input.legend.as_ref(), false)
    };
    let mut meta = ItemMeta::new(input.width, input.height, plot);
    let width = item.bars.first().map(|b| b.len()).unwrap_or(1);
    let has_legend = input.legend.is_some();
    // (legend 칸, 범주, 값) — legend가 있으면 legend 값마다 한 덩어리
    let mut tiles: Vec<(usize, usize, f64)> = vec![];
    for g in 0..width {
        if !visible(input, g) {
            continue;
        }
        for (i, cell) in item.bars.iter().enumerate() {
            if let Some(v) = cell[g].filter(|v| *v > 0.0) {
                tiles.push((g, i, v));
            }
        }
    }
    let total: f64 = tiles.iter().map(|t| t.2).sum();
    let summary = format!("합계 {}", short_number(total));
    if !input.compact {
        if let Some(l) = &input.legend {
            meta.legend = frame::draw_legend(c, t, l, plot);
        }
    }
    if tiles.is_empty() {
        frame::draw_empty(c, t, plot);
        meta.summary = summary;
        return meta;
    }
    let mut groups: Vec<(usize, Vec<(usize, f64)>)> = vec![];
    for (g, i, v) in tiles {
        match groups.iter_mut().find(|x| x.0 == g) {
            Some(x) => x.1.push((i, v)),
            None => groups.push((g, vec![(i, v)])),
        }
    }
    for gr in groups.iter_mut() {
        gr.1.sort_by(|a, b| b.1.total_cmp(&a.1));
    }
    groups.sort_by(|a, b| {
        b.1.iter()
            .map(|x| x.1)
            .sum::<f64>()
            .total_cmp(&a.1.iter().map(|x| x.1).sum::<f64>())
    });
    let outer = squarify(&groups.iter().map(|g| g.1.iter().map(|x| x.1).sum()).collect::<Vec<_>>(), plot);
    let mut rank = 0usize;
    for (gr, orect) in groups.iter().zip(outer) {
        let inner = squarify(&gr.1.iter().map(|x| x.1).collect::<Vec<_>>(), orect);
        for (&(i, v), r) in gr.1.iter().zip(inner) {
            let color = if has_legend {
                series_color(input, gr.0)
            } else if rank < 8 {
                palette(rank)
            } else {
                OTHERS_COLOR
            };
            rank += 1;
            let fill = if has_legend {
                mix(color, 0xffffff, (rank % 3) as f32 * 0.08)
            } else {
                color
            };
            if let Some(p) = super::canvas::rounded_rect(r.x, r.y, (r.w - 1.0).max(0.5), (r.h - 1.0).max(0.5), 0.0) {
                super::charts_category::fill_shape(c, t, &p, fill, input.alpha);
            }
            if !input.compact && r.w > 40.0 && r.h > 18.0 {
                let s = text::truncate(&item.categories[i], FONT, r.w - 8.0);
                c.text(&s, r.x + 4.0, r.y + 14.0, FONT, ink_on(fill), Align::Left);
                if r.h > 32.0 {
                    let s = if t.o.point_label.visible {
                        super::charts_points::label_text(&t.o.point_label, v)
                    } else {
                        short_number(v)
                    };
                    c.text(&s, r.x + 4.0, r.y + 28.0, FONT - 1.0, ink_on(fill), Align::Left);
                }
            }
            let mut lines = vec![item.categories[i].clone()];
            if let Some(n) = legend_name(input, gr.0) {
                lines.push(n);
            }
            lines.push(format!("값 {} ({:.1}%)", short_number(v), v / total * 100.0));
            meta.hits.push(Hit {
                shape: HitShape::Rect(r),
                lines,
                color: Some(fill),
            });
        }
    }
    meta.summary = summary;
    meta
}

pub fn render_funnel(c: &mut Canvas, t: &Style, input: &ItemRender, item: &CategoryItem) -> ItemMeta {
    let plot = if input.compact {
        frame::compact_plot(input.width, input.height)
    } else {
        frame_plot(t, input.width, input.height, 0.0, input.legend.as_ref(), false)
    };
    let mut meta = ItemMeta::new(input.width, input.height, plot);
    let stages: Vec<(usize, f64)> = item
        .bars
        .iter()
        .enumerate()
        .filter(|(i, _)| visible(input, *i))
        .filter_map(|(i, b)| b[0].map(|v| (i, v)))
        .collect();
    let first = stages.first().map(|s| s.1).unwrap_or(0.0);
    let max = stages.iter().map(|s| s.1).fold(0.0, f64::max);
    let summary = format!("{}단계", stages.len());
    if !input.compact {
        if let Some(l) = &input.legend {
            meta.legend = frame::draw_legend(c, t, l, plot);
        }
    }
    if stages.is_empty() || max <= 0.0 {
        frame::draw_empty(c, t, plot);
        meta.summary = summary;
        return meta;
    }
    let n = stages.len() as f32;
    let slot = plot.h / n;
    let bh = slot * 0.62;
    let cx = plot.x + plot.w / 2.0;
    let mut prev: Option<(f32, f32, f32, f64)> = None; // left, right, bottom, value
    for (k, &(i, v)) in stages.iter().enumerate() {
        let w = (v.max(0.0) / max) as f32 * plot.w;
        let y = plot.y + slot * k as f32 + (slot - bh) / 2.0;
        let color = series_color(input, i);
        if let Some((pl, pr, pb, pv)) = prev {
            if let Some(p) = polygon(&[(pl, pb), (pr, pb), (cx + w / 2.0, y), (cx - w / 2.0, y)]) {
                c.fill_path(&p, t.grid, 1.0);
            }
            if !input.compact && pv > 0.0 && y - pb > 11.0 {
                c.text(
                    &format!("{:.1}%", v / pv * 100.0),
                    cx,
                    (pb + y) / 2.0 + 4.0,
                    FONT - 1.0,
                    t.text_muted,
                    Align::Center,
                );
            }
        }
        if let Some(p) = super::canvas::rounded_rect(cx - w / 2.0, y, w.max(1.0), bh, t.o.shape.corner_radius.min(bh / 2.0)) {
            super::charts_category::fill_shape(c, t, &p, color, input.alpha);
        }
        if !input.compact && bh > 12.0 {
            let label = format!("{} · {} ({:.0}%)", item.categories[i], short_number(v), v / first * 100.0);
            let tw = text::measure(&label, FONT);
            let ink = if tw + 8.0 < w { ink_on(color) } else { t.text_primary };
            c.text(&label, cx, y + bh / 2.0 + 4.0, FONT, ink, Align::Center);
        }
        let mut lines = vec![
            item.categories[i].clone(),
            format!("값 {}", short_number(v)),
            format!("첫 단계 대비 {:.1}%", v / first * 100.0),
        ];
        if let Some((_, _, _, pv)) = prev {
            lines.push(format!("이전 단계 대비 {:.1}%", v / pv * 100.0));
        }
        meta.hits.push(Hit {
            shape: HitShape::Rect(RectF::new(cx - w / 2.0, y, w.max(4.0), bh)),
            lines,
            color: Some(color),
        });
        prev = Some((cx - w / 2.0, cx + w / 2.0, y + bh, v));
    }
    meta.summary = summary;
    meta
}

pub fn render_sankey(c: &mut Canvas, t: &Style, input: &ItemRender, item: &SankeyItem) -> ItemMeta {
    let plot = if input.compact {
        frame::compact_plot(input.width, input.height)
    } else {
        frame_plot(t, input.width, input.height, 0.0, input.legend.as_ref(), false)
    };
    let mut meta = ItemMeta::new(input.width, input.height, plot);
    let flows: Vec<&(usize, usize, f64)> = item.flows.iter().filter(|f| visible(input, f.0)).collect();
    let total: f64 = flows.iter().map(|f| f.2).sum();
    let summary = format!("흐름 {}개", flows.len());
    if !input.compact {
        if let Some(l) = &input.legend {
            meta.legend = frame::draw_legend(c, t, l, plot);
        }
    }
    if flows.is_empty() || total <= 0.0 {
        frame::draw_empty(c, t, plot);
        meta.summary = summary;
        return meta;
    }
    let label_w = if input.compact { 0.0 } else { (plot.w * 0.18).min(90.0) };
    let node_w = 10.0;
    let x0 = plot.x + label_w;
    let x1 = plot.right() - label_w - node_w;
    let gap = 4.0;
    let sum_by = |side: usize, n: usize| {
        let mut v = vec![0.0f64; n];
        for f in &flows {
            v[if side == 0 { f.0 } else { f.1 }] += f.2;
        }
        v
    };
    let ssum = sum_by(0, item.sources.len());
    let tsum = sum_by(1, item.targets.len());
    let used = |v: &[f64]| v.iter().filter(|x| **x > 0.0).count().max(1) as f32;
    let kscale = |v: &[f64]| (plot.h - gap * (used(v) - 1.0)) / total as f32;
    let k = kscale(&ssum).min(kscale(&tsum));
    let place = |v: &[f64]| -> Vec<(f32, f32)> {
        let mut y = plot.y;
        v.iter()
            .map(|&s| {
                let h = s as f32 * k;
                let r = (y, h);
                if s > 0.0 {
                    y += h + gap;
                }
                r
            })
            .collect()
    };
    let sp = place(&ssum);
    let tp = place(&tsum);
    let mut sofs = vec![0.0f32; sp.len()];
    let mut tofs = vec![0.0f32; tp.len()];
    let mut sorted = flows.clone();
    sorted.sort_by_key(|f| (f.0, f.1));
    for f in sorted {
        let h = f.2 as f32 * k;
        let ys0 = sp[f.0].0 + sofs[f.0];
        let yt0 = tp[f.1].0 + tofs[f.1];
        sofs[f.0] += h;
        tofs[f.1] += h;
        let color = series_color(input, f.0);
        let (ax, bx) = (x0 + node_w, x1);
        let mx = (ax + bx) / 2.0;
        let mut pb = PathBuilder::new();
        pb.move_to(ax, ys0);
        pb.cubic_to(mx, ys0, mx, yt0, bx, yt0);
        pb.line_to(bx, yt0 + h);
        pb.cubic_to(mx, yt0 + h, mx, ys0 + h, ax, ys0 + h);
        pb.close();
        if let Some(p) = pb.finish() {
            c.fill_path(&p, color, 0.38 * input.alpha.max(0.3));
            if t.o.shape.border_width > 0.0 {
                c.stroke_path(&p, t.border_of(color), 0.8, t.o.shape.border_width, None);
            }
        }
        meta.hits.push(Hit {
            shape: HitShape::Polygon(vec![
                (ax, ys0),
                (mx, (ys0 + yt0) / 2.0),
                (bx, yt0),
                (bx, yt0 + h.max(2.0)),
                (mx, (ys0 + yt0) / 2.0 + h.max(2.0)),
                (ax, ys0 + h.max(2.0)),
            ]),
            lines: vec![
                format!("{} → {}", item.sources[f.0], item.targets[f.1]),
                format!("값 {}", short_number(f.2)),
            ],
            color: Some(color),
        });
    }
    for (i, &(y, h)) in sp.iter().enumerate() {
        if ssum[i] <= 0.0 {
            continue;
        }
        let color = series_color(input, i);
        c.fill_rect(x0, y, node_w, h.max(1.0), color, 1.0);
        if !input.compact {
            let s = text::truncate(&item.sources[i], FONT, label_w - 6.0);
            c.text(&s, x0 - 4.0, y + h / 2.0 + 4.0, FONT, t.text_secondary, Align::Right);
        }
        meta.hits.push(Hit {
            shape: HitShape::Rect(RectF::new(x0, y, node_w, h.max(2.0))),
            lines: vec![item.sources[i].clone(), format!("나가는 합 {}", short_number(ssum[i]))],
            color: Some(color),
        });
    }
    for (j, &(y, h)) in tp.iter().enumerate() {
        if tsum[j] <= 0.0 {
            continue;
        }
        let color = if item.targets[j] == OTHERS {
            OTHERS_COLOR
        } else {
            t.text_secondary
        };
        c.fill_rect(x1, y, node_w, h.max(1.0), color, 1.0);
        if !input.compact {
            let s = text::truncate(&item.targets[j], FONT, label_w - 6.0);
            c.text(&s, x1 + node_w + 4.0, y + h / 2.0 + 4.0, FONT, t.text_secondary, Align::Left);
        }
        meta.hits.push(Hit {
            shape: HitShape::Rect(RectF::new(x1, y, node_w, h.max(2.0))),
            lines: vec![item.targets[j].clone(), format!("들어오는 합 {}", short_number(tsum[j]))],
            color: Some(color),
        });
    }
    meta.summary = summary;
    meta
}
