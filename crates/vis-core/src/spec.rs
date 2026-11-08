//! 차트 설정(spec). 원본 `ChartSpec`에서 화면이 고르는 값만 남겼다.

use serde::{Deserialize, Serialize};

use crate::dataset::ColumnKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartType {
    Scatter,
    Bar,
    Boxplot,
    Line,
    Sankey,
    Heatmap,
    Treemap,
    Candle,
    Gantt,
    Pie,
    Radar,
    Histogram,
    Waterfall,
    Funnel,
}

pub const CHART_TYPES: [ChartType; 14] = [
    ChartType::Scatter,
    ChartType::Line,
    ChartType::Bar,
    ChartType::Boxplot,
    ChartType::Histogram,
    ChartType::Heatmap,
    ChartType::Candle,
    ChartType::Gantt,
    ChartType::Pie,
    ChartType::Radar,
    ChartType::Treemap,
    ChartType::Waterfall,
    ChartType::Funnel,
    ChartType::Sankey,
];

use ColumnKind::*;

/// 차트 종류별 컬럼 역할 (원본 chart-fields.ts / chart-kinds.ts와 같은 표)
pub struct ChartFields {
    pub x: &'static [ColumnKind],
    pub y: Option<&'static [ColumnKind]>,
    pub z: Option<&'static [ColumnKind]>,
    pub agg: bool,
    pub legend: bool,
    /// 확대·이동
    pub zoom: bool,
}

impl ChartType {
    pub fn name(self) -> &'static str {
        match self {
            ChartType::Scatter => "scatter",
            ChartType::Bar => "bar",
            ChartType::Boxplot => "boxplot",
            ChartType::Line => "line",
            ChartType::Sankey => "sankey",
            ChartType::Heatmap => "heatmap",
            ChartType::Treemap => "treemap",
            ChartType::Candle => "candle",
            ChartType::Gantt => "gantt",
            ChartType::Pie => "pie",
            ChartType::Radar => "radar",
            ChartType::Histogram => "histogram",
            ChartType::Waterfall => "waterfall",
            ChartType::Funnel => "funnel",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ChartType::Scatter => "Scatter",
            ChartType::Bar => "Bar",
            ChartType::Boxplot => "Boxplot",
            ChartType::Line => "Line",
            ChartType::Sankey => "Sankey",
            ChartType::Heatmap => "Heatmap",
            ChartType::Treemap => "Treemap",
            ChartType::Candle => "Candle",
            ChartType::Gantt => "Gantt",
            ChartType::Pie => "Pie",
            ChartType::Radar => "Radar",
            ChartType::Histogram => "Histogram",
            ChartType::Waterfall => "Waterfall",
            ChartType::Funnel => "Funnel",
        }
    }

    pub fn fields(self) -> ChartFields {
        const DT_NUM: &[ColumnKind] = &[Datetime, Numeric];
        const CAT: &[ColumnKind] = &[Categorical];
        const NUM: &[ColumnKind] = &[Numeric];
        const DT: &[ColumnKind] = &[Datetime];
        const CAT_DT: &[ColumnKind] = &[Categorical, Datetime];
        let f = |x, y, z, agg, legend, zoom| ChartFields {
            x,
            y,
            z,
            agg,
            legend,
            zoom,
        };
        match self {
            ChartType::Scatter => f(DT_NUM, Some(NUM), None, false, true, true),
            ChartType::Line => f(DT_NUM, Some(NUM), None, false, true, true),
            ChartType::Bar => f(CAT, Some(NUM), None, true, true, true),
            ChartType::Boxplot => f(CAT, Some(NUM), None, false, true, true),
            ChartType::Sankey => f(CAT, Some(CAT), Some(NUM), true, false, false),
            ChartType::Heatmap => f(CAT_DT, Some(CAT), Some(NUM), true, false, false),
            ChartType::Treemap => f(CAT, Some(NUM), None, true, true, false),
            ChartType::Candle => f(DT, Some(NUM), None, false, false, true),
            ChartType::Gantt => f(DT, Some(CAT), None, false, true, true),
            ChartType::Pie => f(CAT, Some(NUM), None, true, false, false),
            ChartType::Radar => f(CAT, Some(NUM), None, true, true, false),
            ChartType::Histogram => f(NUM, None, None, false, true, true),
            ChartType::Waterfall => f(CAT, Some(NUM), None, true, false, true),
            ChartType::Funnel => f(CAT, Some(NUM), None, true, false, false),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AggFn {
    #[default]
    Avg,
    Sum,
    Count,
    Min,
    Max,
    Median,
}

pub const AGG_FNS: [AggFn; 6] = [AggFn::Avg, AggFn::Sum, AggFn::Count, AggFn::Min, AggFn::Max, AggFn::Median];

impl AggFn {
    pub fn label(self) -> &'static str {
        match self {
            AggFn::Avg => "평균",
            AggFn::Sum => "합계",
            AggFn::Count => "개수",
            AggFn::Min => "최솟값",
            AggFn::Max => "최댓값",
            AggFn::Median => "중앙값",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Filter {
    /// 숫자·시계열: [min, max] 안(inverse면 밖). include_null이면 값 없음도 남긴다.
    Range {
        column: String,
        min: f64,
        max: f64,
        inverse: bool,
        include_null: bool,
    },
    /// 범주: 고른 값을 뺀다
    Exclude { column: String, values: Vec<String> },
}

impl Filter {
    pub fn column(&self) -> &str {
        match self {
            Filter::Range { column, .. } | Filter::Exclude { column, .. } => column,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LegendPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommonProps {
    /// legend가 없을 때 점·막대 색
    pub color: String,
    pub legend_position: LegendPosition,
    /// 0이면 불투명
    pub transparency: f32,
    /// legend 값별 색 (없으면 팔레트 순서)
    #[serde(default)]
    pub legend_colors: std::collections::BTreeMap<String, String>,
}

impl Default for CommonProps {
    fn default() -> Self {
        Self {
            color: crate::render::theme::PALETTE[0].to_string(),
            legend_position: LegendPosition::Top,
            transparency: 0.0,
            legend_colors: Default::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChartSpec {
    pub dataset: String,
    pub chart_type: ChartType,
    pub x: String,
    pub y: String,
    #[serde(default)]
    pub z: Option<String>,
    pub agg: AggFn,
    pub legend: Option<String>,
    pub split_by: Vec<String>,
    pub split_separator: String,
    #[serde(default)]
    pub tooltip: Vec<String>,
    pub grid: (u32, u32),
    #[serde(default)]
    pub common: CommonProps,
    #[serde(default)]
    pub filters: Vec<Filter>,
}

impl ChartSpec {
    pub fn summary(&self) -> String {
        let mut s = format!("{} · {}", self.chart_type.label(), self.x);
        if self.chart_type.fields().y.is_some() {
            s.push_str(&format!(" × {}", self.y));
        }
        if !self.split_by.is_empty() {
            s.push_str(&format!(" · split {}", self.split_by.join(",")));
        }
        s
    }
}

/// item 하나의 확대 범위 (데이터 좌표)
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemView {
    pub x: (f64, f64),
    pub y: (f64, f64),
}

/// item 개별 상태. split 조합 키에 붙는다.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ItemState {
    pub view: Option<ItemView>,
    pub hidden_legend: Vec<String>,
    /// item 개별 속성: 색 · legend 값별 색 · legend 위치. 공통 속성보다 우선한다
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub legend_colors: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub legend_position: Option<LegendPosition>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ChartTheme {
    #[default]
    Light,
    Dark,
}
