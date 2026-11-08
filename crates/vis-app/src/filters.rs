//! 오른쪽 필터 패널 (원본 features/data-filter). 컬럼마다 아코디언이고, 열 때 그 컬럼 요약을 읽는다.
//! 숫자·시계열: 범위(최솟값·최댓값) + 반전 + 값 없음 포함. 범주: 검색 + 가상 목록 체크. draw할 때 적용한다.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use vis_core::dataset::ColumnKind;
use vis_core::query::summary::{ColumnSummary, column_summary};
use vis_core::spec::Filter;
use vis_core::store::Store;
use vis_core::timefmt::{KST_OFFSET_MS, format_kst, parse_datetime};

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
    range_inputs: HashMap<String, (Entity<InputState>, Entity<InputState>)>,
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
            range_inputs: HashMap::new(),
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
            self.range_inputs.clear();
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

    fn toggle(&mut self, col: String, kind: ColumnKind, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open.remove(&col) {
            self.open.insert(col.clone());
            // 열 때 요약을 읽는다 (원본은 서버 호출, 여기서는 열 배열을 바로 훑는다)
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
                    if !self.range_inputs.contains_key(&col) {
                        let time = kind == ColumnKind::Datetime;
                        let (lo, hi) = match self.edits.get(&col) {
                            Some(Edit::Range { min, max, .. }) => (*min, *max),
                            _ => (0.0, 0.0),
                        };
                        let fmt = move |v: f64| if time { format_kst(v, "%Y-%m-%d %H:%M:%S") } else { format!("{v}") };
                        let a = cx.new(|cx| InputState::new(window, cx).default_value(fmt(lo)));
                        let b = cx.new(|cx| InputState::new(window, cx).default_value(fmt(hi)));
                        for (k, input) in [(0, a.clone()), (1, b.clone())] {
                            let col = col.clone();
                            self._subs.push(cx.subscribe(&input, move |this, state, e: &InputEvent, cx| {
                                if !matches!(e, InputEvent::Change | InputEvent::Blur | InputEvent::PressEnter { .. }) {
                                    return;
                                }
                                let text = state.read(cx).value().to_string();
                                let v = if time {
                                    parse_datetime(&text).map(|t| t - KST_OFFSET_MS)
                                } else {
                                    text.trim().parse::<f64>().ok()
                                };
                                if let (Some(v), Some(Edit::Range { min, max, .. })) = (v, this.edits.get_mut(&col)) {
                                    if k == 0 {
                                        *min = v;
                                    } else {
                                        *max = v;
                                    }
                                    cx.notify();
                                }
                            }));
                        }
                        self.range_inputs.insert(col.clone(), (a, b));
                    }
                }
            }
        }
        cx.notify();
    }

    fn render_column(&self, name: &str, label: &str, kind: ColumnKind, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let open = self.open.contains(name);
        let active = self.filters().iter().any(|f| f.column() == name);
        let col = name.to_string();
        let header = h_flex()
            .id(SharedString::from(format!("fh-{name}")))
            .px_2()
            .py_1p5()
            .gap_2()
            .cursor_pointer()
            .hover(|s| s.bg(theme.list_hover))
            .on_click(cx.listener(move |this, _, window, cx| this.toggle(col.clone(), kind, window, cx)))
            .child(gpui_kit::component::Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).xsmall())
            .child(div().flex_1().text_sm().child(format!("{label}")))
            .child(div().text_xs().text_color(theme.muted_foreground).child(kind.label()))
            .when(active, |h| h.child(div().size(px(7.)).rounded_full().bg(theme.primary)));
        let mut block = v_flex().border_b_1().border_color(theme.border).child(header);
        if open {
            block = block.child(self.render_body(name, kind, cx));
        }
        block.into_any_element()
    }

    fn render_body(&self, name: &str, kind: ColumnKind, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(summary) = self.summaries.get(name) else {
            return div().p_2().text_xs().child("요약 없음").into_any_element();
        };
        match summary {
            ColumnSummary::Range { min, max, nulls } => {
                let (Some((a, b)), Some(Edit::Range { inverse, include_null, .. })) = (self.range_inputs.get(name), self.edits.get(name))
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
                    .p_2()
                    .gap_1p5()
                    .child(div().text_xs().text_color(theme.muted_foreground).child(format!(
                        "전체 {} ~ {} · 값 없음 {}",
                        fmt(*min),
                        fmt(*max),
                        nulls
                    )))
                    .child(Input::new(a).small())
                    .child(Input::new(b).small())
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
                                    let (lo, hi) = *full;
                                    if let Some((a, b)) = this.range_inputs.get(&c3).cloned() {
                                        let fmtv = |v: f64| if time { format_kst(v, "%Y-%m-%d %H:%M:%S") } else { format!("{v}") };
                                        a.update(cx, |s, cx| s.set_value(fmtv(lo), window, cx));
                                        b.update(cx, |s, cx| s.set_value(fmtv(hi), window, cx));
                                    }
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
        let mut list = v_flex();
        for c in &schema {
            list = list.child(self.render_column(&c.name, &c.label, c.kind, cx));
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
            .child(div().id("filter-scroll").flex_1().overflow_y_scroll().child(list))
    }
}
