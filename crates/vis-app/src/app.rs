//! 앱 뼈대: 상단 바(datastore · gallery · history · 벤치마크), 시작 화면, gallery 탭, history 목록.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, Theme, ThemeMode, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use vis_core::history::History;
use vis_core::store::{DatasetState, Store};

use crate::bench_view::BenchView;
use crate::ui::{fmt_bytes, fmt_int, fmt_ms};
use crate::workspace::{Workspace, WorkspaceEvent};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Start,
    Datastore,
    Gallery,
    History,
    Bench,
}

pub struct VisApp {
    store: Arc<Store>,
    history: Arc<History>,
    section: Section,
    tabs: Vec<Entity<Workspace>>,
    active: usize,
    history_tabs: Vec<(String, Entity<Workspace>)>,
    history_active: Option<usize>,
    bench: Entity<BenchView>,
    working: HashSet<String>,
    poll: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl VisApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let dir = vis_core::store::app_dir();
        let store = Store::open(dir.clone());
        let history = Arc::new(History::open(dir));
        let bench = cx.new(|cx| BenchView::new(store.clone(), window, cx));
        let mut app = Self {
            store,
            history,
            section: Section::Start,
            tabs: vec![],
            active: 0,
            history_tabs: vec![],
            history_active: None,
            bench,
            working: HashSet::new(),
            poll: None,
            _subs: vec![],
        };
        app.new_tab(None, window, cx);
        app.section = Section::Start;
        app
    }

    fn watch(&mut self, ws: &Entity<Workspace>, cx: &mut Context<Self>) {
        self._subs.push(cx.subscribe(ws, |_, _, _e: &WorkspaceEvent, cx| cx.notify()));
    }

    fn new_tab(&mut self, dataset: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let (store, history) = (self.store.clone(), self.history.clone());
        let ws = cx.new(|cx| Workspace::new(store, history, dataset, window, cx));
        self.watch(&ws, cx);
        self.tabs.push(ws);
        self.active = self.tabs.len() - 1;
        self.section = Section::Gallery;
        cx.notify();
    }

    fn close_tab(&mut self, i: usize, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            return;
        }
        self.tabs[i].update(cx, |w, cx| w.cancel(cx));
        self.tabs.remove(i);
        self.active = self.active.min(self.tabs.len() - 1);
        cx.notify();
    }

    fn register(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.working.insert(id.clone()) {
            return;
        }
        let store = self.store.clone();
        let id2 = id.clone();
        std::thread::spawn(move || {
            store.register_sample(&id2);
        });
        self.start_poll(window, cx);
    }

    fn upload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("CSV 파일 열기".into()),
        });
        let store = self.store.clone();
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                this.working.insert(format!("csv:{}", path.display()));
                this.start_poll(window, cx);
            });
            let (tx, rx2) = futures_channel();
            std::thread::spawn(move || {
                let r = store.import_csv(&path, &vis_core::csv_import::CsvOptions::default());
                let _ = tx.send((path, r.map(|d| d.name.clone())));
            });
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                if let Ok((path, r)) = rx2.try_recv() {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.working.remove(&format!("csv:{}", path.display()));
                        match r {
                            Ok(name) => window.push_notification(format!("'{name}'을 등록했습니다."), cx),
                            Err(e) => window.push_notification(format!("가져올 수 없습니다: {e}"), cx),
                        }
                        cx.notify();
                    });
                    break;
                }
            }
        })
        .detach();
    }

    fn start_poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.poll.is_some() {
            return;
        }
        self.poll = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                let busy = this
                    .update(cx, |this, cx| {
                        let store = this.store.clone();
                        this.working.retain(|id| {
                            id.starts_with("csv:") || matches!(store.state(id), DatasetState::Loading(_) | DatasetState::Unregistered)
                        });
                        cx.notify();
                        !this.working.is_empty()
                    })
                    .unwrap_or(false);
                if !busy {
                    break;
                }
            }
            let _ = this.update(cx, |this, _| this.poll = None);
        }));
    }

    /// 지우기 전에 묻는다 (gpui-kit AlertDialog, 위험 동작 버튼, 처음 포커스는 취소)
    fn confirm_delete_dataset(&mut self, id: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let ent = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (ent, id) = (ent.clone(), id.clone());
            alert
                .title(format!("'{name}'을 지울까요?"))
                .description("저장한 데이터 파일을 지웁니다. 샘플은 다시 등록할 수 있습니다.")
                .confirm()
                .ok_text("삭제")
                .cancel_text("취소")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _, cx| {
                    let _ = ent.update(cx, |this, cx| {
                        this.store.delete(&id);
                        cx.notify();
                    });
                    true
                })
        });
    }

    fn confirm_delete_history(&mut self, id: String, title: String, window: &mut Window, cx: &mut Context<Self>) {
        let ent = cx.entity().downgrade();
        let thumb = self.history.thumb_path(&id);
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (ent, id) = (ent.clone(), id.clone());
            alert
                .title("이 기록을 지울까요?")
                .description(
                    v_flex()
                        .gap_2()
                        .when(thumb.exists(), |d| d.child(img(thumb.clone()).w(px(256.)).h(px(160.)).rounded_sm()))
                        .child(title.clone()),
                )
                .confirm()
                .ok_text("삭제")
                .cancel_text("취소")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _, cx| {
                    let _ = ent.update(cx, |this, cx| {
                        this.history.remove(&id);
                        if let Some(i) = this.history_tabs.iter().position(|(h, _)| *h == id) {
                            this.history_tabs.remove(i);
                            this.history_active = None;
                        }
                        cx.notify();
                    });
                    true
                })
        });
    }

    fn open_history(&mut self, id: &str, edit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.history.list().into_iter().find(|e| e.id == id) else {
            return;
        };
        if self.store.get(&entry.spec.dataset).is_none() {
            window.push_notification("이 기록의 데이터가 등록되어 있지 않습니다. datastore에서 등록하세요.", cx);
            return;
        }
        let (store, history) = (self.store.clone(), self.history.clone());
        if edit {
            let ws = cx.new(|cx| Workspace::from_history(store, history, &entry, false, window, cx));
            self.watch(&ws, cx);
            self.tabs.push(ws);
            self.active = self.tabs.len() - 1;
            self.section = Section::Gallery;
        } else {
            if let Some(i) = self.history_tabs.iter().position(|(h, _)| h == id) {
                self.history_active = Some(i);
            } else {
                let ws = cx.new(|cx| Workspace::from_history(store, history, &entry, true, window, cx));
                self.watch(&ws, cx);
                self.history_tabs.push((id.to_string(), ws));
                self.history_active = Some(self.history_tabs.len() - 1);
            }
            self.section = Section::History;
        }
        cx.notify();
    }

    // ── 화면 ─────────────────────────────────────────────────────────

    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let nav = |id: &'static str, label: &'static str, s: Section, cur: Section, cx: &mut Context<Self>| {
            Button::new(id)
                .label(label)
                .small()
                .when(s == cur, |b| b.primary())
                .when(s != cur, |b| b.ghost())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.section = s;
                    cx.notify();
                }))
        };
        let dark = theme.mode.is_dark();
        h_flex()
            .h(px(44.))
            .px_3()
            .gap_1()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.title_bar)
            .child(
                div()
                    .id("logo")
                    .mr_3()
                    .cursor_pointer()
                    .font_weight(FontWeight::BOLD)
                    .child("vis-a-gpui")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.section = Section::Start;
                        cx.notify();
                    })),
            )
            .child(nav("nav-data", "datastore", Section::Datastore, self.section, cx))
            .child(nav("nav-gallery", "gallery", Section::Gallery, self.section, cx))
            .child(nav("nav-history", "history", Section::History, self.section, cx))
            .child(nav("nav-bench", "벤치마크", Section::Bench, self.section, cx))
            .child(div().flex_1())
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("Rust 네이티브 · 렌더 스레드 {}", vis_core::job::default_workers())),
            )
            .child(
                Button::new("theme")
                    .icon(if dark { IconName::Sun } else { IconName::Moon })
                    .small()
                    .ghost()
                    .tooltip("화면 테마 (차트는 다음 draw부터)")
                    .on_click(cx.listener(move |_, _, window, cx| {
                        Theme::change(if dark { ThemeMode::Light } else { ThemeMode::Dark }, Some(window), cx);
                        cx.notify();
                    })),
            )
    }

    fn start_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let ready = self.store.ready().len();
        let hist = self.history.list().len();
        let card = |id: &'static str, title: &'static str, desc: String, s: Section, cx: &mut Context<Self>| {
            v_flex()
                .id(id)
                .w(px(240.))
                .p_4()
                .gap_2()
                .rounded_lg()
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .cursor_pointer()
                .hover(|st| st.bg(theme.list_hover))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.section = s;
                    cx.notify();
                }))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(div().text_sm().text_color(theme.muted_foreground).child(desc))
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_6()
            .child(div().text_2xl().font_weight(FontWeight::BOLD).child("vis-a-lot · Rust 네이티브"))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("브라우저 안의 \"서버\"(PGlite + Render Worker)를 같은 프로세스의 Rust 스레드로 옮긴 데스크톱 판입니다."),
            )
            .child(
                h_flex()
                    .gap_4()
                    .child(card(
                        "c-gallery",
                        "새 차트 그리기",
                        "gallery에서 설정을 고르고 draw".into(),
                        Section::Gallery,
                        cx,
                    ))
                    .child(card(
                        "c-history",
                        "기록 다시 보기",
                        format!("history {hist}건"),
                        Section::History,
                        cx,
                    ))
                    .child(card(
                        "c-data",
                        "데이터 관리",
                        format!("쓸 수 있는 데이터 {ready}개 · 샘플 등록 · CSV"),
                        Section::Datastore,
                        cx,
                    ))
                    .child(card(
                        "c-bench",
                        "성능 측정",
                        "웹(PGlite) 기준값과 같은 케이스 비교".into(),
                        Section::Bench,
                        cx,
                    )),
            )
            .into_any_element()
    }

    fn datastore_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        // 데이터 목록 (gpui-kit Table): 업로드(최근 순) 다음에 샘플
        let mut body = TableBody::new();
        for (info, state) in self.store.list() {
            let id = info.id.clone();
            let (id1, id2, id3) = (id.clone(), id.clone(), id.clone());
            let name = info.name.clone();
            let status: AnyElement = match &state {
                DatasetState::Unregistered => Button::new(SharedString::from(format!("reg-{id}")))
                    .label("등록 (생성)")
                    .small()
                    .primary()
                    .on_click(cx.listener(move |this, _, window, cx| this.register(id1.clone(), window, cx)))
                    .into_any_element(),
                DatasetState::Loading(p) => h_flex()
                    .gap_2()
                    .child(Progress::new(SharedString::from(format!("ld-{id}"))).value(p * 100.0).w(px(120.)))
                    .child(format!("{:.0}%", p * 100.0))
                    .into_any_element(),
                DatasetState::Ready => h_flex()
                    .gap_1()
                    .child(
                        Button::new(SharedString::from(format!("open-{id}")))
                            .label("gallery에서 열기")
                            .small()
                            .primary()
                            .on_click(cx.listener(move |this, _, window, cx| this.new_tab(Some(id2.clone()), window, cx))),
                    )
                    .child(
                        Button::new(SharedString::from(format!("del-{id}")))
                            .icon(IconName::Delete)
                            .small()
                            .ghost()
                            .tooltip("삭제")
                            .on_click(
                                cx.listener(move |this, _, window, cx| this.confirm_delete_dataset(id3.clone(), name.clone(), window, cx)),
                            ),
                    )
                    .into_any_element(),
                DatasetState::Failed(e) => div().text_color(theme.danger).child(e.clone()).into_any_element(),
            };
            body = body.child(
                TableRow::new()
                    .child(
                        TableCell::new().child(v_flex().child(div().text_sm().child(info.name.clone())).child(
                            div().text_xs().text_color(theme.muted_foreground).child(format!(
                                "{}{}",
                                info.id,
                                if info.sample { " · 샘플" } else { " · 업로드" }
                            )),
                        )),
                    )
                    .child(TableCell::new().w(px(120.)).text_right().child(fmt_int(info.rows)))
                    .child(
                        TableCell::new()
                            .w(px(100.))
                            .text_right()
                            .child(if info.bytes > 0 { fmt_bytes(info.bytes) } else { "-".into() }),
                    )
                    .child(TableCell::new().w(px(110.)).text_right().child(if info.load_ms > 0.0 {
                        fmt_ms(info.load_ms)
                    } else {
                        "-".into()
                    }))
                    .child(TableCell::new().w(px(280.)).child(status)),
            );
        }
        let rows = Table::new()
            .child(
                TableHeader::new().child(
                    TableRow::new()
                        .child(TableHead::new().child("이름"))
                        .child(TableHead::new().w(px(120.)).text_right().child("행 수"))
                        .child(TableHead::new().w(px(100.)).text_right().child("크기"))
                        .child(TableHead::new().w(px(110.)).text_right().child("적재 시간"))
                        .child(TableHead::new().w(px(280.)).child("상태")),
                ),
            )
            .child(body);
        let csv_busy = self.working.iter().any(|w| w.starts_with("csv:"));
        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("datastore"))
                    .child(div().flex_1())
                    .child(
                        Button::new("upload")
                            .label(if csv_busy { "가져오는 중…" } else { "upload (CSV)" })
                            .small()
                            .outline()
                            .loading(csv_busy)
                            .disabled(csv_busy)
                            .on_click(cx.listener(|this, _, window, cx| this.upload(window, cx))),
                    ),
            )
            .child(div().text_xs().text_color(theme.muted_foreground).child(format!(
                "샘플은 시드로 생성합니다(원본 생성기와 같은 규칙). 저장 위치 {} · 합계 {}",
                self.store.dir().display(),
                fmt_bytes(self.store.total_bytes())
            )))
            .child(div().id("ds-scroll").flex_1().overflow_y_scroll().child(rows))
            .into_any_element()
    }

    fn gallery_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut bar = TabBar::new("gallery-tabs")
            .selected_index(self.active)
            .on_click(cx.listener(|this, i: &usize, _, cx| {
                this.active = *i;
                cx.notify();
            }));
        let closable = self.tabs.len() > 1;
        for (i, ws) in self.tabs.iter().enumerate() {
            let title = ws.read(cx).title();
            let running = ws.read(cx).is_running();
            bar = bar.child(
                Tab::new()
                    .label(if running { format!("◐ {title}") } else { title })
                    .when(closable, |t| {
                        t.suffix(
                            Button::new(SharedString::from(format!("close-{i}")))
                                .icon(IconName::Close)
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.close_tab(i, cx);
                                })),
                        )
                    }),
            );
        }
        let bar = bar.suffix(
            Button::new("new-tab")
                .icon(IconName::Plus)
                .xsmall()
                .ghost()
                .tooltip("새 탭")
                .on_click(cx.listener(|this, _, window, cx| {
                    let ds = this.tabs.get(this.active).map(|w| w.read(cx).spec.dataset.clone());
                    this.new_tab(ds, window, cx)
                })),
        );
        v_flex()
            .size_full()
            .child(div().border_b_1().border_color(theme.border).child(bar))
            .child(div().flex_1().min_h_0().children(self.tabs.get(self.active).cloned()))
            .into_any_element()
    }

    fn history_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut list = v_flex().gap_2().p_2();
        let entries = self.history.list();
        if entries.is_empty() {
            list = list.child(
                Empty::new().header(
                    EmptyHeader::new()
                        .media(
                            EmptyMedia::new()
                                .with_variant(EmptyMediaVariant::Icon)
                                .child(Icon::new(IconName::Inbox)),
                        )
                        .title(EmptyTitle::new().child("기록 없음"))
                        .description(EmptyDescription::new().child("gallery에서 draw하면 여기에 남습니다.")),
                ),
            );
        }
        for e in entries {
            let (id1, id2, id3) = (e.id.clone(), e.id.clone(), e.id.clone());
            let summary = e.spec.summary();
            let thumb = self.history.thumb_path(&e.id);
            let when = vis_core::timefmt::format_kst(e.created_ms as f64, "%m/%d %H:%M");
            let active = self
                .history_active
                .and_then(|i| self.history_tabs.get(i))
                .is_some_and(|(h, _)| *h == e.id);
            list = list.child(
                v_flex()
                    .id(SharedString::from(format!("h-{}", e.id)))
                    .p_2()
                    .gap_1()
                    .rounded_md()
                    .border_1()
                    .border_color(if active { theme.primary } else { theme.border })
                    .bg(theme.background)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.list_hover))
                    .on_click(cx.listener(move |this, _, window, cx| this.open_history(&id1, false, window, cx)))
                    .when(thumb.exists(), |d| d.child(img(thumb.clone()).w(px(232.)).h(px(145.)).rounded_sm()))
                    .child(div().text_sm().child(e.spec.summary()))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(format!(
                        "{when} · item {} · {} · {}",
                        fmt_int(e.item_count),
                        fmt_ms(e.draw_ms),
                        e.status
                    )))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new(SharedString::from(format!("he-{}", e.id)))
                                    .label("gallery에서 편집")
                                    .xsmall()
                                    .outline()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.open_history(&id2, true, window, cx)
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!("hd-{}", e.id)))
                                    .icon(IconName::Delete)
                                    .xsmall()
                                    .ghost()
                                    .tooltip("삭제")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.confirm_delete_history(id3.clone(), summary.clone(), window, cx);
                                    })),
                            ),
                    ),
            );
        }
        let body = match self.history_active.and_then(|i| self.history_tabs.get(i)) {
            Some((_, ws)) => div().flex_1().min_w_0().h_full().child(ws.clone()).into_any_element(),
            None => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("왼쪽 목록에서 기록을 고르면 다시 보기 전용으로 엽니다.")
                .into_any_element(),
        };
        h_flex()
            .size_full()
            .child(
                div()
                    .id("hist-scroll")
                    .w(px(264.))
                    .h_full()
                    .flex_shrink_0()
                    .overflow_y_scroll()
                    .border_r_1()
                    .border_color(theme.border)
                    .bg(theme.sidebar)
                    .child(list),
            )
            .child(body)
            .into_any_element()
    }
}

impl Render for VisApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let body = match self.section {
            Section::Start => self.start_view(cx),
            Section::Datastore => self.datastore_view(cx),
            Section::Gallery => self.gallery_view(cx),
            Section::History => self.history_view(cx),
            Section::Bench => div().size_full().child(self.bench.clone()).into_any_element(),
        };
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(".SystemUIFont")
            .child(self.top_bar(cx))
            .child(div().flex_1().min_h_0().child(body))
    }
}

/// 스레드에서 결과 하나를 받는 채널 (std mpsc를 비동기 폴링으로 읽는다)
fn futures_channel<T>() -> (std::sync::mpsc::Sender<T>, std::sync::mpsc::Receiver<T>) {
    std::sync::mpsc::channel()
}
