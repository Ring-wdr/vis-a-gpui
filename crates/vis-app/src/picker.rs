//! gpui-kit `Select`를 설정 패널에서 쓰기 쉽게 감싼 것.
//! 선택지는 데이터셋·차트 종류에 따라 바뀌므로, 그릴 때마다 지금 선택지·선택값과 맞춘다(바뀐 경우에만 set_items).

use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{Select, SelectEvent, SelectItem, SelectState};
use gpui_kit::component::{IndexPath, Sizable as _};
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
