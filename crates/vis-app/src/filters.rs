//! 오른쪽 필터 패널 (원본 features/data-filter). 컬럼마다 아코디언이고, 열 때 그 컬럼 요약을 읽는다.
//! 숫자·시계열: 범위(최솟값·최댓값) + 반전 + 값 없음 포함. 범주: 검색 + 가상 목록 체크. draw할 때 적용한다.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::component::accordion::Accordion;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use vis_core::dataset::ColumnKind;
use vis_core::query::summary::{ColumnSummary, column_summary};
use vis_core::spec::Filter;
use vis_core::store::Store;
use vis_core::timefmt::format_kst;

const SLIDER_STEPS: u32 = 1000;

enum Edit {
    Range {
        min: f64,
        max: f64,
        inverse: bool,
        include_null: bool,
        full: (f64, f64),
    },
    Exclude(HashSet<String>),
}

pub struct FilterPanel {
    store: Arc<Store>,
    dataset: String,
    open: HashSet<String>,
    summaries: HashMap<String, ColumnSummary>,
    edits: BTreeMap<String, Edit>,
    /// 범위 Slider (0..=SLIDER_STEPS로 정규화. 시계열 epoch ms는 f32로 담으면 2분 단위로 뭉개진다)
    sliders: HashMap<String, Entity<SliderState>>,
    search: HashMap<String, Entity<InputState>>,
    /// draw 전에 받은 필터 (history 복원)
    preset: Vec<Filter>,
    _subs: Vec<Subscription>,
}

