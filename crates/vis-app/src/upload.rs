//! 업로드(파일 열기) 팝업 (원본 3.5). 위 `파일 선택`·`검사`, 왼쪽 파일 목록(zip이면 안의 CSV들),
//! 가운데 원본 앞부분, 오른쪽 전처리 옵션, 아래 미리보기 표(머리 첫 줄 = 컬럼 포함 체크, 둘째 줄 = 타입).
//! 옵션을 바꾸면 300ms 뒤 앞부분(1MB, 100행)만 다시 읽는다. `검사`는 파일 전체를 읽어 행 수와 변환 불가 값 수를 센다.
//! `수정`은 같은 팝업을 수정 모드로 열어 보관한 원본을 저장한 옵션으로 보여 주고 `다시 적재`한다.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _, Toggle, ToggleGroup};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use vis_core::csv_import::{CsvOptions, Encoding, Inspect, Preview, convertible, inspect, preview};
use vis_core::dataset::ColumnKind;
use vis_core::store::Store;

use crate::assets::icon;

const DELIMS: [(u8, &str); 4] = [(b',', "쉼표"), (b'\t', "탭"), (b';', "세미콜론"), (b'|', "세로 막대")];
const ENCODINGS: [(Encoding, &str); 3] = [(Encoding::Auto, "자동"), (Encoding::Utf8, "UTF-8"), (Encoding::EucKr, "EUC-KR")];

fn kind_label(k: ColumnKind) -> &'static str {
    match k {
        ColumnKind::Categorical => "범주",
        ColumnKind::Numeric => "숫자",
        ColumnKind::Datetime => "시계열",
    }
}

pub struct UploadPanel {
    store: Arc<Store>,
    /// 수정 모드면 데이터셋 id
    edit: Option<String>,
    path: Option<PathBuf>,
    opts: CsvOptions,
    name: Entity<InputState>,
    null_value: Entity<InputState>,
    custom_delim: Entity<InputState>,
    quote: Entity<InputState>,
    preview: Option<Result<Preview, String>>,
    inspected: Option<Result<Inspect, String>>,
    busy: Option<&'static str>,
    status: Option<Result<String, String>>,
    debounce: Option<Task<()>>,
    on_done: Arc<dyn Fn(String, &mut Window, &mut App)>,
    _subs: Vec<Subscription>,
}

