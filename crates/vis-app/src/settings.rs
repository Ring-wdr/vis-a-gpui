//! 속성 다이얼로그 (원본 전체 설정 팝업 · item ⚙ 개별 속성 팝업).
//! - 공통 속성: 색, legend 값별 색, legend 위치, 투명도. 적용하면 item 개별 속성을 덮어쓰고 전체를 다시 그린다.
//! - 개별 속성: 그 item의 색, legend 값별 색, legend 위치. '공통 따르기'로 되돌린다.
//! gpui-kit ColorPicker · RadioGroup · Slider · Dialog를 쓴다.

use std::collections::BTreeMap;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerState};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::slider::{Slider, SliderState};
use gpui_kit::component::{ActiveTheme as _, Colorize as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use vis_core::render::theme::{palette, parse_hex};
use vis_core::spec::{CommonProps, ItemState, LegendPosition};

use crate::workspace::Workspace;

const POSITIONS: [(LegendPosition, &str); 4] = [
    (LegendPosition::Top, "위"),
    (LegendPosition::Bottom, "아래"),
    (LegendPosition::Left, "왼쪽"),
    (LegendPosition::Right, "오른쪽"),
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

pub struct PropsPanel {
    workspace: WeakEntity<Workspace>,
    target: Target,
    color: Entity<ColorPickerState>,
    legend: Vec<(String, Entity<ColorPickerState>)>,
    position: LegendPosition,
    transparency: Option<Entity<SliderState>>,
    /// 개별 속성에서 위치를 공통 속성에 맡기는지
    follow_position: bool,
    _subs: Vec<Subscription>,
}

impl PropsPanel {
    /// legend: (값, 지금 색)
    pub fn new(
        workspace: WeakEntity<Workspace>,
        target: Target,
        common: &CommonProps,
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
        let _subs = transparency.iter().map(|t| cx.observe(t, |_, _, cx| cx.notify())).collect();
        Self {
            workspace,
            target,
            color,
            legend,
            position: item.and_then(|s| s.legend_position).unwrap_or(common.legend_position),
            transparency,
            follow_position: item.is_some_and(|s| s.legend_position.is_none()),
            _subs,
        }
    }

    fn apply(&self, window: &mut Window, cx: &mut App) {
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
                ws.apply_common(common, window, cx);
            }
            Target::Item(i) => ws.apply_item_props(
                i,
                move |s| {
                    s.color = color;
                    s.legend_colors = legend;
                    s.legend_position = (!follow).then_some(position);
                },
                window,
                cx,
            ),
        });
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
            dialog.title(title.clone()).width(px(560.)).child(panel.clone()).footer(
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
                    .child(Button::new("props-ok").label("적용").primary().on_click(move |_, window, cx| {
                        p1.update(cx, |p, cx| p.apply(window, cx));
                        window.close_dialog(cx);
                    })),
            )
        });
    }
}

impl Render for PropsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let label = |s: &str| div().text_sm().text_color(theme.muted_foreground).w(px(120.)).child(s.to_string());
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
                    .child(
                        RadioGroup::horizontal("legend-pos")
                            .children(POSITIONS.iter().map(|p| p.1))
                            .selected_index(if self.follow_position { None } else { selected })
                            .on_change(cx.listener(|this, i: &usize, _, cx| {
                                this.position = POSITIONS[*i].0;
                                this.follow_position = false;
                                cx.notify();
                            })),
                    )
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
                label("투명도"),
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
    }
}
