//! gpui-kit `Select`를 설정 패널에서 쓰기 쉽게 감싼 것.
//! 선택지는 데이터셋·차트 종류에 따라 바뀌므로, 그릴 때마다 지금 선택지·선택값과 맞춘다(바뀐 경우에만 set_items).

use gpui_kit::component::combobox::{Combobox, ComboboxEvent, ComboboxState};
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{Select, SelectEvent, SelectItem, SelectState};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, IndexPath, Sizable as _, h_flex};
use vis_core::spec::{CHART_TYPES, ChartType};

use crate::assets::chart_icon;
use gpui_kit::*;

#[derive(Clone, Debug, PartialEq)]
pub struct Opt {
    pub title: SharedString,
    pub value: String,
}

impl SelectItem for Opt {
    type Value = String;
    fn title(&self) -> SharedString {
        self.title.clone()
    }
    fn value(&self) -> &String {
        &self.value
    }
}

type State = SelectState<SearchableVec<Opt>>;

pub struct Picker {
    pub state: Entity<State>,
    opts: Vec<Opt>,
    selected: Option<String>,
    placeholder: SharedString,
}

impl Picker {
    /// on_pick: 사용자가 고른 값 (Select가 확정했을 때만)
    pub fn new<V: 'static>(
        placeholder: impl Into<SharedString>,
        searchable: bool,
        window: &mut Window,
        cx: &mut Context<V>,
        on_pick: impl Fn(&mut V, String, &mut Window, &mut Context<V>) + 'static,
    ) -> (Self, Subscription) {
        let state = cx.new(|cx| SelectState::new(SearchableVec::new(Vec::<Opt>::new()), None, window, cx).searchable(searchable));
        let sub = cx.subscribe_in(&state, window, move |this, _, e: &SelectEvent<SearchableVec<Opt>>, window, cx| {
            if let SelectEvent::Confirm(Some(v)) = e {
                on_pick(this, v.clone(), window, cx);
            }
        });
        (
            Self {
                state,
                opts: vec![],
                selected: None,
                placeholder: placeholder.into(),
            },
            sub,
        )
    }

    /// 선택지·선택값을 맞춘다
    pub fn sync(&mut self, opts: Vec<(String, String)>, selected: Option<String>, window: &mut Window, cx: &mut App) {
        let opts: Vec<Opt> = opts
            .into_iter()
            .map(|(title, value)| Opt {
                title: title.into(),
                value,
            })
            .collect();
        if opts != self.opts {
            self.opts = opts.clone();
            self.selected = None;
            self.state.update(cx, |s, cx| s.set_items(SearchableVec::new(opts), window, cx));
        }
        if selected != self.selected {
            self.selected = selected.clone();
            let ix = selected.as_ref().and_then(|v| self.opts.iter().position(|o| &o.value == v));
            self.state.update(cx, |s, cx| {
                s.set_selected_index(ix.map(|i| IndexPath::default().row(i)), window, cx)
            });
        }
    }

    pub fn element(&self) -> Select<SearchableVec<Opt>> {
        Select::new(&self.state).small().placeholder(self.placeholder.clone()).w_full()
    }
}

// ── 차트 종류: 원본처럼 아이콘이 붙은 Select ─────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct ChartOpt {
    pub ty: ChartType,
    pub value: String,
}

impl SelectItem for ChartOpt {
    type Value = String;
    fn title(&self) -> SharedString {
        self.ty.label().into()
    }
    fn value(&self) -> &String {
        &self.value
    }
    fn display_title(&self) -> Option<AnyElement> {
        Some(
            h_flex()
                .gap_2()
                .child(chart_icon(self.ty).small())
                .child(self.ty.label())
                .into_any_element(),
        )
    }
    fn render(&self, _: &mut Window, _: &mut App) -> impl IntoElement {
        h_flex().gap_2().child(chart_icon(self.ty)).child(self.ty.label())
    }
}

pub struct ChartPicker {
    pub state: Entity<SelectState<SearchableVec<ChartOpt>>>,
    selected: Option<ChartType>,
}

