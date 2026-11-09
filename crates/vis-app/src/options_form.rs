//! 차트 속성 구역 폼 (원본 OptionSectionForm). option_fields.rs의 필드 표를 gpui-kit 입력으로 그린다.
//! 편집 중인 값(working)은 "아래 단계까지 합친 값(base) + 이 단계 부분 값"이고, 적용할 때 base와 다른 칸만 부분 값으로 남긴다.

use std::collections::{HashMap, HashSet};

use gpui_kit::component::button::{Toggle, ToggleGroup};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Colorize as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Map, Value, json};

use vis_core::options::{OptionSection, get_path, merge, pick_supported, set_path};
use vis_core::render::theme::parse_hex;
use vis_core::spec::ChartType;

use crate::option_fields::{Field, Kind, section_fields};
use crate::picker::Picker;

pub struct OptionsForm {
    pub chart: ChartType,
    base: Value,
    pub working: Value,
    inputs: HashMap<String, Entity<InputState>>,
    colors: HashMap<String, Entity<ColorPickerState>>,
    selects: HashMap<String, Picker>,
    /// 선택 색을 비운 칸 (자동)
    cleared: HashSet<String>,
    built: HashSet<OptionSection>,
    /// 지금 보이는 구역
    pub section: Option<OptionSection>,
    _subs: Vec<Subscription>,
}

/// base와 다른 칸만 남긴 부분 값 (배열은 통째로 비교)
pub fn diff(base: &Value, cur: &Value) -> Value {
    match (base, cur) {
        (Value::Object(b), Value::Object(c)) => {
            let mut out = Map::new();
            for (k, v) in c {
                match b.get(k) {
                    Some(bv) if bv == v => {}
                    Some(bv) if bv.is_object() && v.is_object() => {
                        let d = diff(bv, v);
                        if d.as_object().is_some_and(|o| !o.is_empty()) {
                            out.insert(k.clone(), d);
                        }
                    }
                    _ => {
                        out.insert(k.clone(), v.clone());
                    }
                }
            }
            Value::Object(out)
        }
        _ => cur.clone(),
    }
}

fn hsla_of(hex: &str) -> Hsla {
    rgb(parse_hex(hex).unwrap_or(0x888888)).into()
}

fn hex_of(c: Hsla) -> String {
    let h = c.to_hex();
    h[..7.min(h.len())].to_lowercase()
}

