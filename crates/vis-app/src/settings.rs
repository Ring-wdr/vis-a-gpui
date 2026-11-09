//! 속성 다이얼로그 (원본 전체 설정 팝업 · item ⚙ 개별 속성 팝업).
//! 왼쪽 메뉴로 고른다: 색상(공통 색 · legend 값별 색 · 위치 · 투명도), split 구분 기호, 툴팁(가까운 점),
//! 그리고 차트 종류가 쓰는 차트 속성 구역(제목 · 축 · 범례 · 패널 · 팔레트 · 모양 · 크기 · 점 라벨 · 점수 · 예측 · 값 · 강조 · 툴팁 · 식).
//! - 공통 속성: 적용하면 item 개별 속성을 덮어쓰고 전체를 다시 그린다.
//! - 개별 속성: 그 item만 다시 그린다. '공통 따르기'로 되돌린다.

use std::collections::BTreeMap;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _, Toggle, ToggleGroup};
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerState};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::slider::{Slider, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Colorize as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;

use vis_core::options::{OptionSection, defaults_for, merge, pick_supported, support};
use vis_core::render::theme::{palette, parse_hex};
use vis_core::spec::{ChartTheme, ChartType, CommonProps, ItemState, LegendPosition};

use crate::assets::icon;
use crate::options_form::OptionsForm;
use crate::workspace::Workspace;

/// 원본 LegendPositionPicker와 같은 패널 아이콘
const POSITIONS: [(LegendPosition, &str, Lucide); 4] = [
    (LegendPosition::Top, "위", Lucide::PanelTop),
    (LegendPosition::Bottom, "아래", Lucide::PanelBottom),
    (LegendPosition::Left, "왼쪽", Lucide::PanelLeft),
    (LegendPosition::Right, "오른쪽", Lucide::PanelRight),
];

fn hsla(hex: &str) -> Hsla {
    rgb(parse_hex(hex).unwrap_or(palette(0))).into()
}

fn hex_of(c: Hsla) -> String {
    let h = c.to_hex();
    // #RRGGBBAA → #RRGGBB
    h[..7.min(h.len())].to_lowercase()
}

fn picker(window: &mut Window, cx: &mut App, hex: &str) -> Entity<ColorPickerState> {
    let c = hsla(hex);
    cx.new(|cx| ColorPickerState::new(window, cx).default_value(c))
}

/// 어느 대상의 속성을 고치는지
#[derive(Clone, Copy, PartialEq)]
pub enum Target {
    Common,
    Item(usize),
}

/// 왼쪽 메뉴
#[derive(Clone, Copy, PartialEq)]
enum Page {
    Colors,
    Split,
    Tooltip,
    Opt(OptionSection),
}

/// 공통 설정에서만 쓰는 값 (ChartSpec 쪽)
pub struct SpecExtras {
    pub split_separator: String,
    pub show_nearest: bool,
}

pub struct PropsPanel {
    workspace: WeakEntity<Workspace>,
    target: Target,
    page: Page,
    color: Entity<ColorPickerState>,
    legend: Vec<(String, Entity<ColorPickerState>)>,
    position: LegendPosition,
    transparency: Option<Entity<SliderState>>,
    /// 개별 속성에서 위치를 공통 속성에 맡기는지
    follow_position: bool,
    separator: Entity<InputState>,
    show_nearest: bool,
    form: Entity<OptionsForm>,
    chart: ChartType,
    _subs: Vec<Subscription>,
}

impl PropsPanel {
    /// legend: (값, 지금 색)
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        workspace: WeakEntity<Workspace>,
        target: Target,
        chart: ChartType,
        theme: ChartTheme,
        common: &CommonProps,
        extras: SpecExtras,
        item: Option<&ItemState>,
        legend: Vec<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let color_hex = item.and_then(|s| s.color.clone()).unwrap_or_else(|| common.color.clone());
        let color = picker(window, cx, &color_hex);
        let legend = legend.into_iter().map(|(v, hex)| (v.clone(), picker(window, cx, &hex))).collect();
        let transparency = (target == Target::Common).then(|| {
            let t = common.transparency * 100.0;
            cx.new(|_| SliderState::new().min(0.0).max(90.0).step(5.0).default_value(t))
        });
        let mut _subs: Vec<Subscription> = transparency.iter().map(|t| cx.observe(t, |_, _, cx| cx.notify())).collect();
        let sep = extras.split_separator.clone();
        let separator = cx.new(|cx| InputState::new(window, cx).default_value(sep).placeholder("예: - / · |"));
        // 차트 속성: 공통이면 기본값 위, 개별이면 공통까지 합친 값 위에서 고친다
        let mut base = defaults_for(chart, theme);
        let patch = match (target, item) {
            (Target::Item(_), Some(s)) => {
                merge(&mut base, &pick_supported(chart, &common.options));
                s.options.clone()
            }
            _ => common.options.clone(),
        };
        let form = cx.new(|_| OptionsForm::new(chart, base, &patch));
        _subs.push(cx.observe(&form, |_, _, cx| cx.notify()));
        Self {
            workspace,
            target,
            page: Page::Colors,
            color,
            legend,
            position: item.and_then(|s| s.legend_position).unwrap_or(common.legend_position),
            transparency,
            follow_position: item.is_some_and(|s| s.legend_position.is_none()),
            separator,
            show_nearest: extras.show_nearest,
            form,
            chart,
            _subs,
        }
    }

    /// 식 오류가 있으면 적용하지 않는다
    fn apply(&self, window: &mut Window, cx: &mut App) -> bool {
        if self.form.read(cx).expression_error().is_some() {
            return false;
        }
        let color = self.color.read(cx).value().map(hex_of);
        let legend: BTreeMap<String, String> = self
            .legend
            .iter()
            .filter_map(|(v, s)| s.read(cx).value().map(|c| (v.clone(), hex_of(c))))
            .collect();
        let transparency = self.transparency.as_ref().map(|s| s.read(cx).value().start() / 100.0);
        let position = self.position;
        let follow = self.follow_position;
        let target = self.target;
        let options: Value = self.form.read(cx).patch();
        let sep = self.separator.read(cx).value().to_string();
        let nearest = self.show_nearest;
        let _ = self.workspace.update(cx, |ws, cx| match target {
            Target::Common => {
                let mut common = ws.spec.common.clone();
                if let Some(c) = color {
                    common.color = c;
                }
                common.legend_colors = legend;
                common.legend_position = position;
                if let Some(t) = transparency {
                    common.transparency = t;
                }
                common.options = options;
                ws.apply_common(
                    common,
                    SpecExtras {
                        split_separator: sep,
                        show_nearest: nearest,
                    },
                    window,
                    cx,
                );
            }
            Target::Item(i) => ws.apply_item_props(
                i,
                move |s| {
                    s.color = color;
                    s.legend_colors = legend;
                    s.legend_position = (!follow).then_some(position);
                    s.options = options;
                },
                window,
                cx,
            ),
        });
        true
    }

    fn reset_item(&self, window: &mut Window, cx: &mut App) {
        if let Target::Item(i) = self.target {
            let _ = self.workspace.update(cx, |ws, cx| {
                ws.apply_item_props(
                    i,
                    |s| {
                        s.color = None;
                        s.legend_colors.clear();
                        s.legend_position = None;
                        s.options = Value::Null;
                    },
                    window,
                    cx,
                )
            });
        }
    }

    /// 다이얼로그로 연다
    pub fn open(panel: Entity<PropsPanel>, title: String, window: &mut Window, cx: &mut App) {
        window.open_dialog(cx, move |dialog, _, cx| {
            let (p1, p2) = (panel.clone(), panel.clone());
            let is_item = matches!(panel.read(cx).target, Target::Item(_));
            dialog.title(title.clone()).width(px(860.)).child(panel.clone()).footer(
                DialogFooter::new()
                    .gap_2()
                    .when(is_item, |f| {
                        f.child(
                            Button::new("props-reset")
                                .label("공통 따르기")
                                .outline()
                                .on_click(move |_, window, cx| {
                                    p2.update(cx, |p, cx| p.reset_item(window, cx));
                                    window.close_dialog(cx);
                                }),
                        )
                    })
                    .child(
                        Button::new("props-cancel")
                            .label("취소")
                            .outline()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("props-ok")
                            .icon(icon(Lucide::Check))
                            .label("적용")
                            .primary()
                            .on_click(move |_, window, cx| {
                                if p1.update(cx, |p, cx| p.apply(window, cx)) {
                                    window.close_dialog(cx);
                                }
                            }),
                    ),
            )
        });
    }

    fn colors_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let label = |s: &str| div().text_sm().text_color(theme.muted_foreground).w(px(150.)).child(s.to_string());
        let row = |l: Div, c: AnyElement| h_flex().gap_3().child(l).child(div().flex_1().child(c));
        let selected = POSITIONS.iter().position(|p| p.0 == self.position);
        let mut body = v_flex()
            .gap_3()
            .child(row(
                label("색 (legend 없음)"),
                ColorPicker::new(&self.color).small().into_any_element(),
            ))
            .child(row(
                label("legend 위치"),
                v_flex()
                    .gap_1()
                    .child({
                        let cur = if self.follow_position { None } else { selected };
                        let mut g = ToggleGroup::new("legend-pos").small().segmented();
                        for (k, (_, label, ic)) in POSITIONS.iter().enumerate() {
                            g = g.child(
                                Toggle::new(SharedString::from(format!("lp-{k}")))
                                    .icon(icon(*ic))
                                    .label(*label)
                                    .checked(cur == Some(k)),
                            );
                        }
                        g.on_click(cx.listener(move |this, checked: &Vec<bool>, _, cx| {
                            if let Some(k) = checked.iter().enumerate().position(|(k, on)| *on && Some(k) != cur) {
                                this.position = POSITIONS[k].0;
                                this.follow_position = false;
                                cx.notify();
                            }
                        }))
                    })
                    .when(matches!(self.target, Target::Item(_)) && self.follow_position, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("지금은 공통 속성의 위치를 따릅니다."),
                        )
                    })
                    .into_any_element(),
            ));
        if let Some(t) = &self.transparency {
            let v = t.read(cx).value().start();
            body = body.child(row(
                label("투명도 (전체)"),
                h_flex()
                    .gap_2()
                    .child(Slider::new(t).flex_1())
                    .child(div().w(px(40.)).text_sm().child(format!("{v:.0}%")))
                    .into_any_element(),
            ));
        }
        if !self.legend.is_empty() {
            let mut grid = div().flex().flex_wrap().gap_2();
            for (v, s) in &self.legend {
                grid = grid.child(
                    h_flex()
                        .gap_1()
                        .w(px(180.))
                        .child(ColorPicker::new(s).small())
                        .child(div().text_sm().truncate().child(v.clone())),
                );
            }
            body = body.child(v_flex().gap_1().child(label("legend 값별 색")).child(grid));
        }
        body.child(div().text_xs().text_color(theme.muted_foreground).child(match self.target {
            Target::Common => "적용하면 item 개별 속성을 덮어쓰고 전체를 다시 그립니다.",
            Target::Item(_) => "이 item만 다시 그립니다. 색 결정 순서: item 개별 → 공통 속성 → 팔레트.",
        }))
        .into_any_element()
    }
}