impl FilterPanel {
    pub fn new(store: Arc<Store>, dataset: String, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            store,
            dataset,
            open: HashSet::new(),
            summaries: HashMap::new(),
            edits: BTreeMap::new(),
            sliders: HashMap::new(),
            search: HashMap::new(),
            preset: vec![],
            _subs: vec![],
        }
    }

    pub fn set_dataset(&mut self, id: String, _window: &mut Window, cx: &mut Context<Self>) {
        if id != self.dataset {
            self.dataset = id;
            self.open.clear();
            self.summaries.clear();
            self.edits.clear();
            self.sliders.clear();
            self.search.clear();
            self.preset.clear();
            cx.notify();
        }
    }

    pub fn set_filters(&mut self, filters: Vec<Filter>, cx: &mut Context<Self>) {
        self.edits.clear();
        for f in &filters {
            match f {
                Filter::Range {
                    column,
                    min,
                    max,
                    inverse,
                    include_null,
                } => {
                    self.edits.insert(
                        column.clone(),
                        Edit::Range {
                            min: *min,
                            max: *max,
                            inverse: *inverse,
                            include_null: *include_null,
                            full: (*min, *max),
                        },
                    );
                }
                Filter::Exclude { column, values } => {
                    self.edits.insert(column.clone(), Edit::Exclude(values.iter().cloned().collect()));
                }
            }
        }
        self.preset = filters;
        cx.notify();
    }

    /// 지금 고친 필터 (draw할 때 적용)
    pub fn filters(&self) -> Vec<Filter> {
        let mut out = vec![];
        for (col, e) in &self.edits {
            match e {
                Edit::Range {
                    min,
                    max,
                    inverse,
                    include_null,
                    full,
                } => {
                    // 전체 범위 그대로이고 반전·null 제외도 없으면 필터를 두지 않는다
                    let untouched = (*min <= full.0 && *max >= full.1) && !inverse && *include_null;
                    if !untouched {
                        out.push(Filter::Range {
                            column: col.clone(),
                            min: *min,
                            max: *max,
                            inverse: *inverse,
                            include_null: *include_null,
                        });
                    }
                }
                Edit::Exclude(v) if !v.is_empty() => {
                    let mut values: Vec<String> = v.iter().cloned().collect();
                    values.sort();
                    out.push(Filter::Exclude {
                        column: col.clone(),
                        values,
                    });
                }
                _ => {}
            }
        }
        out
    }

    pub fn active_count(&self) -> usize {
        self.filters().len()
    }

    /// Accordion에서 열린 컬럼이 바뀜
    fn set_open(&mut self, open: HashSet<String>, window: &mut Window, cx: &mut Context<Self>) {
        let schema = self.store.get(&self.dataset).map(|d| d.schema()).unwrap_or_default();
        for col in open.difference(&self.open.clone()) {
            if let Some(c) = schema.iter().find(|c| &c.name == col) {
                self.prepare(col.clone(), c.kind, window, cx);
            }
        }
        self.open = open;
        cx.notify();
    }

    /// 열 때 그 컬럼 요약을 읽고 입력 상태를 만든다 (원본은 서버 호출, 여기서는 열 배열을 바로 훑는다)
    fn prepare(&mut self, col: String, kind: ColumnKind, window: &mut Window, cx: &mut Context<Self>) {
        if !self.summaries.contains_key(&col) {
            if let Some(ds) = self.store.get(&self.dataset) {
                if let Ok(s) = column_summary(&ds, &col) {
                    if let ColumnSummary::Range { min, max, .. } = &s {
                        let e = self.edits.entry(col.clone()).or_insert(Edit::Range {
                            min: *min,
                            max: *max,
                            inverse: false,
                            include_null: true,
                            full: (*min, *max),
                        });
                        if let Edit::Range { full, .. } = e {
                            *full = (*min, *max);
                        }
                    }
                    self.summaries.insert(col.clone(), s);
                }
            }
        }
        match kind {
            ColumnKind::Categorical => {
                if !self.search.contains_key(&col) {
                    let input = cx.new(|cx| InputState::new(window, cx).placeholder("검색"));
                    self._subs.push(cx.subscribe(&input, |_, _, e: &InputEvent, cx| {
                        if matches!(e, InputEvent::Change) {
                            cx.notify();
                        }
                    }));
                    self.search.insert(col.clone(), input);
                }
            }
            _ => {
                if !self.sliders.contains_key(&col) {
                    let Some(Edit::Range { min, max, full, .. }) = self.edits.get(&col) else {
                        return;
                    };
                    let to_t = |v: f64| {
                        if full.1 > full.0 {
                            ((v - full.0) / (full.1 - full.0) * SLIDER_STEPS as f64) as f32
                        } else {
                            0.0
                        }
                    };
                    let (a, b) = (to_t(*min), to_t(*max));
                    let slider = cx.new(|_| SliderState::new().min(0.0).max(SLIDER_STEPS as f32).step(1.0).default_value(a..b));
                    let c = col.clone();
                    self._subs.push(cx.subscribe(&slider, move |this, _, e: &SliderEvent, cx| {
                        if let SliderEvent::Change(v) = e {
                            if let Some(Edit::Range { min, max, full, .. }) = this.edits.get_mut(&c) {
                                let at = |t: f32| full.0 + (full.1 - full.0) * (t as f64 / SLIDER_STEPS as f64);
                                // 끝까지 끌면 정확히 전체 범위 (반올림 오차로 끝 행이 빠지지 않게)
                                *min = if v.start() <= 0.0 { full.0 } else { at(v.start()) };
                                *max = if v.end() >= SLIDER_STEPS as f32 { full.1 } else { at(v.end()) };
                            }
                            cx.notify();
                        }
                    }));
                    self.sliders.insert(col, slider);
                }
            }
        }
    }

    fn render_body(&self, name: &str, kind: ColumnKind, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(summary) = self.summaries.get(name) else {
            return div().p_2().text_xs().child("요약 없음").into_any_element();
        };
        match summary {
            ColumnSummary::Range { min, max, nulls } => {
                let (
                    Some(slider),
                    Some(Edit::Range {
                        inverse,
                        include_null,
                        min: lo,
                        max: hi,
                        ..
                    }),
                ) = (self.sliders.get(name), self.edits.get(name))
                else {
                    return div().into_any_element();
                };
                let time = kind == ColumnKind::Datetime;
                let fmt = |v: f64| {
                    if time {
                        format_kst(v, "%Y-%m-%d %H:%M")
                    } else {
                        vis_core::render::scale::short_number(v)
                    }
                };
                let (c1, c2, c3) = (name.to_string(), name.to_string(), name.to_string());
                v_flex()
                    .gap_1p5()
                    .child(div().text_xs().text_color(theme.muted_foreground).child(format!(
                        "전체 {} ~ {} · 값 없음 {}",
                        fmt(*min),
                        fmt(*max),
                        nulls
                    )))
                    .child(Slider::new(slider).w_full())
                    .child(
                        h_flex()
                            .justify_between()
                            .text_xs()
                            .child(div().child(fmt(*lo)))
                            .child(div().child(fmt(*hi))),
                    )
                    .child(
                        Checkbox::new(SharedString::from(format!("inv-{name}")))
                            .label("범위 반전 (범위 밖만)")
                            .checked(*inverse)
                            .on_change(cx.listener(move |this, v: &bool, _, cx| {
                                if let Some(Edit::Range { inverse, .. }) = this.edits.get_mut(&c1) {
                                    *inverse = *v;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Checkbox::new(SharedString::from(format!("nul-{name}")))
                            .label("값 없음(null) 포함")
                            .checked(*include_null)
                            .on_change(cx.listener(move |this, v: &bool, _, cx| {
                                if let Some(Edit::Range { include_null, .. }) = this.edits.get_mut(&c2) {
                                    *include_null = *v;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("rst-{name}")))
                            .label("전체 범위로")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(Edit::Range {
                                    min,
                                    max,
                                    full,
                                    inverse,
                                    include_null,
                                }) = this.edits.get_mut(&c3)
                                {
                                    (*min, *max, *inverse, *include_null) = (full.0, full.1, false, true);
                                }
                                if let Some(sl) = this.sliders.get(&c3).cloned() {
                                    sl.update(cx, |s, cx| s.set_value(0.0..SLIDER_STEPS as f32, window, cx));
                                }
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
            ColumnSummary::Values { values, nulls } => {
                let q = self
                    .search
                    .get(name)
                    .map(|s| s.read(cx).value().to_string().to_lowercase())
                    .unwrap_or_default();
                let shown: Vec<(String, usize)> = values
                    .iter()
                    .filter(|(v, _)| q.is_empty() || v.to_lowercase().contains(&q))
                    .cloned()
                    .collect();
                let excluded: HashSet<String> = match self.edits.get(name) {
                    Some(Edit::Exclude(s)) => s.clone(),
                    _ => HashSet::new(),
                };
                let count = shown.len();
                let col = name.to_string();
                let (c_all, c_none) = (col.clone(), col.clone());
                let shown_all: Vec<String> = shown.iter().map(|s| s.0.clone()).collect();
                let shown_none = shown_all.clone();
                let list_col = col.clone();
                let list = uniform_list(
                    SharedString::from(format!("vals-{name}")),
                    count,
                    cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                        let excluded: HashSet<String> = match this.edits.get(&list_col) {
                            Some(Edit::Exclude(s)) => s.clone(),
                            _ => HashSet::new(),
                        };
                        range
                            .map(|i| {
                                let (v, n) = shown[i].clone();
                                let checked = !excluded.contains(&v);
                                let col = list_col.clone();
                                let value = v.clone();
                                h_flex()
                                    .h(px(24.))
                                    .px_1()
                                    .gap_1()
                                    .child(
                                        Checkbox::new(SharedString::from(format!("cv-{col}-{i}")))
                                            .label(v)
                                            .checked(checked)
                                            .on_change(cx.listener(move |this, on: &bool, _, cx| {
                                                let e = this.edits.entry(col.clone()).or_insert(Edit::Exclude(HashSet::new()));
                                                if let Edit::Exclude(s) = e {
                                                    if *on {
                                                        s.remove(&value);
                                                    } else {
                                                        s.insert(value.clone());
                                                    }
                                                }
                                                cx.notify();
                                            })),
                                    )
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(vis_core::render::charts_fmt_int(n)),
                                    )
                            })
                            .collect()
                    }),
                )
                .h(px((count.min(10) as f32 * 24.0).max(24.0)));
                v_flex()
                    .p_2()
                    .gap_1()
                    .when_some(self.search.get(name), |d, s| d.child(Input::new(s).small().cleanable(true)))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(div().flex_1().text_xs().text_color(theme.muted_foreground).child(format!(
                                "{}개 중 {}개 제외 · 값 없음 {}",
                                values.len(),
                                excluded.len(),
                                nulls
                            )))
                            .child(
                                Button::new(SharedString::from(format!("all-{name}")))
                                    .label("보이는 값 선택")
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(Edit::Exclude(s)) = this.edits.get_mut(&c_all) {
                                            for v in &shown_all {
                                                s.remove(v);
                                            }
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!("none-{name}")))
                                    .label("해제")
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let e = this.edits.entry(c_none.clone()).or_insert(Edit::Exclude(HashSet::new()));
                                        if let Edit::Exclude(s) = e {
                                            s.extend(shown_none.iter().cloned());
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(list)
                    .into_any_element()
            }
        }
    }
}

impl Render for FilterPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let schema = self.store.get(&self.dataset).map(|d| d.schema()).unwrap_or_default();
        // 컬럼마다 아코디언 (여러 개 열 수 있다). 여는 순간 그 컬럼 요약을 읽는다
        let names: Vec<String> = schema.iter().map(|c| c.name.clone()).collect();
        let active: Vec<String> = self.filters().iter().map(|f| f.column().to_string()).collect();
        let ent = cx.entity().downgrade();
        let mut list =
            Accordion::new("filter-columns")
                .multiple(true)
                .small()
                .h_auto()
                .on_toggle_click(move |open: &[usize], window, cx| {
                    let set: HashSet<String> = open.iter().filter_map(|&i| names.get(i).cloned()).collect();
                    let _ = ent.update(cx, |this, cx| this.set_open(set, window, cx));
                });
        for c in &schema {
            let open = self.open.contains(&c.name);
            let body = open.then(|| self.render_body(&c.name, c.kind, cx));
            let on = active.contains(&c.name);
            let title = h_flex()
                .w_full()
                .gap_2()
                .child(div().flex_1().child(c.label.clone()))
                .child(div().text_xs().text_color(theme.muted_foreground).child(c.kind.label()))
                .when(on, |h| h.child(div().size(px(7.)).rounded_full().bg(theme.primary)));
            list = list.item(move |item| {
                let item = item.open(open).title(title);
                match body {
                    Some(b) => item.child(b),
                    None => item,
                }
            });
        }
        let n = self.active_count();
        v_flex()
            .w(px(300.))
            .h_full()
            .flex_shrink_0()
            .border_l_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(
                v_flex()
                    .px_3()
                    .py_2()
                    .gap_0p5()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(format!("필터 {n}")))
                    .child(div().text_xs().text_color(theme.muted_foreground).child("draw할 때 적용됩니다.")),
            )
            .child(div().id("filter-scroll").flex_1().overflow_y_scroll().p_1().child(list))
    }
}