fn num_text(v: Option<&Value>) -> String {
    match v.and_then(|v| v.as_f64()) {
        Some(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", n as i64),
        Some(n) => format!("{n}"),
        None => String::new(),
    }
}

impl OptionsForm {
    /// base: 이 단계 아래까지 합친 전체 값, patch: 이 단계의 저장된 부분 값
    pub fn new(chart: ChartType, base: Value, patch: &Value) -> Self {
        let mut working = base.clone();
        merge(&mut working, &pick_supported(chart, patch));
        Self {
            chart,
            base,
            working,
            inputs: HashMap::new(),
            colors: HashMap::new(),
            selects: HashMap::new(),
            cleared: HashSet::new(),
            built: HashSet::new(),
            section: None,
            _subs: vec![],
        }
    }

    fn get(&self, path: &str) -> Option<&Value> {
        get_path(&self.working, path)
    }

    /// 구역의 입력 상태를 처음 열 때 만든다
    fn ensure(&mut self, section: OptionSection, window: &mut Window, cx: &mut Context<Self>) {
        if !self.built.insert(section) {
            return;
        }
        for field in section_fields(section) {
            let path = field.path.clone();
            match &field.kind {
                Kind::Number { nullable, min, max, .. } => {
                    let init = num_text(self.get(&path));
                    let state = cx.new(|cx| {
                        InputState::new(window, cx)
                            .default_value(init)
                            .placeholder(if *nullable { "비우면 자동" } else { "" })
                    });
                    let (p, nullable, min, max) = (path.clone(), *nullable, *min, *max);
                    self._subs.push(cx.subscribe(&state, move |this, s, e: &InputEvent, cx| {
                        if matches!(e, InputEvent::Change) {
                            let t = s.read(cx).value().trim().to_string();
                            if t.is_empty() && nullable {
                                set_path(&mut this.working, &p, Value::Null);
                            } else if let Ok(v) = t.parse::<f64>() {
                                set_path(&mut this.working, &p, json!(v.clamp(min, max)));
                            }
                            cx.notify();
                        }
                    }));
                    self.inputs.insert(path, state);
                }
                Kind::Text { .. } => {
                    let init = self.get(&path).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let ph = match &field.kind {
                        Kind::Text { placeholder, .. } => *placeholder,
                        _ => "",
                    };
                    let state = cx.new(|cx| InputState::new(window, cx).default_value(init).placeholder(ph));
                    let max = match &field.kind {
                        Kind::Text { max, .. } => *max,
                        _ => 200,
                    };
                    let p = path.clone();
                    self._subs.push(cx.subscribe(&state, move |this, s, e: &InputEvent, cx| {
                        if matches!(e, InputEvent::Change) {
                            let t: String = s.read(cx).value().chars().take(max).collect();
                            set_path(&mut this.working, &p, Value::String(t));
                            cx.notify();
                        }
                    }));
                    self.inputs.insert(path, state);
                }
                Kind::Color { .. } => {
                    let cur = self.get(&path).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if cur.is_empty() {
                        self.cleared.insert(path.clone());
                    }
                    let c = hsla_of(if cur.is_empty() { "#888888" } else { &cur });
                    let state = cx.new(|cx| ColorPickerState::new(window, cx).default_value(c));
                    let p = path.clone();
                    self._subs.push(cx.subscribe(&state, move |this, _, e: &ColorPickerEvent, cx| {
                        let ColorPickerEvent::Change(Some(c)) = e else { return };
                        this.cleared.remove(&p);
                        set_path(&mut this.working, &p, Value::String(hex_of(*c)));
                        cx.notify();
                    }));
                    self.colors.insert(path, state);
                }
                Kind::Select(choices) => {
                    let p = path.clone();
                    let (mut picker, sub) = Picker::new(field.label, false, window, cx, move |this: &mut Self, v, _, cx| {
                        set_path(&mut this.working, &p, Value::String(v));
                        cx.notify();
                    });
                    let opts = choices.iter().map(|(v, l)| (l.to_string(), v.to_string())).collect();
                    let cur = self.get(&path).and_then(|v| v.as_str()).map(|s| s.to_string());
                    picker.sync(opts, cur, window, cx);
                    self._subs.push(sub);
                    self.selects.insert(path, picker);
                }
                _ => {}
            }
        }
    }

    /// 지금 값에서 이 단계의 부분 값 (base와 다른 칸만, 차트가 쓰는 구역만)
    pub fn patch(&self) -> Value {
        pick_supported(self.chart, &diff(&self.base, &self.working))
    }

    /// 식 구역 오류 (적용 전에 보인다)
    pub fn expression_error(&self) -> Option<String> {
        ["x", "y"].iter().find_map(|k| {
            let src = self.get(&format!("expression.{k}")).and_then(|v| v.as_str()).unwrap_or("");
            vis_core::analytics::Expr::compile(src)
                .err()
                .map(|e| format!("{} 식: {e}", k.to_uppercase()))
        })
    }

    /// 이 구역을 기본값(아래 단계 값)으로 되돌린다
    pub fn reset_section(&mut self, section: OptionSection, cx: &mut Context<Self>) {
        if let (Some(b), Some(w)) = (self.base.get(section.key()).cloned(), self.working.as_object_mut()) {
            w.insert(section.key().into(), b);
        }
        self.built.remove(&section);
        for f in section_fields(section) {
            self.inputs.remove(&f.path);
            self.colors.remove(&f.path);
            self.selects.remove(&f.path);
            self.cleared.remove(&f.path);
        }
        cx.notify();
    }

    pub fn render_section(&mut self, section: OptionSection, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.ensure(section, window, cx);
        let theme = cx.theme().clone();
        let mut col = v_flex().gap_2();
        if let Some(h) = section.hint() {
            col = col.child(div().text_xs().text_color(theme.muted_foreground).child(h));
        }
        for field in section_fields(section) {
            if field.when.is_some_and(|w| !w(&self.working)) {
                continue;
            }
            col = col.child(self.field_row(&field, cx));
        }
        if section == OptionSection::Expression {
            if let Some(e) = self.expression_error() {
                col = col.child(div().text_xs().text_color(theme.danger).child(e));
            }
        }
        col.into_any_element()
    }

    fn field_row(&self, field: &Field, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let label = |s: &str| {
            div()
                .w(px(190.))
                .flex_shrink_0()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(s.to_string())
        };
        let row = |c: AnyElement| {
            h_flex()
                .gap_3()
                .min_h(px(28.))
                .child(label(field.label))
                .child(div().flex_1().min_w_0().child(c))
        };
        let path = field.path.clone();
        let id = SharedString::from(format!("opt-{path}"));
        match &field.kind {
            Kind::Heading => div()
                .pt_2()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(field.label)
                .into_any_element(),
            Kind::Bool => {
                let on = self.get(&path).and_then(|v| v.as_bool()).unwrap_or(false);
                row(Switch::new(id)
                    .checked(on)
                    .small()
                    .on_click(cx.listener(move |this, v: &bool, _, cx| {
                        set_path(&mut this.working, &path, Value::Bool(*v));
                        cx.notify();
                    }))
                    .into_any_element())
                .into_any_element()
            }
            Kind::Number { .. } | Kind::Text { .. } => match self.inputs.get(&path) {
                Some(s) => row(Input::new(s).small().into_any_element()).into_any_element(),
                None => div().into_any_element(),
            },
            Kind::Color { optional } => {
                let Some(s) = self.colors.get(&path) else {
                    return div().into_any_element();
                };
                let cleared = self.cleared.contains(&path);
                let p = path.clone();
                row(h_flex()
                    .gap_2()
                    .child(ColorPicker::new(s).small())
                    .when(!cleared, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(self.get(&path).and_then(|v| v.as_str()).unwrap_or("").to_string()),
                        )
                    })
                    .when(*optional, |d| {
                        d.child(
                            Checkbox::new(SharedString::from(format!("clr-{p}")))
                                .label("비움 (자동)")
                                .checked(cleared)
                                .on_click(cx.listener(move |this, v: &bool, _, cx| {
                                    if *v {
                                        this.cleared.insert(p.clone());
                                        set_path(&mut this.working, &p, Value::String(String::new()));
                                    } else {
                                        this.cleared.remove(&p);
                                        let c = this
                                            .colors
                                            .get(&p)
                                            .and_then(|s| s.read(cx).value())
                                            .map(hex_of)
                                            .unwrap_or("#888888".into());
                                        set_path(&mut this.working, &p, Value::String(c));
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .into_any_element())
                .into_any_element()
            }
            Kind::Select(_) => match self.selects.get(&path) {
                Some(p) => row(p.element().into_any_element()).into_any_element(),
                None => div().into_any_element(),
            },
            Kind::Multi(choices) => {
                let cur: Vec<String> = self
                    .get(&path)
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let mut g = ToggleGroup::new(id).small();
                for (v, l) in choices {
                    g = g.child(
                        Toggle::new(SharedString::from(format!("{path}-{v}")))
                            .label(*l)
                            .checked(cur.iter().any(|c| c == v)),
                    );
                }
                let values: Vec<&'static str> = choices.iter().map(|c| c.0).collect();
                row(g
                    .on_click(cx.listener(move |this, checked: &Vec<bool>, _, cx| {
                        // 고른 순서를 지키고, 새로 켠 것은 끝에 붙인다
                        let on: Vec<&str> = values.iter().zip(checked).filter(|(_, c)| **c).map(|(v, _)| *v).collect();
                        let mut next: Vec<String> = cur.iter().filter(|c| on.contains(&c.as_str())).cloned().collect();
                        for v in on {
                            if !next.iter().any(|c| c == v) {
                                next.push(v.to_string());
                            }
                        }
                        set_path(&mut this.working, &path, json!(next));
                        cx.notify();
                    }))
                    .into_any_element())
                .into_any_element()
            }
        }
    }
}

impl Render for OptionsForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.section {
            Some(s) => self.render_section(s, window, cx),
            None => div().into_any_element(),
        }
    }
}
