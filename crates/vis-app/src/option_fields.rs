//! 차트 속성 편집 화면의 필드 표 (원본 src/features/chart-settings/model/option-fields.ts).
//! 구역마다 필드 설명(라벨, 경로, 입력 종류)을 두고 한 폼(options_form.rs)이 그린다. 경로는 ChartOptions JSON 기준 점 표기다.

use serde_json::Value;
use vis_core::options::{OptionSection, get_path};

pub type Choice = (&'static str, &'static str);

#[derive(Clone)]
pub enum Kind {
    Bool,
    /// step은 원본 표의 입력 간격 (값 확인은 min·max로만)
    Number {
        min: f64,
        max: f64,
        #[allow(dead_code)]
        step: f64,
        nullable: bool,
    },
    Color {
        optional: bool,
    },
    Select(Vec<Choice>),
    Multi(Vec<Choice>),
    Text {
        max: usize,
        placeholder: &'static str,
    },
    Heading,
}

#[derive(Clone)]
pub struct Field {
    pub path: String,
    pub label: &'static str,
    pub kind: Kind,
    /// 다른 값에 따라 보일 때만 (예: 예측 방법별 매개변수)
    pub when: Option<fn(&Value) -> bool>,
}

fn f(path: impl Into<String>, label: &'static str, kind: Kind) -> Field {
    Field {
        path: path.into(),
        label,
        kind,
        when: None,
    }
}
fn b(path: impl Into<String>, label: &'static str) -> Field {
    f(path, label, Kind::Bool)
}
fn num(path: impl Into<String>, label: &'static str, min: f64, max: f64, step: f64) -> Field {
    f(
        path,
        label,
        Kind::Number {
            min,
            max,
            step,
            nullable: false,
        },
    )
}
fn num_null(path: impl Into<String>, label: &'static str, min: f64, max: f64, step: f64) -> Field {
    f(
        path,
        label,
        Kind::Number {
            min,
            max,
            step,
            nullable: true,
        },
    )
}
fn color(path: impl Into<String>, label: &'static str) -> Field {
    f(path, label, Kind::Color { optional: false })
}
fn color_opt(path: impl Into<String>, label: &'static str) -> Field {
    f(path, label, Kind::Color { optional: true })
}
fn text(path: impl Into<String>, label: &'static str, max: usize, placeholder: &'static str) -> Field {
    f(path, label, Kind::Text { max, placeholder })
}
fn sel(path: impl Into<String>, label: &'static str, choices: &[Choice]) -> Field {
    f(path, label, Kind::Select(choices.to_vec()))
}
fn heading(label: &'static str) -> Field {
    f("", label, Kind::Heading)
}
fn when(mut field: Field, w: fn(&Value) -> bool) -> Field {
    field.when = Some(w);
    field
}

const ALIGNS: &[Choice] = &[("left", "왼쪽"), ("center", "가운데"), ("right", "오른쪽")];
const SHAPES: &[Choice] = &[
    ("circle", "원"),
    ("square", "사각형"),
    ("triangle", "삼각형"),
    ("diamond", "마름모"),
    ("cross", "십자"),
];
const COMBINE: &[Choice] = &[("additive", "덧셈"), ("multiplicative", "곱셈")];

fn method(o: &Value) -> &str {
    get_path(o, "prediction.method").and_then(|v| v.as_str()).unwrap_or("sma")
}

