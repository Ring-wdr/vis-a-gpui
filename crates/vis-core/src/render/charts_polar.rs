//! pie · radar (원본 pie.ts, radar.ts). 이동·확대가 없는 고정 좌표 차트다.

use std::f32::consts::{PI, TAU};

use tiny_skia::PathBuilder;

use super::canvas::{Align, Canvas, polygon};
use super::frame::{self, FONT, draw_title, frame_plot};
use super::meta::{Hit, HitShape, ItemMeta};
use super::scale::{Linear, short_number};
use super::text;
use super::theme::{Theme, luminance};
use super::{ItemRender, legend_name, series_color, visible};
use crate::query::category::CategoryItem;

pub fn render_pie(c: &mut Canvas, t: &Theme, input: &ItemRender, item: &CategoryItem) -> ItemMeta {
    let plot = if input.compact {
        frame::compact_plot(input.width, input.height)
    } else {
        frame_plot(input.width, input.height, 0.0, input.legend.as_ref(), false)
    };
    let slices: Vec<(usize, f64)> = item
        .bars
        .iter()
        .enumerate()
        .filter(|(i, _)| visible(input, *i))
        .filter_map(|(i, b)| b[0].filter(|v| *v > 0.0).map(|v| (i, v)))
        .collect();
    let total: f64 = slices.iter().map(|s| s.1).sum();
    let mut meta = ItemMeta::new(input.width, input.height, plot);
    let summary = format!("합계 {}", short_number(total));
    if !input.compact {
        draw_title(c, t, &input.title, &summary);
        if let Some(l) = &input.legend {
            meta.legend = frame::draw_legend(c, t, l, plot);
        }
    }
    let cx = plot.x + plot.w / 2.0;
    let cy = plot.y + plot.h / 2.0;
    let r = (plot.w.min(plot.h) / 2.0 - 4.0).max(4.0);
    if slices.is_empty() || total <= 0.0 {
        frame::draw_empty(c, t, plot);
        meta.summary = summary;
        return meta;
    }
    let mut a = 0.0f32;
    for &(i, v) in &slices {
        let sweep = (v / total) as f32 * TAU;
        let color = series_color(input, i);
        let mut pb = PathBuilder::new();
        pb.move_to(cx, cy);
        let steps = ((sweep / TAU * 96.0).ceil() as usize).max(2);
        for s in 0..=steps {
            let ang = a + sweep * s as f32 / steps as f32;
            pb.line_to(cx + r * ang.sin(), cy - r * ang.cos());
        }
        pb.close();
        if let Some(p) = pb.finish() {
            c.fill_path(&p, color, input.alpha);
            c.stroke_path(&p, t.surface, 1.0, 1.0, None);
        }
        let ratio = v / total * 100.0;
        if !input.compact && sweep > 0.25 {
            let mid = a + sweep / 2.0;
            let (lx, ly) = (cx + r * 0.65 * mid.sin(), cy - r * 0.65 * mid.cos());
            let ink = if luminance(color) > 0.6 { 0x0b0b0b } else { 0xffffff };
            c.text(&format!("{ratio:.0}%"), lx, ly + 4.0, FONT, ink, Align::Center);
        }
        meta.hits.push(Hit {
            shape: HitShape::Arc {
                cx,
                cy,
                r0: 0.0,
                r1: r,
                a0: a,
                a1: a + sweep,
            },
            lines: vec![
                item.categories[i].clone(),
                format!("값 {}", short_number(v)),
                format!("비율 {ratio:.1}%"),
            ],
            color: Some(color),
        });
        a += sweep;
    }
    meta.summary = summary;
    meta
}

