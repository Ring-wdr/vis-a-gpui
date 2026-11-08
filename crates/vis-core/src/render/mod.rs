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

use crate::query::candle::CandleItem;
use crate::query::category::CategoryItem;
use crate::query::gantt::GanttItem;
use crate::query::heatmap::HeatItem;
use crate::query::histogram::{Bins, HistItem};
use crate::query::points::PointsData;
use crate::query::sankey::SankeyItem;
use crate::spec::{AggFn, ChartTheme, ChartType, ItemView};

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
}

pub struct Rendered {
    pub canvas: Canvas,
    pub meta: ItemMeta,
}

/// 진행 단계 콜백 (aggregate → render)
pub type Progress<'a> = &'a (dyn Fn(f32) + Sync);

pub fn render(input: &ItemRender, progress: Progress) -> Rendered {
    let t = theme::theme(input.theme);
    let mut c = Canvas::new(input.width, input.height, input.scale);
    c.fill(t.surface);
    let meta = match &input.data {
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
    Rendered { canvas: c, meta }
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