fn axis_fields(axis: &str) -> Vec<Field> {
    let p = format!("axes.{axis}");
    vec![
        b(format!("{p}.visible"), "축 보이기"),
        b(format!("{p}.title.visible"), "축 제목 보이기"),
        text(format!("{p}.title.text"), "축 제목", 80, "비우면 컬럼 이름"),
        color_opt(format!("{p}.title.color"), "축 제목 색 (비우면 눈금 글자 색)"),
        num_null(format!("{p}.min"), "최솟값", -1e15, 1e15, 1.0),
        num_null(format!("{p}.max"), "최댓값", -1e15, 1e15, 1.0),
        b(format!("{p}.log"), "로그 축"),
        b(format!("{p}.line.visible"), "축 선 보이기"),
        num(format!("{p}.line.width"), "축 선 굵기", 0.0, 10.0, 0.5),
        color(format!("{p}.line.color"), "축 선 색"),
        b(format!("{p}.grid.visible"), "그리드 보이기"),
        num(format!("{p}.grid.width"), "그리드 굵기", 0.0, 10.0, 0.5),
        color(format!("{p}.grid.color"), "그리드 색"),
        b(format!("{p}.tick.visible"), "눈금 보이기"),
        num(format!("{p}.tick.count"), "눈금 개수", 1.0, 20.0, 1.0),
        num(format!("{p}.tick.length"), "눈금 길이", 0.0, 20.0, 1.0),
        color(format!("{p}.tick.color"), "눈금 색"),
        b(format!("{p}.label.visible"), "눈금 글자 보이기"),
        num(format!("{p}.label.angle"), "눈금 글자 각도", -90.0, 90.0, 5.0),
        num(format!("{p}.label.fontSize"), "눈금 글자 크기", 6.0, 40.0, 1.0),
        color(format!("{p}.label.color"), "눈금 글자 색"),
        num_null(format!("{p}.decimals"), "소수 자리", 0.0, 10.0, 1.0),
        text(format!("{p}.dateFormat"), "날짜 형식", 40, "예: %m/%d %H:%M · 비우면 자동"),
    ]
}