impl ChartPicker {
    pub fn new<V: 'static>(
        window: &mut Window,
        cx: &mut Context<V>,
        on_pick: impl Fn(&mut V, ChartType, &mut Window, &mut Context<V>) + 'static,
    ) -> (Self, Subscription) {
        let opts: Vec<ChartOpt> = CHART_TYPES
            .iter()
            .map(|&ty| ChartOpt {
                ty,
                value: ty.name().to_string(),
            })
            .collect();
        let state = cx.new(|cx| SelectState::new(SearchableVec::new(opts), None, window, cx).searchable(true));
        let sub = cx.subscribe_in(
            &state,
            window,
            move |this, _, e: &SelectEvent<SearchableVec<ChartOpt>>, window, cx| {
                if let SelectEvent::Confirm(Some(v)) = e {
                    if let Some(t) = CHART_TYPES.iter().find(|t| t.name() == v) {
                        on_pick(this, *t, window, cx);
                    }
                }
            },
        );
        (Self { state, selected: None }, sub)
    }

    pub fn sync(&mut self, ty: ChartType, window: &mut Window, cx: &mut App) {
        if self.selected != Some(ty) {
            self.selected = Some(ty);
            let ix = CHART_TYPES.iter().position(|t| *t == ty);
            self.state.update(cx, |s, cx| {
                s.set_selected_index(ix.map(|i| IndexPath::default().row(i)), window, cx)
            });
        }
    }

    pub fn element(&self) -> Select<SearchableVec<ChartOpt>> {
        Select::new(&self.state).small().w_full().menu_max_h(px(420.))
    }
}

// ── split by · tooltip: 누른 순서를 지키는 다중 선택 (원본 OrderedSelect → gpui-kit Combobox) ──

type ComboState = ComboboxState<SearchableVec<Opt>>;

pub struct MultiPicker {
    pub state: Entity<ComboState>,
    opts: Vec<Opt>,
    selected: Vec<String>,
    placeholder: SharedString,
    numbered: bool,
}

impl MultiPicker {
    /// on_change: 고른 값 (누른 순서)
    pub fn new<V: 'static>(
        placeholder: impl Into<SharedString>,
        numbered: bool,
        window: &mut Window,
        cx: &mut Context<V>,
        on_change: impl Fn(&mut V, Vec<String>, &mut Window, &mut Context<V>) + 'static,
    ) -> (Self, Subscription) {
        let state = cx.new(|cx| {
            ComboboxState::new(SearchableVec::new(Vec::<Opt>::new()), vec![], window, cx)
                .multiple(true)
                .searchable(true)
        });
        let sub = cx.subscribe_in(&state, window, move |this, _, e: &ComboboxEvent<SearchableVec<Opt>>, window, cx| {
            if let ComboboxEvent::Change(v) = e {
                on_change(this, v.clone(), window, cx);
            }
        });
        (
            Self {
                state,
                opts: vec![],
                selected: vec![],
                placeholder: placeholder.into(),
                numbered,
            },
            sub,
        )
    }

    pub fn sync(&mut self, opts: Vec<(String, String)>, selected: Vec<String>, window: &mut Window, cx: &mut App) {
        let opts: Vec<Opt> = opts
            .into_iter()
            .map(|(title, value)| Opt {
                title: title.into(),
                value,
            })
            .collect();
        if opts != self.opts {
            self.opts = opts.clone();
            self.selected.clear();
            self.state.update(cx, |s, cx| s.set_items(SearchableVec::new(opts), window, cx));
        }
        if selected != self.selected {
            self.selected = selected.clone();
            self.state.update(cx, |s, cx| s.set_selected_values(&selected, window, cx));
        }
    }

    pub fn element(&self) -> Combobox<SearchableVec<Opt>> {
        let numbered = self.numbered;
        let placeholder = self.placeholder.clone();
        Combobox::new(&self.state)
            .small()
            .w_full()
            .cleanable(true)
            .render_trigger(move |ctx, _, cx| {
                let sel = ctx.selection();
                if sel.is_empty() {
                    return div()
                        .text_color(cx.theme().muted_foreground)
                        .child(placeholder.clone())
                        .into_any_element();
                }
                // 고른 순서대로 Tag. split은 순번을 붙인다 (item 이름 = 이 순서로 이은 값)
                h_flex()
                    .flex_wrap()
                    .gap_1()
                    .children(sel.iter().enumerate().map(|(i, (_, o))| {
                        let label = o.title.split(" (").next().unwrap_or("").to_string();
                        Tag::primary()
                            .outline()
                            .small()
                            .child(if numbered { format!("{} {label}", i + 1) } else { label })
                    }))
                    .into_any_element()
            })
    }
}
