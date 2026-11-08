//! 화면 공통 조각: 드롭다운 선택, 이미지 변환, 숫자 글자.

use std::sync::Arc;

use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::*;
use smallvec::SmallVec;

/// BGRA 비트맵 → GPUI 이미지 (벡터를 옮기므로 복사 없음)
pub fn render_image(w: u32, h: u32, bgra: Vec<u8>) -> Arc<RenderImage> {
    let buf = image::RgbaImage::from_raw(w, h, bgra).expect("bitmap size");
    let frame = image::Frame::new(buf);
    let mut frames: SmallVec<[image::Frame; 1]> = SmallVec::new();
    frames.push(frame);
    Arc::new(RenderImage::new(frames))
}

/// 왼쪽 패널의 한 줄: 라벨 + 내용
pub fn field(label: impl Into<SharedString>, child: impl IntoElement, cx: &App) -> Div {
    v_flex()
        .gap_1()
        .child(div().text_xs().text_color(cx.theme().muted_foreground).child(label.into()))
        .child(child)
}

pub fn fmt_int(n: usize) -> String {
    vis_core::render::charts_fmt_int(n)
}

pub fn fmt_ms(ms: f64) -> String {
    if ms >= 10_000.0 {
        format!("{:.1}초", ms / 1000.0)
    } else if ms >= 1000.0 {
        format!("{:.2}초", ms / 1000.0)
    } else if ms >= 10.0 {
        format!("{ms:.0}ms")
    } else {
        format!("{ms:.1}ms")
    }
}

pub fn fmt_bytes(b: usize) -> String {
    let b = b as f64;
    if b >= 1e9 {
        format!("{:.2} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else {
        format!("{:.0} KB", b / 1e3)
    }
}

pub fn hex_color(c: u32) -> Hsla {
    rgb(c).into()
}