pub fn section_fields(s: OptionSection) -> Vec<Field> {
    match s {
        OptionSection::Title => vec![
            b("title.visible", "제목 보이기"),
            text("title.text", "제목", 80, "비우면 split 조합 이름"),
            text("title.subtitle", "부제목", 80, ""),
            color("title.subtitleColor", "부제목 색"),
            num("title.fontSize", "글자 크기", 6.0, 40.0, 1.0),
            b("title.bold", "굵게"),
            color("title.color", "글자 색"),
            sel("title.align", "정렬", ALIGNS),
            heading("요약 (오른쪽 점 수·범주 수)"),
            b("title.summary.visible", "요약 보이기"),
            color("title.summary.color", "요약 글자 색"),
        ],
        OptionSection::Axes => {
            let mut out = vec![heading("X축")];
            out.extend(axis_fields("x"));
            out.push(heading("Y축"));
            out.extend(axis_fields("y"));
            out
        }
        OptionSection::Legend => vec![
            b("legend.visible", "범례 보이기"),
            b("legend.border", "테두리 보이기"),
            color("legend.borderColor", "테두리 색"),
            b("legend.reverse", "순서 뒤집기"),
            num("legend.fontSize", "글자 크기", 6.0, 40.0, 1.0),
            color("legend.fontColor", "글자 색"),
            color("legend.titleColor", "제목 색"),
        ],
        OptionSection::Panel => vec![
            color("panel.backColor", "배경 색"),
            b("panel.gradient", "그라데이션"),
            b("panel.border", "테두리"),
            color("panel.borderColor", "테두리 색"),
            heading("여백"),
            num("panel.margin.top", "위", 0.0, 200.0, 1.0),
            num("panel.margin.right", "오른쪽", 0.0, 200.0, 1.0),
            num("panel.margin.bottom", "아래", 0.0, 200.0, 1.0),
            num("panel.margin.left", "왼쪽", 0.0, 200.0, 1.0),
        ],
        OptionSection::Palette => vec![
            b("palette.gradient", "그라데이션 색 쓰기"),
            color("palette.start", "시작 색"),
            color_opt("palette.middle", "중간 색"),
            color("palette.end", "끝 색"),
            num("palette.steps", "단계 수 (0이면 범례 값 수)", 0.0, 64.0, 1.0),
            b("palette.reverse", "색 순서 뒤집기"),
            color("palette.othersColor", "기타 색 ('기타' 조각, 강조 시 나머지)"),
        ],
        OptionSection::Shape => vec![
            sel("shape.type", "기본 모양", SHAPES),
            sel(
                "shape.style",
                "적용",
                &[("both", "채우기 + 테두리"), ("border", "테두리만"), ("brush", "채우기만")],
            ),
            num("shape.transparency", "투명도 (0 = 불투명)", 0.0, 1.0, 0.05),
            num("shape.borderWidth", "테두리 굵기", 0.0, 10.0, 0.5),
            color_opt("shape.borderColor", "테두리 색 (비우면 자동)"),
            num("shape.dimOpacity", "선택 밖 진하기", 0.0, 1.0, 0.05),
            num("shape.cornerRadius", "막대 모서리 둥글기", 0.0, 20.0, 1.0),
            color("shape.medianColor", "중앙값 선 색 (boxplot)"),
        ],
        OptionSection::Size => vec![
            heading("점 크기"),
            b("size.pointAuto", "자동"),
            when(num("size.pointFixed", "고정 크기", 0.5, 40.0, 0.5), |o| {
                !get_path(o, "size.pointAuto").and_then(|v| v.as_bool()).unwrap_or(true)
            }),
            num("size.pointMin", "최소", 0.5, 40.0, 0.5),
            num("size.pointMax", "최대", 0.5, 40.0, 0.5),
            heading("글자 크기"),
            b("size.fontAuto", "자동"),
            num("size.fontMin", "최소", 6.0, 40.0, 1.0),
            num("size.fontMax", "최대", 6.0, 40.0, 1.0),
            heading("선"),
            num("size.lineWidth", "선 굵기", 0.5, 10.0, 0.5),
        ],
        OptionSection::PointLabel => vec![
            b("pointLabel.visible", "점 라벨 보이기"),
            sel(
                "pointLabel.position",
                "위치",
                &[("top", "위"), ("center", "가운데"), ("bottom", "아래")],
            ),
            num("pointLabel.angle", "각도", -90.0, 90.0, 5.0),
            num("pointLabel.fontSize", "글자 크기", 6.0, 40.0, 1.0),
            color("pointLabel.color", "글자 색"),
            text("pointLabel.prefix", "앞 글자", 20, ""),
            text("pointLabel.suffix", "뒤 글자", 20, ""),
            num_null("pointLabel.decimals", "소수 자리", 0.0, 10.0, 1.0),
            heading("상자"),
            b("pointLabel.box.visible", "상자 보이기"),
            color("pointLabel.box.background", "배경 색"),
            color("pointLabel.box.border", "테두리 색"),
        ],
        OptionSection::Score => vec![
            b("score.visible", "점수 보이기"),
            f(
                "score.kinds",
                "점수 목록",
                Kind::Multi(vec![
                    ("rscore", "상관계수 (r)"),
                    ("fscore", "분산분석 F"),
                    ("zscore", "규격 내 확률 (z)"),
                    ("cpk", "공정 능력 (Cpk)"),
                ]),
            ),
            num_null("score.ucl", "상한", -1e15, 1e15, 1.0),
            num_null("score.lcl", "하한", -1e15, 1e15, 1.0),
            num("score.fontSize", "글자 크기", 6.0, 40.0, 1.0),
            color("score.fontColor", "글자 색"),
            color("score.backColor", "상자 배경 색"),
            color("score.borderColor", "상자 테두리 색"),
            num("score.borderWidth", "상자 테두리 굵기", 0.0, 10.0, 0.5),
        ],
        OptionSection::Prediction => vec![
            b("prediction.enabled", "예측 보이기"),
            sel(
                "prediction.method",
                "방법",
                &[
                    ("sma", "단순 이동 평균"),
                    ("ses", "단순 지수 평활"),
                    ("des", "이중 지수 평활"),
                    ("holtWinters", "홀트-윈터스 지수 평활"),
                ],
            ),
            num("prediction.period", "예측 개수", 1.0, 500.0, 1.0),
            b("prediction.showInterval", "신뢰 구간"),
            b("prediction.description", "설명 보이기"),
            when(num("prediction.window", "이동 평균 길이", 2.0, 500.0, 1.0), |o| {
                method(o) == "sma"
            }),
            when(
                num_null("prediction.alpha", "수준 계수 α (비우면 자동)", 0.0, 1.0, 0.05),
                |o| matches!(method(o), "ses" | "des" | "holtWinters"),
            ),
            when(
                num_null("prediction.beta", "추세 계수 β (비우면 자동)", 0.0, 1.0, 0.05),
                |o| matches!(method(o), "des" | "holtWinters"),
            ),
            when(
                num_null("prediction.gamma", "계절 계수 γ (비우면 자동)", 0.0, 1.0, 0.05),
                |o| method(o) == "holtWinters",
            ),
            when(sel("prediction.trend", "추세 방식", COMBINE), |o| {
                matches!(method(o), "des" | "holtWinters")
            }),
            when(num("prediction.seasonLength", "계절 길이", 2.0, 500.0, 1.0), |o| {
                method(o) == "holtWinters"
            }),
            when(sel("prediction.seasonal", "계절 방식", COMBINE), |o| method(o) == "holtWinters"),
            when(b("prediction.damped", "감쇠"), |o| method(o) == "holtWinters"),
        ],
        OptionSection::Value => vec![
            num_null("value.nullNumeric", "빈 칸 값 (비우면 그리지 않음)", -1e15, 1e15, 1.0),
            b("value.outlier", "이상치 빼기"),
            sel(
                "value.outlierMethod",
                "이상치 방식",
                &[("iqr", "IQR (Q1 − k·IQR .. Q3 + k·IQR)"), ("sigma", "정규분포 (평균 ± k·σ)")],
            ),
            num("value.outlierK", "k", 0.0, 10.0, 0.1),
        ],
        OptionSection::Focus => vec![
            b("focus.enabled", "강조 쓰기"),
            sel("focus.target", "대상", &[("x", "x 값"), ("y", "y 값")]),
            sel(
                "focus.operator",
                "조건",
                &[(">", ">"), (">=", ">="), ("<", "<"), ("<=", "<="), ("=", "="), ("!=", "!=")],
            ),
            num("focus.value", "값", -1e15, 1e15, 1.0),
            color("focus.color", "색"),
            sel("focus.shape", "모양", SHAPES),
            num("focus.size", "크기", 1.0, 40.0, 0.5),
            b("focus.legendSingleColor", "나머지는 한 가지 색"),
        ],
        OptionSection::Tooltip => vec![
            text("tooltip.title", "제목", 80, "비우면 제목 없음"),
            num("tooltip.fontSize", "글자 크기", 6.0, 40.0, 1.0),
            color("tooltip.fontColor", "글자 색"),
            color("tooltip.background", "배경 색"),
            num("tooltip.transparency", "투명도", 0.0, 1.0, 0.05),
            b("tooltip.round", "둥근 모서리"),
            b("tooltip.border", "테두리"),
            color("tooltip.borderColor", "테두리 색"),
            text("tooltip.prefix", "앞 글자", 20, ""),
            text("tooltip.suffix", "뒤 글자", 20, ""),
            num_null("tooltip.decimals", "소수 자리", 0.0, 10.0, 1.0),
        ],
        OptionSection::Expression => vec![
            text("expression.x", "X 값", 200, "예: x * 2"),
            text("expression.y", "Y 값", 200, "예: log(y)"),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vis_core::options::{OPTION_SECTIONS, default_json};

    /// 서버 기본값의 모든 칸이 설정 창에 있는지 (원본 option-fields.test.ts)
    #[test]
    fn every_default_has_a_field() {
        let mut paths = std::collections::HashSet::new();
        for s in OPTION_SECTIONS {
            for f in section_fields(s) {
                if !f.path.is_empty() {
                    paths.insert(f.path);
                }
            }
        }
        fn walk(v: &Value, prefix: &str, out: &mut Vec<String>) {
            match v {
                Value::Object(m) => {
                    for (k, v) in m {
                        let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                        walk(v, &p, out);
                    }
                }
                _ => out.push(prefix.to_string()),
            }
        }
        let mut all = vec![];
        walk(&default_json(), "", &mut all);
        let missing: Vec<_> = all.into_iter().filter(|p| !paths.contains(p)).collect();
        assert!(missing.is_empty(), "{missing:?}");
    }
}
