//! 차트 색. 원본 server/render/theme.ts와 같은 값이다.

use crate::spec::ChartTheme;

#[derive(Clone, Copy)]
pub struct Theme {
    pub surface: u32,
    pub text_primary: u32,
    pub text_secondary: u32,
    pub text_muted: u32,
    pub grid: u32,
    pub axis: u32,
    pub selection: u32,
}

pub const LIGHT: Theme = Theme {
    surface: 0xfcfcfb,
    text_primary: 0x0b0b0b,
    text_secondary: 0x52514e,
    text_muted: 0x8a8984,
    grid: 0xe7e6e2,
    axis: 0xc9c8c2,
    selection: 0x2a78d6,
};

pub const DARK: Theme = Theme {
    surface: 0x141413,
    text_primary: 0xecebe8,
    text_secondary: 0xb5b4ae,
    text_muted: 0x8a8984,
    grid: 0x363632,
    axis: 0x55544f,
    selection: 0x4d8fe6,
};

pub fn theme(t: ChartTheme) -> Theme {
    match t {
        ChartTheme::Light => LIGHT,
        ChartTheme::Dark => DARK,
    }
}

/// dataviz 기본 categorical 팔레트 (순서가 색각 이상 대비 안전성의 근거라 돌려쓰지 않는다)
pub const PALETTE: [&str; 8] = [
    "#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#6250d6", "#e34948",
];
pub const OTHERS_COLOR: u32 = 0xa3a29c;

pub fn palette(i: usize) -> u32 {
    parse_hex(PALETTE[i % PALETTE.len()]).unwrap()
}

pub fn parse_hex(s: &str) -> Option<u32> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}

pub fn hex(c: u32) -> String {
    format!("#{c:06x}")
}

pub fn rgb(c: u32) -> (u8, u8, u8) {
    ((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// a와 b를 t만큼 섞는다
pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let (ar, ag, ab) = rgb(a);
    let (br, bg, bb) = rgb(b);
    let m = |x: u8, y: u8| ((x as f32) * (1.0 - t) + (y as f32) * t).round() as u32;
    (m(ar, br) << 16) | (m(ag, bg) << 8) | m(ab, bb)
}

/// 상대 밝기 (글자 색 고르기)
pub fn luminance(c: u32) -> f32 {
    let (r, g, b) = rgb(c);
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.0
}

/// 순차 색 (heatmap): 밝은 파랑 → 진한 파랑
pub fn sequential(t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let stops = [0xeaf2fbu32, 0x9cc3ee, 0x4a8fdc, 0x2a78d6, 0x123f78];
    let x = t * (stops.len() - 1) as f32;
    let i = (x.floor() as usize).min(stops.len() - 2);
    mix(stops[i], stops[i + 1], x - i as f32)
}
