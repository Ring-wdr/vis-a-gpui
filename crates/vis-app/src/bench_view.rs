//! 벤치마크 화면. vis-a-lot `pnpm bench:db`와 같은 케이스를 이 PC에서 재고, 웹(PGlite) 기준값과 나란히 보인다.
//! 기준값은 docs/bench/web-pglite-*.json (같은 데이터·같은 케이스를 Node 22 + PGlite로 잰 값)이다.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;

use vis_core::bench::{self, CaseResult, DrawResult};
use vis_core::job::default_workers;
use vis_core::store::Store;

use crate::ui::{fmt_int, fmt_ms};

const WEB_100K: &str = include_str!("../../../docs/bench/web-pglite-100000.json");
const WEB_1M: &str = include_str!("../../../docs/bench/web-pglite-1000000.json");

enum Msg {
    Status(String),
    Case(CaseResult),
    Draw(DrawResult),
    Done,
}

pub struct BenchView {
    store: Arc<Store>,
    rows: usize,
    running: bool,
    status: String,
    cases: Vec<CaseResult>,
    draws: Vec<DrawResult>,
    gen_ms: Option<f64>,
    rx: Option<Receiver<Msg>>,
    poll: Option<Task<()>>,
}

fn web_baseline(rows: usize) -> (f64, Vec<(String, f64)>) {
    let v: Value = serde_json::from_str(if rows == 100_000 { WEB_100K } else { WEB_1M }).unwrap_or(Value::Null);
    let load = v["dataset"]["loadMs"].as_f64().unwrap_or(0.0);
    let list = v["results"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| (r["name"].as_str().unwrap_or("").to_string(), r["medianMs"].as_f64().unwrap_or(0.0)))
                .collect()
        })
        .unwrap_or_default();
    (load, list)
}

impl BenchView {
    pub fn new(store: Arc<Store>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            store,
            rows: 100_000,
            running: false,
            status: String::new(),
            cases: vec![],
            draws: vec![],
            gen_ms: None,
            rx: None,
            poll: None,
        }
    }

    fn run(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        self.running = true;
        self.cases.clear();
        self.draws.clear();
        self.gen_ms = None;
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let rows = self.rows;
        let store = self.store.clone();
        std::thread::spawn(move || {
            let id = if rows == 100_000 { "iot-100k" } else { "iot-1m" };
            let _ = tx.send(Msg::Status(format!("{id} 데이터 생성 중…")));
            let (ds, gen_ms) = bench::generate_sample(id);
            let _ = tx.send(Msg::Status(format!("생성 {gen_ms:.0} ms · 데이터 케이스 측정 중…")));
            let _ = tx.send(Msg::Case(CaseResult {
                name: "__generate".into(),
                median_ms: gen_ms,
                p95_ms: gen_ms,
                min_ms: gen_ms,
                runs: 1,
            }));
            bench::data_cases(&ds, 5, |r| {
                let _ = tx.send(Msg::Case(r.clone()));
            });
            let _ = tx.send(Msg::Status("draw 시나리오 측정 중…".into()));
            for s in bench::draw_scenarios(id) {
                let _ = bench::run_draw(&ds, &s.spec, default_workers(), (400.0, 300.0), false);
                let mut r = bench::run_draw(&ds, &s.spec, default_workers(), (400.0, 300.0), false);
                r.name = s.name.into();
                let _ = tx.send(Msg::Draw(r));
            }
            drop(store);
            let _ = tx.send(Msg::Done);
        });
        self.poll = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                let keep = this
                    .update(cx, |this, cx| {
                        let mut keep = true;
                        if let Some(rx) = &this.rx {
                            while let Ok(m) = rx.try_recv() {
                                match m {
                                    Msg::Status(s) => this.status = s,
                                    Msg::Case(c) if c.name == "__generate" => this.gen_ms = Some(c.median_ms),
                                    Msg::Case(c) => this.cases.push(c),
                                    Msg::Draw(d) => this.draws.push(d),
                                    Msg::Done => {
                                        this.running = false;
                                        this.status = "완료".into();
                                        keep = false;
                                    }
                                }
                            }
                        }
                        cx.notify();
                        keep
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        }));
    }
}