impl UploadPanel {
    pub fn new(
        store: Arc<Store>,
        edit: Option<String>,
        on_done: Arc<dyn Fn(String, &mut Window, &mut App)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let info = edit.as_ref().and_then(|id| store.info(id));
        let opts = info.as_ref().and_then(|i| i.options.clone()).unwrap_or_default();
        let path = edit.as_ref().and_then(|id| store.source_path(id));
        let init_name = if opts.name.is_empty() {
            info.as_ref().map(|i| i.name.clone()).unwrap_or_default()
        } else {
            opts.name.clone()
        };
        let mk = |v: String, ph: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).default_value(v).placeholder(ph))
        };
        let name = mk(init_name, "비우면 파일 이름", window, cx);
        let null_value = mk(opts.null_value.clone(), "기본: 빈 값", window, cx);
        let custom = if DELIMS.iter().any(|d| d.0 == opts.delimiter) {
            String::new()
        } else {
            (opts.delimiter as char).to_string()
        };
        let custom_delim = mk(custom, "한 글자", window, cx);
        let quote = mk((opts.quote as char).to_string(), "\"", window, cx);
        let mut _subs = vec![];
        for (k, s) in [&null_value, &custom_delim, &quote].into_iter().enumerate() {
            _subs.push(cx.subscribe_in(s, window, move |this: &mut Self, st, e: &InputEvent, window, cx| {
                if !matches!(e, InputEvent::Change) {
                    return;
                }
                let v = st.read(cx).value().to_string();
                match k {
                    0 => this.opts.null_value = v,
                    1 => {
                        if let Some(c) = v.chars().next().filter(|c| c.is_ascii()) {
                            this.opts.delimiter = c as u8;
                        }
                    }
                    _ => {
                        if let Some(c) = v.chars().next().filter(|c| c.is_ascii()) {
                            this.opts.quote = c as u8;
                        }
                    }
                }
                this.changed(window, cx);
            }));
        }
        let mut this = Self {
            store,
            edit,
            path,
            opts,
            name,
            null_value,
            custom_delim,
            quote,
            preview: None,
            inspected: None,
            busy: None,
            status: None,
            debounce: None,
            on_done,
            _subs,
        };
        this.refresh(window, cx);
        this
    }

    /// 옵션이 바뀌면 300ms 뒤 앞부분만 다시 읽는다
    fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.inspected = None;
        self.debounce = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(300)).await;
            let _ = this.update_in(cx, |this, window, cx| this.refresh(window, cx));
        }));
        cx.notify();
    }

    fn refresh(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else { return };
        self.preview = Some(preview(&path, &self.opts));
        cx.notify();
    }

    fn pick_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("CSV 또는 zip 열기".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                this.path = Some(path);
                this.opts.types.clear();
                this.opts.exclude.clear();
                this.inspected = None;
                this.status = None;
                this.refresh(window, cx);
            });
        })
        .detach();
    }

    /// 백그라운드에서 f를 돌리고 끝나면 done
    fn run<T: Send + 'static>(
        &mut self,
        label: &'static str,
        f: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.busy = Some(label);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(80)).await;
                if let Ok(v) = rx.try_recv() {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.busy = None;
                        done(this, v, window, cx);
                        cx.notify();
                    });
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn inspect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else { return };
        let opts = self.opts.clone();
        self.run(
            "검사 중…",
            move || inspect(&path, &opts),
            |this, r, _, _| this.inspected = Some(r),
            window,
            cx,
        );
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else { return };
        let mut opts = self.opts.clone();
        opts.name = self.name.read(cx).value().trim().to_string();
        let store = self.store.clone();
        let edit = self.edit.clone();
        self.status = None;
        self.run(
            if edit.is_some() { "다시 적재 중…" } else { "적재 중…" },
            move || match &edit {
                Some(id) => store.reimport(id, &opts).map(|d| d.name.clone()),
                None => store.import_csv(&path, &opts).map(|d| d.name.clone()),
            },
            |this, r, window, cx| match r {
                Ok(name) => {
                    this.status = Some(Ok(format!("'{name}'을(를) 적재했습니다.")));
                    (this.on_done)(name, window, cx);
                    window.close_dialog(cx);
                }
                // 거절되면 사유를 보이고 옵션을 고쳐 다시 확인할 수 있다 (수정이면 기존 데이터는 그대로)
                Err(e) => this.status = Some(Err(e)),
            },
            window,
            cx,
        );
    }

    pub fn open(panel: Entity<UploadPanel>, window: &mut Window, cx: &mut App) {
        let editing = panel.read(cx).edit.is_some();
        window.open_dialog(cx, move |dialog, _, cx| {
            let p = panel.read(cx);
            let ready = p.path.is_some() && p.busy.is_none() && p.preview.as_ref().is_some_and(|r| r.is_ok());
            let (p1, p2, p3) = (panel.clone(), panel.clone(), panel.clone());
            dialog
                .title(if editing {
                    "데이터 수정 (다시 적재)"
                } else {
                    "데이터 올리기 (CSV · zip)"
                })
                .width(px(1120.))
                .child(panel.clone())
                .footer(
                    DialogFooter::new()
                        .gap_2()
                        .when(!editing, |f| {
                            f.child(
                                Button::new("up-pick")
                                    .icon(icon(Lucide::FileUp))
                                    .label("파일 선택")
                                    .outline()
                                    .on_click(move |_, window, cx| p1.update(cx, |p, cx| p.pick_file(window, cx))),
                            )
                        })
                        .child(
                            Button::new("up-inspect")
                                .icon(icon(Lucide::ScanSearch))
                                .label("검사")
                                .outline()
                                .disabled(!ready)
                                .on_click(move |_, window, cx| p2.update(cx, |p, cx| p.inspect(window, cx))),
                        )
                        .child(div().flex_1())
                        .child(
                            Button::new("up-cancel")
                                .label("취소")
                                .outline()
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("up-ok")
                                .icon(icon(Lucide::Check))
                                .label(if editing { "다시 적재" } else { "확인 (적재)" })
                                .primary()
                                .disabled(!ready)
                                .on_click(move |_, window, cx| p3.update(cx, |p, cx| p.confirm(window, cx))),
                        ),
                )
        });
    }

    fn options_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let label = |s: &str| div().text_xs().text_color(theme.muted_foreground).child(s.to_string());
        let cur_delim = DELIMS.iter().position(|d| d.0 == self.opts.delimiter);
        let mut dg = ToggleGroup::new("up-delim").xsmall().segmented();
        for (k, (_, l)) in DELIMS.iter().enumerate() {
            dg = dg.child(
                Toggle::new(SharedString::from(format!("d-{k}")))
                    .label(*l)
                    .checked(cur_delim == Some(k)),
            );
        }
        let dg = dg.on_click(cx.listener(move |this, checked: &Vec<bool>, window, cx| {
            if let Some(k) = checked.iter().enumerate().position(|(k, on)| *on && Some(k) != cur_delim) {
                this.opts.delimiter = DELIMS[k].0;
                this.changed(window, cx);
            }
        }));
        let cur_enc = ENCODINGS.iter().position(|e| e.0 == self.opts.encoding);
        let mut eg = ToggleGroup::new("up-enc").xsmall().segmented();
        for (k, (_, l)) in ENCODINGS.iter().enumerate() {
            eg = eg.child(
                Toggle::new(SharedString::from(format!("e-{k}")))
                    .label(*l)
                    .checked(cur_enc == Some(k)),
            );
        }
        let eg = eg.on_click(cx.listener(move |this, checked: &Vec<bool>, window, cx| {
            if let Some(k) = checked.iter().enumerate().position(|(k, on)| *on && Some(k) != cur_enc) {
                this.opts.encoding = ENCODINGS[k].0;
                this.changed(window, cx);
            }
        }));
        v_flex()
            .w(px(280.))
            .flex_shrink_0()
            .gap_2()
            .child(label("데이터 이름"))
            .child(Input::new(&self.name).small())
            .child(label("구분 기호"))
            .child(dg)
            .child(
                h_flex()
                    .gap_2()
                    .child(label("직접 입력"))
                    .child(Input::new(&self.custom_delim).small().w(px(60.))),
            )
            .child(
                Switch::new("up-header")
                    .small()
                    .checked(self.opts.header)
                    .label("첫 번째 행 헤더 (아니면 column_1…)")
                    .on_click(cx.listener(|this, v: &bool, window, cx| {
                        this.opts.header = *v;
                        this.changed(window, cx);
                    })),
            )
            .child(label("Null 값 (이 값과 같은 필드를 null로)"))
            .child(Input::new(&self.null_value).small())
            .child(
                h_flex()
                    .gap_2()
                    .child(label("따옴표"))
                    .child(Input::new(&self.quote).small().w(px(60.))),
            )
            .child(label("인코딩 (자동: UTF-8, 깨지면 EUC-KR)"))
            .child(eg)
            .child(
                Switch::new("up-ignore")
                    .small()
                    .checked(self.opts.ignore_errors)
                    .label("데이터 오류 무시")
                    .on_click(cx.listener(|this, v: &bool, window, cx| {
                        this.opts.ignore_errors = *v;
                        this.changed(window, cx);
                    })),
            )
            .into_any_element()
    }

    fn table_view(&self, p: &Preview, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let ins = self.inspected.as_ref().and_then(|r| r.as_ref().ok());
        // 검사한 옵션 그대로면 '자동'도 전체 기준 추론
        let auto_of = |c: usize| ins.and_then(|i| i.inferred.get(c).copied()).unwrap_or(p.inferred[c]);
        let w = px(150.);
        let all_on = self.opts.exclude.is_empty();
        let mut head1 = h_flex().child(
            div()
                .w(px(40.))
                .flex_shrink_0()
                .child(Checkbox::new("inc-all").checked(all_on).tooltip("전체 포함").on_click(cx.listener(
                    move |this, v: &bool, _, cx| {
                        if *v {
                            this.opts.exclude.clear();
                        } else if let Some(Ok(p)) = &this.preview {
                            this.opts.exclude = p.header.clone();
                        }
                        cx.notify();
                    },
                ))),
        );
        let mut head2 = h_flex().child(div().w(px(40.)).flex_shrink_0().text_xs().child("타입"));
        for (c, h) in p.header.iter().enumerate() {
            let included = !self.opts.exclude.contains(h);
            let (h1, h2) = (h.clone(), h.clone());
            head1 = head1.child(
                div().w(w).flex_shrink_0().px_1().child(
                    Checkbox::new(SharedString::from(format!("inc-{c}")))
                        .label(h.clone())
                        .checked(included)
                        .on_click(cx.listener(move |this, v: &bool, _, cx| {
                            this.opts.exclude.retain(|x| *x != h1);
                            if !*v {
                                this.opts.exclude.push(h1.clone());
                            }
                            cx.notify();
                        })),
                ),
            );
            // 타입: 누를 때마다 자동 → 범주 → 숫자 → 시계열
            let set = self.opts.types.get(h).copied();
            let text = match set {
                None => format!("자동 ({})", kind_label(auto_of(c))),
                Some(k) => kind_label(k).to_string(),
            };
            head2 = head2.child(
                div().w(w).flex_shrink_0().px_1().child(
                    Button::new(SharedString::from(format!("ty-{c}")))
                        .label(text)
                        .xsmall()
                        .outline()
                        .tooltip("누를 때마다 자동 → 범주 → 숫자 → 시계열")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let next = match this.opts.types.get(&h2) {
                                None => Some(ColumnKind::Categorical),
                                Some(ColumnKind::Categorical) => Some(ColumnKind::Numeric),
                                Some(ColumnKind::Numeric) => Some(ColumnKind::Datetime),
                                Some(ColumnKind::Datetime) => None,
                            };
                            match next {
                                Some(k) => this.opts.types.insert(h2.clone(), k),
                                None => this.opts.types.remove(&h2),
                            };
                            this.changed(window, cx);
                        })),
                ),
            );
        }
        let mut body = v_flex();
        for (r, row) in p.rows.iter().enumerate().take(100) {
            let mut line = h_flex().text_xs().child(
                div()
                    .w(px(40.))
                    .flex_shrink_0()
                    .text_color(theme.muted_foreground)
                    .child(format!("{}", r + 1)),
            );
            for (c, v) in row.iter().enumerate() {
                let kind = self.opts.types.get(&p.header[c]).copied().unwrap_or(auto_of(c));
                let ok = convertible(v, kind, &self.opts);
                let excluded = self.opts.exclude.contains(&p.header[c]);
                line = line.child(
                    div()
                        .w(w)
                        .flex_shrink_0()
                        .px_1()
                        .truncate()
                        .when(!ok, |d| d.text_color(theme.danger))
                        .when(excluded, |d| d.opacity(0.35))
                        .child(v.clone()),
                );
            }
            body = body.child(line);
        }
        v_flex()
            .id("up-table")
            .h(px(260.))
            .overflow_scroll()
            .border_1()
            .border_color(theme.border)
            .rounded_md()
            .child(head1.border_b_1().border_color(theme.border).py_1())
            .child(head2.border_b_1().border_color(theme.border).py_1())
            .child(body)
            .into_any_element()
    }
}

