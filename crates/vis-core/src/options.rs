//! 차트 속성(옵션). 원본 server/render/options.ts · server/charts/chart-options.ts · chart-defaults.ts.
//! 공통 속성·item 개별 속성은 이 모양의 부분 값(JSON 객체, ChartOptionsPatch)을 저장하고,
//! 그릴 때 기본값 → 테마 → 차트 종류별 기본값 → 공통 → 개별 순으로 합친 전체 값(ChartOptions)을 렌더러에 넘긴다.
//! 차트 종류마다 쓸 수 있는 구역은 OPTION_SUPPORT 표가 정한다. 못 쓰는 구역은 기본값으로 온다.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::spec::{ChartTheme, ChartType, LegendPosition};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OptionSection {
    Title,
    Axes,
    Legend,
    Panel,
    Palette,
    Shape,
    Size,
    PointLabel,
    Score,
    Prediction,
    Value,
    Focus,
    Tooltip,
    Expression,
}

pub const OPTION_SECTIONS: [OptionSection; 14] = [
    OptionSection::Title,
    OptionSection::Axes,
    OptionSection::Legend,
    OptionSection::Panel,
    OptionSection::Palette,
    OptionSection::Shape,
    OptionSection::Size,
    OptionSection::PointLabel,
    OptionSection::Score,
    OptionSection::Prediction,
    OptionSection::Value,
    OptionSection::Focus,
    OptionSection::Tooltip,
    OptionSection::Expression,
];