pub fn render_radar(c: &mut Canvas, t: &Theme, input: &ItemRender, item: &CategoryItem) -> ItemMeta {
    let plot = if input.compact {
        frame::compact_plot(input.width, input.height)
    } else {
        frame_plot(input.width, input.height, 0.0, input.legend.as_ref(), false)
    };
    let mut meta = ItemMeta::new(input.width, input.height, plot);
    let axes = item.categories.len();
    let summary = format!("축 {axes}개");
    if !input.compact {
        draw_title(c, t, &input.title, &summary);
        if let Some(l) = &input.legend {
            meta.legend = frame::draw_legend(c, t, l, plot);
        }
    }
    if axes < crate::query::category::RADAR_MIN {
        frame::draw_empty(c, t, plot);
        meta.summary = "축이 3개 이상이어야 그린다".into();
        return meta;
    }
    let width = item.bars.first().map(|b| b.len()).unwrap_or(1);
    let (mut lo, mut hi) = (0.0f64, f64::NEG_INFINITY);
    for cell in &item.bars {
        for (g, v) in cell.iter().enumerate() {
            if let (true, Some(v)) = (visible(input, g), v) {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
        }
    }
    if !hi.is_finite() || hi == lo {
        hi = lo + 1.0;
    }
    let s = Linear::new((lo, hi), (0.0, 1.0)).nice(4);
    let ticks = s.ticks(4);
    let cx = plot.x + plot.w / 2.0;
    let cy = plot.y + plot.h / 2.0 + 4.0;
    let label_room = if input.compact { 0.0 } else { 26.0 };
    let r = (plot.w.min(plot.h) / 2.0 - label_room).max(8.0);
    let at = |k: usize, frac: f32| {
        let ang = TAU * k as f32 / axes as f32;
        (cx + r * frac * ang.sin(), cy - r * frac * ang.cos())
    };
    let frac = |v: f64| ((v - s.d0) / (s.d1 - s.d0)).clamp(0.0, 1.0) as f32;
    // 눈금 다각형과 축
    for &tv in &ticks {
        let f = frac(tv);
        if f <= 0.0 {
            continue;
        }
        let pts: Vec<(f32, f32)> = (0..axes).map(|k| at(k, f)).collect();
        if let Some(p) = polygon(&pts) {
            c.stroke_path(&p, t.grid, 1.0, 1.0, None);
        }
        if !input.compact {
            c.text(&short_number(tv), cx + 3.0, cy - r * f - 2.0, FONT - 1.0, t.text_muted, Align::Left);
        }
    }
    for k in 0..axes {
        let (x, y) = at(k, 1.0);
        c.line(cx, cy, x, y, t.grid, 1.0);
        if !input.compact {
            let ang = TAU * k as f32 / axes as f32;
            let (lx, ly) = at(k, 1.0 + 14.0 / r);
            let align = if ang.sin().abs() < 0.2 {
                Align::Center
            } else if ang.sin() > 0.0 {
                Align::Left
            } else {
                Align::Right
            };
            let s = text::truncate(&item.categories[k], FONT, 90.0);
            c.text(&s, lx, ly + 4.0, FONT, t.text_secondary, align);
        }
    }
    for g in 0..width {
        if !visible(input, g) {
            continue;
        }
        let color = series_color(input, g);
        let pts: Vec<(f32, f32)> = (0..axes).map(|k| at(k, frac(item.bars[k][g].unwrap_or(s.d0)))).collect();
        if let Some(p) = polygon(&pts) {
            c.fill_path(&p, color, 0.18 * input.alpha.max(0.2));
            c.stroke_path(&p, color, input.alpha, 2.0, None);
        }
        for (k, &(x, y)) in pts.iter().enumerate() {
            c.circle(x, y, 2.5, color, 1.0);
            let mut lines = vec![item.categories[k].clone()];
            if let Some(n) = legend_name(input, g) {
                lines.push(n);
            }
            lines.push(format!("값 {}", item.bars[k][g].map(short_number).unwrap_or("-".into())));
            meta.hits.push(Hit {
                shape: HitShape::Circle { cx: x, cy: y, r: 7.0 },
                lines,
                color: Some(color),
            });
        }
    }
    let _ = PI;
    meta.summary = summary;
    meta
}