impl Render for UploadPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let files = match &self.preview {
            Some(Ok(p)) => p.files.clone(),
            _ => vec![],
        };
        let mut file_list = v_flex()
            .w(px(180.))
            .flex_shrink_0()
            .gap_1()
            .child(div().text_xs().text_color(theme.muted_foreground).child("파일"));
        match &self.path {
            None => {
                file_list = file_list.child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("파일 선택으로 CSV 또는 zip을 고르세요."),
                )
            }
            Some(path) => {
                // 수정 모드는 보관한 원본(…src) 대신 올렸을 때 파일 이름
                let shown = self
                    .edit
                    .as_ref()
                    .and_then(|id| self.store.info(id))
                    .and_then(|i| i.source)
                    .unwrap_or_else(|| path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default());
                file_list = file_list.child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(shown));
                for f in files.iter().filter(|_| files.len() > 1) {
                    file_list = file_list.child(div().text_xs().pl_2().child(f.clone()));
                }
            }
        }
        let raw = match &self.preview {
            Some(Ok(p)) => p.raw.clone(),
            Some(Err(e)) => format!("읽을 수 없습니다: {e}"),
            None => String::new(),
        };
        let top = h_flex()
            .items_start()
            .gap_3()
            .h(px(260.))
            .child(div().id("up-files").h_full().overflow_y_scroll().child(file_list))
            .child(
                div()
                    .id("up-raw")
                    .flex_1()
                    .h_full()
                    .overflow_scroll()
                    .p_2()
                    .rounded_md()
                    .bg(theme.muted)
                    .font_family("monospace")
                    .text_xs()
                    .child(raw),
            )
            .child(div().id("up-opts").h_full().overflow_y_scroll().child(self.options_view(cx)));
        let table = match &self.preview {
            Some(Ok(p)) => {
                let p = Preview {
                    files: vec![],
                    raw: String::new(),
                    header: p.header.clone(),
                    rows: p.rows.clone(),
                    inferred: p.inferred.clone(),
                };
                self.table_view(&p, cx)
            }
            _ => div().h(px(260.)).into_any_element(),
        };
        let mut status = h_flex().gap_3().text_sm().min_h(px(24.));
        if let Some(b) = self.busy {
            status = status.child(div().text_color(theme.muted_foreground).child(b));
        }
        if let Some(Ok(i)) = &self.inspected {
            let bad: usize = i.bad_values.iter().sum();
            status = status.child(format!(
                "검사: {}행 · 읽지 못한 행 {} · 변환 불가 값 {}{}",
                vis_core::render::charts_fmt_int(i.rows),
                i.bad_rows,
                bad,
                if bad > 0 {
                    format!(
                        " ({})",
                        i.header
                            .iter()
                            .zip(&i.bad_values)
                            .filter(|(_, n)| **n > 0)
                            .map(|(h, n)| format!("{h} {n}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                } else {
                    String::new()
                }
            ));
        }
        if let Some(Err(e)) = &self.inspected {
            status = status.child(div().text_color(theme.danger).child(format!("검사 실패: {e}")));
        }
        match &self.status {
            Some(Ok(s)) => status = status.child(s.clone()),
            Some(Err(e)) => status = status.child(div().text_color(theme.danger).child(format!("거절: {e}"))),
            None => {}
        }
        v_flex().gap_3().child(top).child(table).child(status)
    }
}
