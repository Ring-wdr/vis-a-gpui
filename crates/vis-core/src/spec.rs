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

/// 하위 차트 (원본 chart-fields.ts SUB_TYPES). 차트마다 첫 값이 기본이다
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubType {
    None,
    Ecdf,
    Ncdf,
    Probability,
    Side,
    Stack,
    Stack100,
    Dot,
    Doughnut,
    Pie3d,
    Sunburst,
}

impl SubType {
    pub fn label(self) -> &'static str {
        match self {
            SubType::None => "기본",
            SubType::Ecdf => "ECDF (누적 비율)",
            SubType::Ncdf => "정규 CDF",
            SubType::Probability => "정규 확률",
            SubType::Side => "나란히",
            SubType::Stack => "쌓기",
            SubType::Stack100 => "100% 쌓기",
            SubType::Dot => "점 겹치기",
            SubType::Doughnut => "도넛",
            SubType::Pie3d => "3D",
            SubType::Sunburst => "선버스트",
        }
    }
}

impl ChartType {
    /// 하위 차트 목록. 첫 값이 기본이고, 빈 목록이면 하위 차트가 없다
    pub fn sub_types(self) -> &'static [SubType] {
        use SubType::*;
        match self {
            ChartType::Line => &[None, Ecdf, Ncdf, Probability],
            ChartType::Bar => &[Side, Stack, Stack100],
            ChartType::Boxplot => &[None, Dot],
            ChartType::Pie => &[None, Doughnut, Pie3d, Sunburst],
            _ => &[],
        }
    }

    /// 확대·이동 (원본 차트별 상단 도구 줄 표)
    pub fn can_zoom(self) -> bool {
        !matches!(self, ChartType::Sankey | ChartType::Pie | ChartType::Radar | ChartType::Funnel)
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
    /// 차트 속성 부분 값 (options.rs, JSON 객체)
    #[serde(default)]
    pub options: serde_json::Value,
}

impl Default for CommonProps {
    fn default() -> Self {
        Self {
            color: crate::render::theme::PALETTE[0].to_string(),
            legend_position: LegendPosition::Top,
            transparency: 0.0,
            legend_colors: Default::default(),
            options: serde_json::Value::Null,
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
    /// 하위 차트. 없거나 그 차트에 없는 값이면 기본
    #[serde(default)]
    pub sub_type: Option<SubType>,
    /// 툴팁: 가리킨 자리에서 가장 가까운 점을 표시 (전체 설정 › 툴팁)
    #[serde(default = "yes")]
    pub show_nearest: bool,
}

fn yes() -> bool {
    true
}

impl ChartSpec {
    /// 그릴 하위 차트 (없는 차트면 None)
    pub fn sub_type(&self) -> Option<SubType> {
        let list = self.chart_type.sub_types();
        let first = *list.first()?;
        Some(self.sub_type.filter(|s| list.contains(s)).unwrap_or(first))
    }
    /// line의 분포 하위 차트 (점 좌표가 원래 컬럼 값이 아니다)
    pub fn distribution(&self) -> Option<crate::analytics::Distribution> {
        use crate::analytics::Distribution;
        match (self.chart_type, self.sub_type()) {
            (ChartType::Line, Some(SubType::Ecdf)) => Some(Distribution::Ecdf),
            (ChartType::Line, Some(SubType::Ncdf)) => Some(Distribution::Ncdf),
            (ChartType::Line, Some(SubType::Probability)) => Some(Distribution::Probability),
            _ => None,
        }
    }
    /// 사각형·자유형 삭제를 할 수 있는지 (scatter, 분포가 아닌 line)
    pub fn can_delete(&self) -> bool {
        matches!(self.chart_type, ChartType::Scatter | ChartType::Line) && self.distribution().is_none()
    }
    /// 선택 영역으로 데이터 표·다운로드의 행을 거르는 차트
    pub fn filters_rows_by_selection(&self) -> bool {
        matches!(
            self.chart_type,
            ChartType::Scatter | ChartType::Bar | ChartType::Boxplot | ChartType::Line | ChartType::Histogram | ChartType::Waterfall
        ) && self.distribution().is_none()
    }
    /// 그릴 때 쓰는 legend 컬럼 (pie sunburst는 legend 컬럼을 바깥 고리로 쓴다)
    pub fn legend_column(&self) -> Option<&str> {
        if self.chart_type == ChartType::Pie && self.sub_type() == Some(SubType::Sunburst) {
            return self.legend.as_deref();
        }
        if self.chart_type.fields().legend {
            self.legend.as_deref()
        } else {
            None
        }
    }

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
    /// item 개별 차트 속성 부분 값 (options.rs)
    #[serde(default)]
    pub options: serde_json::Value,
    /// 도형 (데이터 좌표)
    #[serde(default)]
    pub shapes: Vec<crate::region::Shape>,
    /// 선택 영역 (item마다 하나)
    #[serde(default)]
    pub selection: Option<crate::region::Region>,
    /// 즐겨찾기 (이름 붙여 남긴 영역)
    #[serde(default)]
    pub favorites: Vec<crate::region::NamedRegion>,
    /// 새 범례: 영역 안의 점을 새 legend 항목으로 칠한다
    #[serde(default)]
    pub legend_groups: Vec<crate::region::NamedRegion>,
    /// 지운 영역 (scatter, line)
    #[serde(default)]
    pub deletions: Vec<crate::region::Region>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ChartTheme {
    #[default]
    Light,
    Dark,
}

/// 툴팁에 더 보여줄 컬럼 (vis-a-lot `features/item-tooltip/lib/tooltip-columns.ts`의 liveTooltipColumns).
/// 왼쪽 패널의 tooltip 선택(draft)을 draw 없이 바로 따른다. 값은 가리킨 행 번호로 읽으므로 다시 그릴 필요가 없다.
/// 패널에서 데이터셋을 바꿨으면 그린 차트의 데이터셋에 없는 컬럼은 읽을 수 없어 뺀다.
/// 그린 차트의 x, y는 이미 툴팁 줄로 보이므로 뺀다. 그린 데이터셋의 컬럼을 모르면 아무것도 고르지 않는다.
pub fn live_tooltip_columns(selected: &[String], drawn_columns: Option<&[String]>, drawn: Option<(&str, &str)>) -> Vec<String> {
    let Some(names) = drawn_columns else { return vec![] };
    selected
        .iter()
        .filter(|c| names.contains(c) && drawn.is_none_or(|(x, y)| c.as_str() != x && c.as_str() != y))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tooltip_tests {
    use super::live_tooltip_columns;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn keeps_selected_order_and_drawn_dataset_columns() {
        let cols = s(&["a", "b", "c", "d"]);
        assert_eq!(live_tooltip_columns(&s(&["c", "zz", "a"]), Some(&cols), None), s(&["c", "a"]));
    }

    #[test]
    fn drops_drawn_x_y() {
        let cols = s(&["a", "b", "c", "d"]);
        assert_eq!(live_tooltip_columns(&s(&["a", "b", "c"]), Some(&cols), Some(("a", "b"))), s(&["c"]));
    }

    #[test]
    fn nothing_before_schema() {
        assert!(live_tooltip_columns(&s(&["a"]), None, None).is_empty());
    }
}