impl Render for PropsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        // 왼쪽 메뉴
        let mut pages: Vec<(Page, &'static str)> = vec![(Page::Colors, "색상 · 범례 위치")];
        if self.target == Target::Common {
            pages.push((Page::Split, "split by (구분 기호)"));
            pages.push((Page::Tooltip, "툴팁 (가까운 점)"));
        }
        for s in support(self.chart) {
            pages.push((Page::Opt(s), s.label()));
        }
        let mut nav = v_flex()
            .w(px(180.))
            .flex_shrink_0()
            .gap_0p5()
            .pr_2()
            .border_r_1()
            .border_color(theme.border);
        for (k, (p, label)) in pages.iter().enumerate() {
            let p = *p;
            let active = p == self.page;
            if k == 1 && self.target == Target::Common || matches!(p, Page::Opt(OptionSection::Title)) {
                nav = nav.child(
                    div()
                        .pt_2()
                        .pb_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(if matches!(p, Page::Opt(_)) {
                            "차트 속성"
                        } else {
                            "전체 설정"
                        }),
                );
            }
            nav = nav.child(
                div()
                    .id(SharedString::from(format!("nav-{k}")))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_sm()
                    .cursor_pointer()
                    .when(active, |d| d.bg(theme.accent).text_color(theme.accent_foreground))
                    .when(!active, |d| d.hover(|s| s.bg(theme.muted)))
                    .child(*label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.page = p;
                        cx.notify();
                    })),
            );
        }
        let content: AnyElement = match self.page {
            Page::Colors => self.colors_page(cx),
            Page::Split => v_flex()
                .gap_2()
                .child(div().text_sm().child("split 조합 이름을 잇는 문자"))
                .child(Input::new(&self.separator).small().w(px(200.)))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("예: 공장-라인. 적용하면 item 이름(제목)이 바뀝니다."),
                )
                .into_any_element(),
            Page::Tooltip => v_flex()
                .gap_2()
                .child(
                    Switch::new("nearest")
                        .checked(self.show_nearest)
                        .label("가리킨 자리에서 가장 가까운 점 표시")
                        .on_click(cx.listener(|this, v: &bool, _, cx| {
                            this.show_nearest = *v;
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("끄면 점 차트 툴팁이 커서 자리의 x · y 값만 보입니다."),
                )
                .into_any_element(),
            Page::Opt(s) => {
                self.form.update(cx, |f, _| f.section = Some(s));
                let form = self.form.clone();
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .child(div().flex_1().text_sm().font_weight(FontWeight::SEMIBOLD).child(s.label()))
                            .child(
                                Button::new("sec-reset")
                                    .icon(icon(Lucide::RotateCcw))
                                    .label("이 구역 기본값")
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.form.update(cx, |f, cx| f.reset_section(s, cx));
                                    })),
                            ),
                    )
                    .child(form)
                    .into_any_element()
            }
        };
        h_flex()
            .items_start()
            .gap_3()
            .h(px(520.))
            .child(div().id("props-nav").h_full().overflow_y_scroll().child(nav))
            .child(div().id("props-body").flex_1().h_full().overflow_y_scroll().pr_2().child(content))
    }
}
