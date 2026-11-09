//! pie · radar (원본 pie.ts, radar.ts). 이동·확대가 없는 고정 좌표 차트다.

use std::f32::consts::{PI, TAU};

use tiny_skia::PathBuilder;

use super::canvas::{Align, Canvas, polygon};
use super::frame::{self, FONT, frame_plot};
use super::meta::{Hit, HitShape, ItemMeta};
use super::scale::{Linear, short_number};
use super::text;
use super::theme::{Style, luminance, mix};
use super::{ItemRender, legend_name, series_color, visible};
use crate::query::category::CategoryItem;
use crate::spec::{AggFn, SubType};

/// pie 하위 차트: doughnut은 가운데를 비우고, pie3d는 원을 눕혀(타원) 앞쪽 옆면을 그리고,
/// sunburst는 안쪽 고리에 조각, 바깥 고리에 조각을 legend 컬럼 값으로 나눈 몫을 그린다 (원본 pie.ts)
const DOUGHNUT_HOLE: f32 = 0.55;
const SUNBURST_HOLE: f32 = 0.18;
const SUNBURST_RING: f32 = 0.6;
const TILT: f32 = 0.55;
const DEPTH: f32 = 0.14;

/// 부채꼴(r0 > 0이면 고리 조각). k는 세로 비율(pie3d)
fn sector(cx: f32, cy: f32, r0: f32, r1: f32, a0: f32, a1: f32, k: f32) -> Option<tiny_skia::Path> {
    let steps = (((a1 - a0) / TAU * 96.0).ceil() as usize).max(2);
    let at = |r: f32, a: f32| (cx + r * a.sin(), cy - r * k * a.cos());
    let mut pb = PathBuilder::new();
    if r0 <= 0.0 {
        pb.move_to(cx, cy);
    } else {
        let (x, y) = at(r0, a0);
        pb.move_to(x, y);
    }
    for s in 0..=steps {
        let (x, y) = at(r1, a0 + (a1 - a0) * s as f32 / steps as f32);
        pb.line_to(x, y);
    }
    if r0 > 0.0 {
        for s in (0..=steps).rev() {
            let (x, y) = at(r0, a0 + (a1 - a0) * s as f32 / steps as f32);
            pb.line_to(x, y);
        }
    }
    pb.close();
    pb.finish()
}