impl Render for BenchView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let (web_load, web) = web_baseline(self.rows);
        let row = |a: String, b: String, c: String, d: String, head: bool| {
            h_flex()
                .px_2()
                .py_1()
                .text_sm()
                .when(head, |r| r.text_xs().text_color(theme.muted_foreground))
                .child(div().flex_1().child(a))
                .child(div().w(px(140.)).text_right().child(b))
                .child(div().w(px(140.)).text_right().child(c))
                .child(div().w(px(100.)).text_right().font_weight(FontWeight::SEMIBOLD).child(d))
        };
        let mut table = v_flex().border_1().border_color(theme.border).rounded_md().child(row(
            "케이스".into(),
            "웹 (PGlite)".into(),
            "네이티브 (이 PC)".into(),
            "배율".into(),
            true,
        ));
        if let Some(g) = self.gen_ms {
            table = table.child(row(
                "데이터 생성·적재".into(),
                fmt_ms(web_load),
                fmt_ms(g),
                format!("{:.0}×", web_load / g.max(0.001)),
                false,
            ));
        }
        for c in &self.cases {
            let w = web.iter().find(|(n, _)| *n == c.name).map(|w| w.1);
            table = table.child(row(
                c.name.clone(),
                w.map(fmt_ms).unwrap_or("-".into()),
                fmt_ms(c.median_ms),
                w.map(|w| {
                    if c.median_ms > 0.01 {
                        format!("{:.0}×", w / c.median_ms)
                    } else {
                        "≫".into()
                    }
                })
                .unwrap_or("-".into()),
                false,
            ));
        }
        let mut draws = v_flex().border_1().border_color(theme.border).rounded_md().child(
            h_flex()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(div().flex_1().child("draw 시나리오 (400×300, 섬네일 포함)"))
                .child(div().w(px(80.)).text_right().child("item"))
                .child(div().w(px(100.)).text_right().child("쿼리"))
                .child(div().w(px(100.)).text_right().child("전체"))
                .child(div().w(px(100.)).text_right().child("item/s")),
        );
        for d in &self.draws {
            draws = draws.child(
                h_flex()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .child(div().flex_1().child(d.name.clone()))
                    .child(div().w(px(80.)).text_right().child(fmt_int(d.items)))
                    .child(div().w(px(100.)).text_right().child(fmt_ms(d.query_ms)))
                    .child(div().w(px(100.)).text_right().child(fmt_ms(d.total_ms)))
                    .child(div().w(px(100.)).text_right().child(fmt_int(d.items_per_sec.round() as usize))),
            );
        }
        let running = self.running;
        v_flex()
            .id("bench-scroll")
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("성능 측정: 웹(CSR) vs Rust 네이티브"))
            .child(div().text_sm().text_color(theme.muted_foreground).child(
                "왼쪽 기준값은 vis-a-lot의 pnpm bench:db (PGlite, 같은 생성기·같은 쿼리)를 4코어 Linux에서 잰 중앙값입니다. 오른쪽은 이 PC에서 같은 케이스를 지금 잰 중앙값입니다 (warmup 1회 + 5회).",
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(Button::new("b-100k").label("10만 행").small().when(self.rows == 100_000, |b| b.primary()).disabled(running).on_click(cx.listener(|this, _, _, cx| {
                        this.rows = 100_000;
                        cx.notify();
                    })))
                    .child(Button::new("b-1m").label("100만 행").small().when(self.rows == 1_000_000, |b| b.primary()).disabled(running).on_click(cx.listener(|this, _, _, cx| {
                        this.rows = 1_000_000;
                        cx.notify();
                    })))
                    .child(Button::new("b-run").icon(crate::assets::icon(gpui_kit::assets::IconName::Play)).label("측정 시작").small().primary().loading(running).disabled(running).on_click(cx.listener(|this, _, window, cx| this.run(window, cx))))
                    .child(div().text_sm().text_color(theme.muted_foreground).child(self.status.clone())),
            )
            .child(table)
            .child(draws)
    }
}