impl OptionSection {
    pub fn key(self) -> &'static str {
        match self {
            OptionSection::Title => "title",
            OptionSection::Axes => "axes",
            OptionSection::Legend => "legend",
            OptionSection::Panel => "panel",
            OptionSection::Palette => "palette",
            OptionSection::Shape => "shape",
            OptionSection::Size => "size",
            OptionSection::PointLabel => "pointLabel",
            OptionSection::Score => "score",
            OptionSection::Prediction => "prediction",
            OptionSection::Value => "value",
            OptionSection::Focus => "focus",
            OptionSection::Tooltip => "tooltip",
            OptionSection::Expression => "expression",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            OptionSection::Title => "제목",
            OptionSection::Axes => "축",
            OptionSection::Legend => "범례",
            OptionSection::Panel => "패널",
            OptionSection::Palette => "팔레트",
            OptionSection::Shape => "모양",
            OptionSection::Size => "크기",
            OptionSection::PointLabel => "점 라벨",
            OptionSection::Score => "점수",
            OptionSection::Prediction => "예측",
            OptionSection::Value => "값",
            OptionSection::Focus => "강조",
            OptionSection::Tooltip => "툴팁",
            OptionSection::Expression => "식",
        }
    }
    pub fn hint(self) -> Option<&'static str> {
        Some(match self {
            OptionSection::Title => "글자 크기는 크기 › 글자 크기 자동을 끈 때만 씁니다.",
            OptionSection::Axes => "글자 크기는 크기 › 글자 크기 자동을 끈 때만 씁니다. 로그 축은 양수 값 축에서만 적용됩니다.",
            OptionSection::Score => {
                "상관계수: x·y 상관계수, 분산분석 F: 범례 그룹 간 일원 분산분석 F, 규격 내 확률: 값이 하한~상한 안에 들 확률, 공정 능력: Cpk. 이 둘은 상한·하한이 있어야 계산합니다."
            }
            OptionSection::Value => "빈 칸 값은 그 조합에 데이터가 없는 칸에 씁니다.",
            OptionSection::Focus => "조건에 맞는 점(막대)을 강조합니다.",
            OptionSection::Expression => {
                "변수 x, y와 + - * / ^, 함수 abs log ln sqrt exp round floor ceil min max pow를 씁니다. 예: log(y), x * 2 + 1. 비우면 원래 값입니다."
            }
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub visible: bool,
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TitleOptions {
    pub visible: bool,
    /// 비우면 split 조합 이름(item label)
    pub text: String,
    pub subtitle: String,
    /// size.fontAuto가 꺼져 있을 때만 쓴다
    pub font_size: f32,
    pub bold: bool,
    pub color: String,
    pub align: TextAlign,
    pub subtitle_color: String,
    pub summary: Summary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AxisTitle {
    pub visible: bool,
    pub text: String,
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineStyle {
    pub visible: bool,
    pub width: f32,
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TickStyle {
    pub visible: bool,
    pub count: u32,
    pub length: f32,
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelStyle {
    pub visible: bool,
    /// 도 단위
    pub angle: f32,
    pub font_size: f32,
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AxisOptions {
    pub visible: bool,
    pub title: AxisTitle,
    /// 값 축 범위. None이면 자동
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// 로그 축. 양수 값 축에서만 쓴다
    pub log: bool,
    pub line: LineStyle,
    pub grid: LineStyle,
    pub tick: TickStyle,
    pub label: LabelStyle,
    /// 숫자 눈금의 소수 자리. None이면 자동
    pub decimals: Option<u32>,
    /// 시계열 눈금 형식(strftime). 비우면 범위에 맞춰 자동
    pub date_format: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Axes {
    pub x: AxisOptions,
    pub y: AxisOptions,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegendStyle {
    pub visible: bool,
    pub border: bool,
    pub border_color: String,
    pub reverse: bool,
    pub font_size: f32,
    pub font_color: String,
    pub title_color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Margin {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelOptions {
    pub back_color: String,
    pub gradient: bool,
    pub border: bool,
    pub border_color: String,
    pub margin: Margin,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaletteOptions {
    pub gradient: bool,
    pub start: String,
    pub middle: String,
    pub end: String,
    /// 단계 수. 0이면 legend 값 수
    pub steps: u32,
    pub reverse: bool,
    pub others_color: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PointShape {
    Circle,
    Square,
    Triangle,
    Diamond,
    Cross,
}

pub const POINT_SHAPES: [PointShape; 5] = [
    PointShape::Circle,
    PointShape::Square,
    PointShape::Triangle,
    PointShape::Diamond,
    PointShape::Cross,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShapeStyle {
    Both,
    Border,
    Brush,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeOptions {
    #[serde(rename = "type")]
    pub kind: PointShape,
    pub style: ShapeStyle,
    /// 0(불투명)..1(투명)
    pub transparency: f32,
    pub border_width: f32,
    /// 비우면 자동
    pub border_color: String,
    /// 선택 영역 밖 도형의 진하기 (0..1)
    pub dim_opacity: f32,
    pub corner_radius: f32,
    pub median_color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SizeOptions {
    pub point_auto: bool,
    pub point_fixed: f32,
    pub point_min: f32,
    pub point_max: f32,
    pub font_auto: bool,
    pub font_min: f32,
    pub font_max: f32,
    pub line_width: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LabelPosition {
    Top,
    Center,
    Bottom,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelBox {
    pub visible: bool,
    pub background: String,
    pub border: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointLabelOptions {
    pub visible: bool,
    pub position: LabelPosition,
    pub angle: f32,
    pub font_size: f32,
    pub color: String,
    pub prefix: String,
    pub suffix: String,
    pub decimals: Option<u32>,
    #[serde(rename = "box")]
    pub label_box: LabelBox,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScoreKind {
    Rscore,
    Fscore,
    Zscore,
    Cpk,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreOptions {
    pub visible: bool,
    pub kinds: Vec<ScoreKind>,
    pub ucl: Option<f64>,
    pub lcl: Option<f64>,
    pub font_size: f32,
    pub font_color: String,
    pub back_color: String,
    pub border_color: String,
    pub border_width: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PredictionMethod {
    #[serde(rename = "sma")]
    Sma,
    #[serde(rename = "ses")]
    Ses,
    #[serde(rename = "des")]
    Des,
    #[serde(rename = "holtWinters")]
    HoltWinters,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Combine {
    Additive,
    Multiplicative,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictionOptions {
    pub enabled: bool,
    pub method: PredictionMethod,
    pub period: u32,
    pub show_interval: bool,
    pub description: bool,
    pub window: u32,
    pub alpha: Option<f64>,
    pub beta: Option<f64>,
    pub gamma: Option<f64>,
    pub season_length: u32,
    pub trend: Combine,
    pub seasonal: Combine,
    pub damped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutlierMethod {
    Iqr,
    Sigma,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueOptions {
    pub null_numeric: Option<f64>,
    pub outlier: bool,
    pub outlier_method: OutlierMethod,
    pub outlier_k: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FocusOperator {
    #[serde(rename = ">")]
    Gt,
    #[serde(rename = ">=")]
    Ge,
    #[serde(rename = "<")]
    Lt,
    #[serde(rename = "<=")]
    Le,
    #[serde(rename = "=")]
    Eq,
    #[serde(rename = "!=")]
    Ne,
}

impl FocusOperator {
    pub fn test(self, v: f64, target: f64) -> bool {
        match self {
            FocusOperator::Gt => v > target,
            FocusOperator::Ge => v >= target,
            FocusOperator::Lt => v < target,
            FocusOperator::Le => v <= target,
            FocusOperator::Eq => v == target,
            FocusOperator::Ne => v != target,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusOptions {
    pub enabled: bool,
    pub target: Axis,
    pub operator: FocusOperator,
    pub value: f64,
    pub color: String,
    pub shape: PointShape,
    pub size: f32,
    pub legend_single_color: bool,
}

impl FocusOptions {
    /// 점 (x, y)가 강조 조건에 맞는지
    pub fn hit(&self, x: f64, y: f64) -> bool {
        self.enabled && self.operator.test(if self.target == Axis::X { x } else { y }, self.value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TooltipOptions {
    pub title: String,
    pub font_size: f32,
    pub font_color: String,
    pub background: String,
    pub transparency: f32,
    pub round: bool,
    pub border: bool,
    pub border_color: String,
    pub prefix: String,
    pub suffix: String,
    pub decimals: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpressionOptions {
    pub x: String,
    pub y: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartOptions {
    pub title: TitleOptions,
    pub axes: Axes,
    pub legend: LegendStyle,
    pub panel: PanelOptions,
    pub palette: PaletteOptions,
    pub shape: ShapeOptions,
    pub size: SizeOptions,
    pub point_label: PointLabelOptions,
    pub score: ScoreOptions,
    pub prediction: PredictionOptions,
    pub value: ValueOptions,
    pub focus: FocusOptions,
    pub tooltip: TooltipOptions,
    pub expression: ExpressionOptions,
}

/// 원본 DEFAULT_OPTIONS (JSON). 옵션을 하나도 정하지 않으면 지금까지 그리던 모습과 같다.
pub fn default_json() -> Value {
    let axis = |grid: bool, line: bool| {
        json!({
            "visible": true,
            "title": { "visible": false, "text": "", "color": "" },
            "min": null, "max": null, "log": false,
            "line": { "visible": line, "width": 1, "color": "#c9c8c2" },
            "grid": { "visible": grid, "width": 1, "color": "#e7e6e2" },
            "tick": { "visible": true, "count": 5, "length": 0, "color": "#c9c8c2" },
            "label": { "visible": true, "angle": 0, "fontSize": 11, "color": "#52514e" },
            "decimals": null,
            "dateFormat": ""
        })
    };
    json!({
        "title": {
            "visible": true, "text": "", "subtitle": "", "fontSize": 12, "bold": true, "color": "#0b0b0b",
            "align": "left", "subtitleColor": "#52514e", "summary": { "visible": true, "color": "#8a8984" }
        },
        "axes": { "x": axis(false, true), "y": axis(true, false) },
        "legend": {
            "visible": true, "border": true, "borderColor": "#e7e6e2", "reverse": false, "fontSize": 11,
            "fontColor": "#52514e", "titleColor": "#8a8984"
        },
        "panel": {
            "backColor": "#fcfcfb", "gradient": false, "border": false, "borderColor": "#c9c8c2",
            "margin": { "top": 0, "right": 0, "bottom": 0, "left": 0 }
        },
        "palette": {
            "gradient": false, "start": "#cfe3fa", "middle": "", "end": "#0d3d78", "steps": 0, "reverse": false,
            "othersColor": "#a9a8a2"
        },
        "shape": {
            "type": "circle", "style": "both", "transparency": 0, "borderWidth": 0, "borderColor": "",
            "dimOpacity": 0.2, "cornerRadius": 4, "medianColor": "#0b0b0b"
        },
        "size": {
            "pointAuto": true, "pointFixed": 4, "pointMin": 1, "pointMax": 12, "fontAuto": true, "fontMin": 9,
            "fontMax": 16, "lineWidth": 1.5
        },
        "pointLabel": {
            "visible": false, "position": "top", "angle": 0, "fontSize": 10, "color": "#52514e", "prefix": "",
            "suffix": "", "decimals": null, "box": { "visible": false, "background": "#ffffff", "border": "#c9c8c2" }
        },
        "score": {
            "visible": false, "kinds": ["rscore"], "ucl": null, "lcl": null, "fontSize": 11, "fontColor": "#0b0b0b",
            "backColor": "#ffffff", "borderColor": "#c9c8c2", "borderWidth": 1
        },
        "prediction": {
            "enabled": false, "method": "sma", "period": 10, "showInterval": false, "description": true, "window": 5,
            "alpha": null, "beta": null, "gamma": null, "seasonLength": 12, "trend": "additive",
            "seasonal": "additive", "damped": false
        },
        "value": { "nullNumeric": null, "outlier": false, "outlierMethod": "iqr", "outlierK": 1.5 },
        "focus": {
            "enabled": false, "target": "y", "operator": ">", "value": 0, "color": "#e34948", "shape": "circle",
            "size": 6, "legendSingleColor": false
        },
        "tooltip": {
            "title": "", "fontSize": 12, "fontColor": "#0b0b0b", "background": "#ffffff", "transparency": 0,
            "round": true, "border": true, "borderColor": "#c9c8c2", "prefix": "", "suffix": "", "decimals": null
        },
        "expression": { "x": "", "y": "" }
    })
}

/// 다크 테마에서 기본값 위에 덮는 값 (글자와 바탕 계열만)
pub fn dark_json() -> Value {
    let axis = json!({
        "line": { "color": "#55544f" }, "grid": { "color": "#363632" }, "tick": { "color": "#55544f" },
        "label": { "color": "#b5b4ae" }
    });
    json!({
        "title": { "color": "#ecebe8", "subtitleColor": "#b5b4ae", "summary": { "color": "#8a8984" } },
        "axes": { "x": axis.clone(), "y": axis },
        "legend": { "borderColor": "#363632", "fontColor": "#b5b4ae", "titleColor": "#8a8984" },
        "panel": { "backColor": "#141413", "borderColor": "#55544f" },
        "shape": { "medianColor": "#ecebe8" },
        "pointLabel": { "color": "#b5b4ae", "box": { "background": "#232322", "border": "#55544f" } },
        "score": { "fontColor": "#ecebe8", "backColor": "#232322", "borderColor": "#55544f" },
        "tooltip": { "fontColor": "#ecebe8", "background": "#232322", "borderColor": "#55544f" }
    })
}

impl Default for ChartOptions {
    fn default() -> Self {
        serde_json::from_value(default_json()).expect("기본 옵션")
    }
}

/// base 위에 patch를 깊게 덮는다. 배열과 null은 통째로 바꾼다.
pub fn merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                match b.get_mut(k) {
                    Some(bv) if bv.is_object() && v.is_object() => merge(bv, v),
                    _ => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, p) if !p.is_null() || !b.is_object() => *b = p.clone(),
        _ => {}
    }
}

/// 차트 종류별 기본값 (legend 위치, 옵션 부분 값)
pub fn chart_defaults(chart: ChartType) -> (LegendPosition, Value) {
    match chart {
        ChartType::Funnel => (LegendPosition::Right, json!({})),
        ChartType::Radar => (LegendPosition::Top, json!({ "size": { "lineWidth": 2 } })),
        ChartType::Candle | ChartType::Waterfall | ChartType::Sankey => (LegendPosition::Top, json!({ "size": { "lineWidth": 1 } })),
        _ => (LegendPosition::Top, json!({})),
    }
}

/// 차트 종류 하나의 기본값 (테마 반영)
pub fn defaults_for(chart: ChartType, theme: ChartTheme) -> Value {
    let mut v = default_json();
    if theme == ChartTheme::Dark {
        merge(&mut v, &dark_json());
    }
    merge(&mut v, &chart_defaults(chart).1);
    v
}

pub fn support(chart: ChartType) -> Vec<OptionSection> {
    use OptionSection::*;
    let without: &[OptionSection] = match chart {
        ChartType::Scatter | ChartType::Line => &[],
        ChartType::Bar => &[Score, Prediction],
        ChartType::Boxplot => &[Score, Prediction, Tooltip, Expression],
        ChartType::Histogram => &[Score, Prediction, Focus, Expression],
        ChartType::Pie | ChartType::Treemap => &[Axes, Score, Prediction, Focus, Expression],
        ChartType::Gantt => &[Score, Prediction, Expression],
        ChartType::Heatmap => &[Legend, Shape, Score, Prediction, Focus, Expression],
        ChartType::Candle => &[Legend, Shape, Score, Prediction, Value, Expression],
        ChartType::Waterfall => &[Legend, Shape, Score, Prediction, Focus, Expression],
        ChartType::Funnel | ChartType::Radar | ChartType::Sankey => &[Axes, PointLabel, Score, Prediction, Focus, Expression],
    };
    OPTION_SECTIONS.iter().copied().filter(|s| !without.contains(s)).collect()
}

pub fn supports(chart: ChartType, s: OptionSection) -> bool {
    support(chart).contains(&s)
}

/// 차트가 쓰지 않는 구역을 뺀 부분 값
pub fn pick_supported(chart: ChartType, patch: &Value) -> Value {
    let mut out = Map::new();
    if let Value::Object(p) = patch {
        for s in support(chart) {
            if let Some(v) = p.get(s.key()) {
                out.insert(s.key().into(), v.clone());
            }
        }
    }
    Value::Object(out)
}

/// 기본값 → 테마 → 차트 종류 → 공통 → item 개별 순으로 합친 전체 JSON
pub fn resolve_json(chart: ChartType, theme: ChartTheme, common: &Value, item: &Value) -> Value {
    let mut v = defaults_for(chart, theme);
    merge(&mut v, &pick_supported(chart, common));
    merge(&mut v, &pick_supported(chart, item));
    v
}

pub fn resolve(chart: ChartType, theme: ChartTheme, common: &Value, item: &Value) -> ChartOptions {
    // 잘못된 값(예전 저장, 범위 밖)은 기본값으로 그린다
    serde_json::from_value(resolve_json(chart, theme, common, item))
        .unwrap_or_else(|_| serde_json::from_value(defaults_for(chart, theme)).unwrap_or_default())
}

/// 점 표기 경로("title.text")의 값
pub fn get_path<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |cur, k| cur.get(k))
}

/// 부분 값에 경로 값을 쓴다 (없는 객체는 만든다)
pub fn set_path(v: &mut Value, path: &str, value: Value) {
    if !v.is_object() {
        *v = Value::Object(Map::new());
    }
    let keys: Vec<&str> = path.split('.').collect();
    let mut cur = v;
    for k in &keys[..keys.len() - 1] {
        let obj = cur.as_object_mut().unwrap();
        let next = obj.entry(k.to_string()).or_insert_with(|| Value::Object(Map::new()));
        if !next.is_object() {
            *next = Value::Object(Map::new());
        }
        cur = next;
    }
    cur.as_object_mut().unwrap().insert(keys[keys.len() - 1].to_string(), value);
}

/// 부분 값에서 경로를 지운다 (빈 객체도 정리)
pub fn remove_path(v: &mut Value, path: &str) {
    fn go(v: &mut Value, keys: &[&str]) {
        let Some(obj) = v.as_object_mut() else { return };
        if keys.len() == 1 {
            obj.remove(keys[0]);
            return;
        }
        if let Some(child) = obj.get_mut(keys[0]) {
            go(child, &keys[1..]);
            if child.as_object().is_some_and(|o| o.is_empty()) {
                obj.remove(keys[0]);
            }
        }
    }
    let keys: Vec<&str> = path.split('.').collect();
    go(v, &keys);
}

/// 글자 크기: 자동이면 item 크기에 맞춰 font_min..font_max, 아니면 구역의 글자 크기
pub fn font_px(o: &ChartOptions, fixed: f32, base: f32, width: f32, height: f32) -> f32 {
    if o.size.font_auto {
        // 400×300 기준 base, 커지면 비례해서 키운다
        let k = ((width * height).sqrt() / (400.0f32 * 300.0).sqrt()).clamp(0.6, 2.0);
        (base * k.sqrt()).clamp(o.size.font_min, o.size.font_max.max(o.size.font_min))
    } else {
        fixed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse_and_merge() {
        let o = ChartOptions::default();
        assert!(o.title.visible);
        assert!(!o.axes.x.grid.visible && o.axes.y.grid.visible);
        let common = json!({ "title": { "text": "A" }, "score": { "visible": true } });
        let item = json!({ "title": { "subtitle": "B" } });
        let r = resolve(ChartType::Bar, ChartTheme::Light, &common, &item);
        assert_eq!(r.title.text, "A");
        assert_eq!(r.title.subtitle, "B");
        // bar는 score 구역을 쓰지 않는다
        assert!(!r.score.visible);
        let d = resolve(ChartType::Radar, ChartTheme::Dark, &Value::Null, &Value::Null);
        assert_eq!(d.size.line_width, 2.0);
        assert_eq!(d.panel.back_color, "#141413");
    }

    #[test]
    fn paths() {
        let mut p = json!({});
        set_path(&mut p, "axes.x.title.text", json!("t"));
        assert_eq!(get_path(&p, "axes.x.title.text"), Some(&json!("t")));
        remove_path(&mut p, "axes.x.title.text");
        assert_eq!(p, json!({}));
    }
}
