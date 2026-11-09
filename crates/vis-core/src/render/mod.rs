//! 차트 렌더러. 원본 server/render/*.ts (OffscreenCanvas 2D)를 tiny-skia 래스터로 옮겼다.
//! 렌더 결과는 PNG 대신 화면에 바로 올릴 BGRA 비트맵과 metadata다 (원하면 PNG로도 인코딩한다).

pub mod canvas;
pub mod frame;
pub mod meta;
pub mod scale;
pub mod text;
pub mod theme;

mod charts_category;
mod charts_misc;
mod charts_points;
mod charts_polar;

use canvas::Canvas;
use frame::LegendInput;
use meta::ItemMeta;

use crate::options::ChartOptions;
use crate::query::candle::CandleItem;
use crate::query::category::CategoryItem;
use crate::query::gantt::GanttItem;
use crate::query::heatmap::HeatItem;
use crate::query::histogram::{Bins, HistItem};
use crate::query::points::PointsData;
use crate::query::sankey::SankeyItem;
use crate::region::{Region, Shape};
use crate::spec::{AggFn, ChartTheme, ChartType, ItemView, SubType};
use meta::{AxisKind, AxisMeta, HitShape};
use theme::Style;

pub enum ItemData<'a> {
    Points { line: bool, x_time: bool, data: &'a PointsData },
    Category { item: &'a CategoryItem, agg: AggFn },
    Histogram { bins: Bins, item: &'a HistItem },
    Heatmap(&'a HeatItem),
    Candle(&'a CandleItem),
    Gantt(&'a GanttItem),
    Sankey(&'a SankeyItem),
}

pub struct ItemRender<'a> {
    pub chart: ChartType,
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub theme: ChartTheme,
    /// 섬네일: 축·제목·legend 없이 그림 영역만
    pub compact: bool,
    pub title: String,
    pub color: u32,
    pub alpha: f32,
    pub legend: Option<LegendInput>,
    pub view: Option<ItemView>,
    pub x_title: String,
    pub y_title: String,
    pub data: ItemData<'a>,
    /// 합친 차트 속성 (기본 → 테마 → 차트 종류 → 공통 → 개별)
    pub options: ChartOptions,
    pub sub_type: Option<SubType>,
    pub selection: Option<Region>,
    pub shapes: Vec<Shape>,
    /// 보조 이름 (pie sunburst 바깥 고리의 legend 값)
    pub aux_labels: Vec<String>,
    /// 통계 함수 (doughnut 가운데 합계)
    pub agg: Option<AggFn>,
}

pub struct Rendered {
    pub canvas: Canvas,
    pub meta: ItemMeta,
}

/// 진행 단계 콜백 (aggregate → render)
pub type Progress<'a> = &'a (dyn Fn(f32) + Sync);

pub fn render(input: &ItemRender, progress: Progress) -> Rendered {
    let t = Style::new(input.theme, input.options.clone(), (input.width, input.height), input.compact);
    let mut c = Canvas::new(input.width, input.height, input.scale);
    frame::draw_panel(&mut c, &t);
    let mut meta = match &input.data {
        ItemData::Points { line, x_time, data } => charts_points::render_points(&mut c, &t, input, *line, *x_time, data, progress),
        ItemData::Category { item, agg } => match input.chart {
            ChartType::Boxplot => charts_category::render_boxplot(&mut c, &t, input, item),
            ChartType::Waterfall => charts_category::render_waterfall(&mut c, &t, input, item),
            ChartType::Pie => charts_polar::render_pie(&mut c, &t, input, item),
            ChartType::Radar => charts_polar::render_radar(&mut c, &t, input, item),
            ChartType::Treemap => charts_misc::render_treemap(&mut c, &t, input, item),
            ChartType::Funnel => charts_misc::render_funnel(&mut c, &t, input, item),
            _ => charts_category::render_bar(&mut c, &t, input, item, *agg),
        },
        ItemData::Histogram { bins, item } => charts_category::render_histogram(&mut c, &t, input, *bins, item),
        ItemData::Heatmap(item) => charts_misc::render_heatmap(&mut c, &t, input, item),
        ItemData::Candle(item) => charts_misc::render_candle(&mut c, &t, input, item),
        ItemData::Gantt(item) => charts_misc::render_gantt(&mut c, &t, input, item),
        ItemData::Sankey(item) => charts_misc::render_sankey(&mut c, &t, input, item),
    };
    let plot = meta.plot;
    // 축이 없는 차트: 그림 영역 픽셀과 1:1인 고정 좌표 (원본 polar.ts fixedSpace)
    if meta.x.is_none() || meta.y.is_none() {
        meta.x = Some(AxisMeta {
            kind: AxisKind::Number,
            domain: (0.0, plot.w as f64),
            range: (plot.x, plot.right()),
        });
        meta.y = Some(AxisMeta {
            kind: AxisKind::Number,
            domain: (0.0, plot.h as f64),
            range: (plot.bottom(), plot.y),
        });
    }
    let (xa, ya) = (meta.x.unwrap(), meta.y.unwrap());
    let px = |v: f64| xa.map(v);
    let py = |v: f64| ya.map(v);
    c.set_clip(Some((plot.x, plot.y, plot.w, plot.h)));
    let mut summary = meta.summary.clone();
    if let Some(sel) = &input.selection {
        let dim = 1.0 - t.o.shape.dim_opacity.clamp(0.0, 1.0);
        let poly = sel.project(&px, &py);
        if meta.points.is_some() {
            // 점 차트: 다 그린 뒤 선택 밖을 배경색으로 한 번 덮는다 (even-odd)
            c.fill_outside((plot.x, plot.y, plot.w, plot.h), &poly, t.surface, dim);
        } else {
            // 나머지: 대표 좌표가 영역 밖인 도형을 흐리게
            let test = sel.tester();
            let mut n = 0;
            for (i, h) in meta.hits.iter().enumerate() {
                let (ax, ay) = meta
                    .anchors
                    .iter()
                    .find(|a| a.0 == i)
                    .map(|a| a.1)
                    .unwrap_or_else(|| h.shape.anchor());
                if test(xa.invert(ax), ya.invert(ay)) {
                    n += 1;
                    continue;
                }
                if let Some(p) = hit_path(&h.shape) {
                    c.fill_path(&p, t.surface, dim);
                }
            }
            meta.selected.get_or_insert(n);
        }
        if let Some(p) = canvas::polygon(&poly) {
            c.fill_path(&p, t.selection, 0.08);
            c.stroke_path(&p, t.selection, 1.0, 1.5, Some(&[4.0, 3.0]));
        }
        if let Some(n) = meta.selected {
            summary = format!("{summary} · 선택 {}", charts_fmt_int(n));
        }
    }
    if !input.shapes.is_empty() && !input.compact {
        draw_shapes(&mut c, &t, &input.shapes, &px, &py);
    }
    c.set_clip(None);
    if !input.compact {
        frame::draw_title(&mut c, &t, &input.title, &summary);
    }
    frame::draw_panel_border(&mut c, &t);
    meta.summary = summary;
    meta.tooltip = t.o.tooltip.clone();
    Rendered { canvas: c, meta }
}

/// 가리킬 수 있는 도형의 경로 (선택 밖 흐리게)
fn hit_path(s: &HitShape) -> Option<tiny_skia::Path> {
    match s {
        HitShape::Rect(r) => tiny_skia::Rect::from_xywh(r.x, r.y, r.w.max(0.5), r.h.max(0.5)).map(tiny_skia::PathBuilder::from_rect),
        HitShape::Circle { cx, cy, r } => tiny_skia::PathBuilder::from_circle(*cx, *cy, *r),
        HitShape::Polygon(p) => canvas::polygon(p),
        HitShape::Arc { cx, cy, r0, r1, a0, a1 } => {
            let steps = (((a1 - a0) / 0.05).ceil() as usize).max(2);
            let at = |r: f32, a: f32| (cx + r * a.sin(), cy - r * a.cos());
            let mut pts: Vec<(f32, f32)> = (0..=steps).map(|k| at(*r1, a0 + (a1 - a0) * k as f32 / steps as f32)).collect();
            pts.extend((0..=steps).rev().map(|k| at(*r0, a0 + (a1 - a0) * k as f32 / steps as f32)));
            canvas::polygon(&pts)
        }
    }
}

/// 도형 (원본 shapes.ts): 진한 글자 색 2px 선, 사각형은 옅게 채움, 텍스트는 배경색 테두리
fn draw_shapes(c: &mut Canvas, t: &Style, shapes: &[Shape], px: &dyn Fn(f64) -> f32, py: &dyn Fn(f64) -> f32) {
    let ink = t.text_primary;
    for s in shapes {
        match s {
            Shape::Rect { x0, y0, x1, y1 } => {
                let (l, r) = (px(*x0).min(px(*x1)), px(*x0).max(px(*x1)));
                let (tp, b) = (py(*y0).min(py(*y1)), py(*y0).max(py(*y1)));
                c.fill_rect(l, tp, r - l, b - tp, ink, 0.08);
                c.stroke_rect(l, tp, r - l, b - tp, ink, 2.0);
            }
            Shape::Line { x0, y0, x1, y1 } => c.line(px(*x0), py(*y0), px(*x1), py(*y1), ink, 2.0),
            Shape::Freehand { points } => {
                let pts: Vec<(f32, f32)> = points.iter().map(|&(x, y)| (px(x), py(y))).collect();
                if let Some(p) = canvas::polyline(&pts) {
                    c.stroke_path(&p, ink, 1.0, 2.0, None);
                }
            }
            Shape::Text { x, y, text } => {
                let (x, y) = (px(*x), py(*y));
                let size = 12.0;
                // 점 위에서도 읽히도록 배경색 테두리를 먼저
                for (dx, dy) in [
                    (-1.5, 0.0),
                    (1.5, 0.0),
                    (0.0, -1.5),
                    (0.0, 1.5),
                    (-1.0, -1.0),
                    (1.0, 1.0),
                    (-1.0, 1.0),
                    (1.0, -1.0),
                ] {
                    c.text(text, x + dx, y + 4.0 + dy, size, t.surface, canvas::Align::Left);
                }
                c.text_bold(text, x, y + 4.0, size, ink, canvas::Align::Left);
            }
        }
    }
}

/// 숨긴 legend 값이 아니면 true
pub(crate) fn visible(input: &ItemRender, g: usize) -> bool {
    input.legend.as_ref().is_none_or(|l| !l.is_hidden(g))
}

pub(crate) fn series_color(input: &ItemRender, g: usize) -> u32 {
    input.legend.as_ref().and_then(|l| l.colors.get(g).copied()).unwrap_or(input.color)
}

pub(crate) fn legend_name(input: &ItemRender, g: usize) -> Option<String> {
    input.legend.as_ref().and_then(|l| l.values.get(g).cloned())
}

/// 천 단위 쉼표 숫자
pub fn charts_fmt_int(n: usize) -> String {
    charts_points::fmt_int(n)
}
