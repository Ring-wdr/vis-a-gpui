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

/// 렌더 스타일: 테마 색 + 합친 차트 속성. 렌더러는 `t.surface`처럼 색을, `t.o.title`처럼 속성을 읽는다.
/// 색은 속성에서 온다(패널 배경 = surface, 축 글자 = text_secondary …). 다크 테마는 속성 기본값이 이미 다크 값이다.
pub struct Style {
    pub c: Theme,
    pub o: crate::options::ChartOptions,
    /// item 크기 (글자 크기 자동)
    pub size: (f32, f32),
    pub compact: bool,
}

impl std::ops::Deref for Style {
    type Target = Theme;
    fn deref(&self) -> &Theme {
        &self.c
    }
}

/// 비었거나 잘못된 색이면 fallback
pub fn color_or(hex: &str, fallback: u32) -> u32 {
    parse_hex(hex).unwrap_or(fallback)
}

impl Style {
    pub fn new(theme_kind: ChartTheme, o: crate::options::ChartOptions, size: (f32, f32), compact: bool) -> Style {
        let base = theme(theme_kind);
        let c = Theme {
            surface: color_or(&o.panel.back_color, base.surface),
            text_primary: color_or(&o.title.color, base.text_primary),
            text_secondary: color_or(&o.axes.y.label.color, base.text_secondary),
            text_muted: color_or(&o.title.summary.color, base.text_muted),
            grid: color_or(&o.axes.y.grid.color, base.grid),
            axis: color_or(&o.axes.x.line.color, base.axis),
            selection: base.selection,
        };
        Style { c, o, size, compact }
    }
    /// 구역 글자 크기: 자동이면 item 크기에 맞춰, 아니면 구역 값
    pub fn font(&self, fixed: f32, base: f32) -> f32 {
        crate::options::font_px(&self.o, fixed, base, self.size.0, self.size.1)
    }
    /// 도형 불투명도 (모양 › 투명도)
    pub fn shape_alpha(&self) -> f32 {
        1.0 - self.o.shape.transparency.clamp(0.0, 0.95)
    }
    /// 도형 테두리: 정한 색, 없으면 자동(굵기를 정했으면 배경색, 아니면 도형 색을 진하게)
    pub fn border_of(&self, fill: u32) -> u32 {
        match parse_hex(&self.o.shape.border_color) {
            Some(c) => c,
            None if self.o.shape.border_width > 0.0 => self.surface,
            None => mix(fill, 0x000000, 0.25),
        }
    }
    pub fn others_color(&self) -> u32 {
        color_or(&self.o.palette.others_color, OTHERS_COLOR)
    }
}

/// 팔레트 › 그라데이션: 시작 → (중간) → 끝을 n단계로 (steps가 0이면 n)
pub fn gradient_palette(p: &crate::options::PaletteOptions, n: usize) -> Vec<u32> {
    let steps = if p.steps == 0 { n.max(1) } else { p.steps as usize };
    let start = color_or(&p.start, 0xcfe3fa);
    let end = color_or(&p.end, 0x0d3d78);
    let middle = parse_hex(&p.middle);
    let at = |t: f32| match middle {
        Some(m) if t <= 0.5 => mix(start, m, t * 2.0),
        Some(m) => mix(m, end, (t - 0.5) * 2.0),
        None => mix(start, end, t),
    };
    let mut cols: Vec<u32> = (0..steps)
        .map(|i| at(if steps <= 1 { 0.0 } else { i as f32 / (steps - 1) as f32 }))
        .collect();
    if p.reverse {
        cols.reverse();
    }
    (0..n).map(|i| cols[i % cols.len()]).collect()
}