pub fn render_pie(c: &mut Canvas, t: &Style, input: &ItemRender, item: &CategoryItem) -> ItemMeta {
    let o = &t.o;
    let variant = input.sub_type;
    let plot = if input.compact {
        frame::compact_plot(input.width, input.height)
    } else {
        frame_plot(t, input.width, input.height, 0.0, input.legend.as_ref(), false)
    };
    // 조각 값: legend(sunburst)가 있으면 legend 값별 몫의 합
    let cell_sum = |b: &Vec<Option<f64>>| -> Option<f64> {
        let v: f64 = b.iter().flatten().filter(|v| **v > 0.0).sum();
        (v > 0.0).then_some(v)
    };
    let slices: Vec<(usize, f64)> = item
        .bars
        .iter()
        .enumerate()
        .filter(|(i, _)| visible(input, *i))
        .filter_map(|(i, b)| cell_sum(b).or(o.value.null_numeric.filter(|v| *v > 0.0)).map(|v| (i, v)))
        .collect();
    let total: f64 = slices.iter().map(|s| s.1).sum();
    let mut meta = ItemMeta::new(input.width, input.height, plot);
    let mut summary = format!("합계 {}", short_number(total));
    if !input.compact {
        if let Some(l) = &input.legend {
            meta.legend = frame::draw_legend(c, t, l, plot);
        }
    }
    let cx = plot.x + plot.w / 2.0;
    let tilted = variant == Some(SubType::Pie3d);
    let k = if tilted { TILT } else { 1.0 };
    let r = if tilted {
        ((plot.w / 2.0 - 4.0).min((plot.h - 8.0) / (2.0 * TILT + DEPTH * 2.0))).max(4.0)
    } else {
        (plot.w.min(plot.h) / 2.0 - 4.0).max(4.0)
    };
    let depth = if tilted { r * DEPTH } else { 0.0 };
    let cy = plot.y + plot.h / 2.0 - depth / 2.0;
    if slices.is_empty() || total <= 0.0 {
        frame::draw_empty(c, t, plot);
        meta.summary = summary;
        return meta;
    }
    let hole = match variant {
        Some(SubType::Doughnut) => r * DOUGHNUT_HOLE,
        Some(SubType::Sunburst) => r * SUNBURST_HOLE,
        _ => 0.0,
    };
    let slice_outer = if variant == Some(SubType::Sunburst) { r * SUNBURST_RING } else { r };
    // 각도
    let mut a = 0.0f32;
    let laid: Vec<(usize, f64, f32, f32)> = slices
        .iter()
        .map(|&(i, v)| {
            let sweep = (v / total) as f32 * TAU;
            let out = (i, v, a, a + sweep);
            a += sweep;
            out
        })
        .collect();
    // pie3d 옆면: 앞쪽(아래 반원, 각도 π/2..3π/2) 조각의 바깥 가장자리를 depth만큼 내린 띠
    if tilted {
        for &(i, _, a0, a1) in &laid {
            let (s0, s1) = (a0.max(PI / 2.0), a1.min(PI * 1.5));
            if s1 <= s0 {
                continue;
            }
            let color = mix(series_color(input, i), 0x000000, 0.25);
            let steps = (((s1 - s0) / TAU * 96.0).ceil() as usize).max(2);
            let at = |ang: f32, dy: f32| (cx + r * ang.sin(), cy - r * k * ang.cos() + dy);
            let mut pts: Vec<(f32, f32)> = (0..=steps).map(|s| at(s0 + (s1 - s0) * s as f32 / steps as f32, 0.0)).collect();
            pts.extend((0..=steps).rev().map(|s| at(s0 + (s1 - s0) * s as f32 / steps as f32, depth)));
            if let Some(p) = polygon(&pts) {
                c.fill_path(&p, color, input.alpha);
            }
        }
    }
    let label_px = t.font(o.point_label.font_size, FONT);
    for &(i, v, a0, a1) in &laid {
        let color = series_color(input, i);
        let sweep = a1 - a0;
        if let Some(p) = sector(cx, cy, hole, slice_outer, a0, a1, k) {
            super::charts_category::fill_shape(c, t, &p, color, input.alpha);
            c.stroke_path(&p, t.surface, 1.0, 1.0, None);
        }
        let ratio = v / total * 100.0;
        if !input.compact && sweep > 0.25 {
            let mid = (a0 + a1) / 2.0;
            let rr = if hole > 0.0 {
                (hole + slice_outer) / 2.0
            } else {
                slice_outer * 0.62
            };
            let (lx, ly) = (cx + rr * mid.sin(), cy - rr * k * mid.cos());
            let ink = if luminance(color) > 0.6 { 0x0b0b0b } else { 0xffffff };
            let s = if o.point_label.visible {
                super::charts_points::label_text(&o.point_label, v)
            } else {
                format!("{ratio:.0}%")
            };
            c.text(&s, lx, ly + label_px * 0.36, label_px, ink, Align::Center);
        }
        meta.hits.push(Hit {
            shape: if tilted {
                HitShape::Polygon(
                    (0..=24)
                        .map(|s| {
                            let ang = a0 + sweep * s as f32 / 24.0;
                            (cx + r * ang.sin(), cy - r * k * ang.cos())
                        })
                        .chain(std::iter::once((cx, cy)))
                        .collect(),
                )
            } else {
                HitShape::Arc {
                    cx,
                    cy,
                    r0: hole,
                    r1: slice_outer,
                    a0,
                    a1,
                }
            },
            lines: vec![
                item.categories[i].clone(),
                format!("값 {}", short_number(v)),
                format!("비율 {ratio:.1}%"),
            ],
            color: Some(color),
        });
        // sunburst 바깥 고리: 조각 각도 안을 legend 값 몫으로 나눈다
        if variant == Some(SubType::Sunburst) {
            let parts: Vec<(usize, f64)> = item.bars[i]
                .iter()
                .enumerate()
                .filter_map(|(g, v)| v.filter(|v| *v > 0.0).map(|v| (g, v)))
                .collect();
            let sum: f64 = parts.iter().map(|p| p.1).sum();
            let mut b0 = a0;
            for (n, &(g, cv)) in parts.iter().enumerate() {
                let b1 = b0 + (cv / sum) as f32 * sweep;
                let shade = mix(color, 0xffffff, 0.15 + 0.5 * n as f32 / parts.len().max(1) as f32);
                if let Some(p) = sector(cx, cy, slice_outer + 1.0, r, b0, b1, 1.0) {
                    c.fill_path(&p, shade, input.alpha);
                    c.stroke_path(&p, t.surface, 1.0, 1.0, None);
                }
                let name = input.aux_labels.get(g).cloned().unwrap_or_default();
                if !input.compact && b1 - b0 > 0.35 {
                    let mid = (b0 + b1) / 2.0;
                    let rr = (slice_outer + r) / 2.0;
                    let s = text::truncate(&name, label_px - 1.0, (r - slice_outer) * 0.9);
                    c.text(
                        &s,
                        cx + rr * mid.sin(),
                        cy - rr * mid.cos() + 4.0,
                        label_px - 1.0,
                        0x0b0b0b,
                        Align::Center,
                    );
                }
                meta.hits.push(Hit {
                    shape: HitShape::Arc {
                        cx,
                        cy,
                        r0: slice_outer + 1.0,
                        r1: r,
                        a0: b0,
                        a1: b1,
                    },
                    lines: vec![
                        format!("{} › {name}", item.categories[i]),
                        format!("값 {}", short_number(cv)),
                        format!("조각 안 비율 {:.1}%", cv / sum * 100.0),
                    ],
                    color: Some(shade),
                });
                b0 = b1;
            }
        }
    }
    // doughnut 가운데: 보이는 조각 합계 (합계·개수일 때만 뜻이 있다)
    if variant == Some(SubType::Doughnut) && !input.compact && matches!(input.agg, Some(AggFn::Sum | AggFn::Count)) {
        c.text(
            &short_number(total),
            cx,
            cy + 6.0,
            t.font(o.title.font_size + 4.0, 16.0),
            t.text_primary,
            Align::Center,
        );
    }
    if tilted {
        summary.push_str(" · 3D");
    }
    meta.summary = summary;
    meta
}

pub fn render_radar(c: &mut Canvas, t: &Style, input: &ItemRender, item: &CategoryItem) -> ItemMeta {
    let plot = if input.compact {
        frame::compact_plot(input.width, input.height)
    } else {
        frame_plot(t, input.width, input.height, 0.0, input.legend.as_ref(), false)
    };
    let mut meta = ItemMeta::new(input.width, input.height, plot);
    let axes = item.categories.len();
    let summary = format!("축 {axes}개");
    if !input.compact {
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
            if t.o.shape.style != crate::options::ShapeStyle::Border {
                c.fill_path(&p, color, 0.18 * input.alpha.max(0.2));
            }
            c.stroke_path(&p, color, input.alpha, t.o.size.line_width, None);
        }
        for (k, &(x, y)) in pts.iter().enumerate() {
            let focused = t.o.focus.enabled && item.bars[k][g].is_some_and(|v| t.o.focus.hit(k as f64, v));
            if focused {
                c.circle(x, y, t.o.focus.size / 2.0, super::theme::color_or(&t.o.focus.color, 0xe34948), 1.0);
            } else {
                c.circle(x, y, if t.o.size.point_auto { 2.5 } else { t.o.size.point_fixed / 2.0 }, color, 1.0);
            }
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
