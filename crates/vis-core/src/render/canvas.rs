//! OffscreenCanvas 2D 대신 쓰는 래스터 캔버스 (tiny-skia). 좌표는 CSS 픽셀이고, 고해상도 화면이면
//! pixel_ratio배 크기 Pixmap에 같은 배율을 걸어 그린다 (원본 drawPng와 같다).

use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, PremultipliedColorU8, Rect, Stroke, StrokeDash, Transform,
};

use super::text;
use super::theme::rgb;

#[derive(Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

pub struct Canvas {
    pub pixmap: Pixmap,
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    ts: Transform,
    clip: Option<tiny_skia::Mask>,
    clip_rect: Option<(f32, f32, f32, f32)>,
}

pub fn color(c: u32, alpha: f32) -> Color {
    let (r, g, b) = rgb(c);
    Color::from_rgba8(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn paint(c: u32, alpha: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color(c, alpha));
    p.anti_alias = true;
    p
}

impl Canvas {
    pub fn new(width: f32, height: f32, scale: f32) -> Canvas {
        let scale = scale.min(4096.0 / width.max(height).max(1.0)).max(0.25);
        let pw = ((width * scale).round() as u32).max(1);
        let ph = ((height * scale).round() as u32).max(1);
        Canvas {
            pixmap: Pixmap::new(pw, ph).expect("pixmap"),
            width,
            height,
            scale,
            ts: Transform::from_scale(scale, scale),
            clip: None,
            clip_rect: None,
        }
    }

    pub fn fill(&mut self, c: u32) {
        self.pixmap.fill(color(c, 1.0));
    }

    /// 그림 영역 밖으로 나가지 않게 자른다. None이면 풀기
    pub fn set_clip(&mut self, r: Option<(f32, f32, f32, f32)>) {
        self.clip_rect = r;
        self.clip = r.and_then(|(x, y, w, h)| {
            let mut m = tiny_skia::Mask::new(self.pixmap.width(), self.pixmap.height())?;
            let rect = Rect::from_xywh(x, y, w, h)?;
            m.fill_path(&PathBuilder::from_rect(rect), FillRule::Winding, false, self.ts);
            Some(m)
        });
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: u32, alpha: f32) {
        if let Some(r) = Rect::from_xywh(x, y, w.max(0.0), h.max(0.0)) {
            if r.width() > 0.0 && r.height() > 0.0 {
                self.pixmap.fill_rect(r, &paint(c, alpha), self.ts, self.clip.as_ref());
            }
        }
    }

    pub fn fill_path(&mut self, path: &Path, c: u32, alpha: f32) {
        self.pixmap
            .fill_path(path, &paint(c, alpha), FillRule::Winding, self.ts, self.clip.as_ref());
    }

    pub fn stroke_path(&mut self, path: &Path, c: u32, alpha: f32, width: f32, dash: Option<&[f32]>) {
        let mut stroke = Stroke {
            width,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Round,
            ..Default::default()
        };
        if let Some(d) = dash {
            stroke.dash = StrokeDash::new(d.to_vec(), 0.0);
        }
        self.pixmap
            .stroke_path(path, &paint(c, alpha), &stroke, self.ts, self.clip.as_ref());
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: u32, width: f32) {
        let mut pb = PathBuilder::new();
        pb.move_to(x0, y0);
        pb.line_to(x1, y1);
        if let Some(p) = pb.finish() {
            self.stroke_path(&p, c, 1.0, width, None);
        }
    }

    /// 화면 픽셀에 맞춘 1px 가로선
    pub fn hline(&mut self, x0: f32, x1: f32, y: f32, c: u32) {
        let y = (y * self.scale).round() / self.scale + 0.5 / self.scale;
        self.line(x0, y, x1, y, c, 1.0 / self.scale.max(1.0));
    }

    pub fn vline(&mut self, x: f32, y0: f32, y1: f32, c: u32) {
        let x = (x * self.scale).round() / self.scale + 0.5 / self.scale;
        self.line(x, y0, x, y1, c, 1.0 / self.scale.max(1.0));
    }

    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: u32, width: f32) {
        if let Some(r) = Rect::from_xywh(x, y, w, h) {
            self.stroke_path(&PathBuilder::from_rect(r), c, 1.0, width, None);
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, fill: u32, alpha: f32) {
        if let Some(p) = PathBuilder::from_circle(cx, cy, r) {
            self.fill_path(&p, fill, alpha);
        }
    }

    /// 긴 꺾은선 (line 차트). 선 조각마다 픽셀까지의 거리로 덮임 정도를 구해 마스크에 최댓값으로 모으고
    /// 한 번에 칠한다. 경로를 외곽선으로 바꿔 채우는 일반 stroke보다 몇 배 빠르고, 이음매가 두 번 칠해지지 않는다.
    pub fn polyline_aa(&mut self, points: &[(f32, f32)], c: u32, alpha: f32, width: f32) {
        if points.len() < 2 {
            return;
        }
        let sc = self.scale;
        let pw = self.pixmap.width() as i32;
        let ph = self.pixmap.height() as i32;
        let (cx0, cy0, cx1, cy1) = match self.clip_rect {
            Some((x, y, w, h)) => (
                ((x * sc).floor() as i32).max(0),
                ((y * sc).floor() as i32).max(0),
                (((x + w) * sc).ceil() as i32).min(pw),
                (((y + h) * sc).ceil() as i32).min(ph),
            ),
            None => (0, 0, pw, ph),
        };
        if cx1 <= cx0 || cy1 <= cy0 {
            return;
        }
        let mw = (cx1 - cx0) as usize;
        let mut mask = vec![0u8; mw * (cy1 - cy0) as usize];
        let hw = (width * sc / 2.0).max(0.5);
        let reach = hw + 1.0;
        for seg in points.windows(2) {
            let (ax, ay) = (seg[0].0 * sc, seg[0].1 * sc);
            let (bx, by) = (seg[1].0 * sc, seg[1].1 * sc);
            if !(ax.is_finite() && ay.is_finite() && bx.is_finite() && by.is_finite()) {
                continue;
            }
            let x0 = ((ax.min(bx) - reach).floor() as i32).max(cx0);
            let x1 = ((ax.max(bx) + reach).ceil() as i32).min(cx1);
            let y0 = ((ay.min(by) - reach).floor() as i32).max(cy0);
            let y1 = ((ay.max(by) + reach).ceil() as i32).min(cy1);
            if x0 >= x1 || y0 >= y1 {
                continue;
            }
            let (dx, dy) = (bx - ax, by - ay);
            let len2 = dx * dx + dy * dy;
            for y in y0..y1 {
                let py = y as f32 + 0.5;
                let row = (y - cy0) as usize * mw;
                for x in x0..x1 {
                    let px = x as f32 + 0.5;
                    let t = if len2 > 0.0 {
                        (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let (qx, qy) = (ax + dx * t - px, ay + dy * t - py);
                    let d = (qx * qx + qy * qy).sqrt();
                    let cov = hw + 0.5 - d;
                    if cov <= 0.0 {
                        continue;
                    }
                    let v = (cov.min(1.0) * 255.0) as u8;
                    let m = &mut mask[row + (x - cx0) as usize];
                    if v > *m {
                        *m = v;
                    }
                }
            }
        }
        let (r, g, b) = rgb(c);
        let a = (alpha.clamp(0.0, 1.0) * 255.0) as u32;
        let data = self.pixmap.pixels_mut();
        for y in cy0..cy1 {
            let row = (y - cy0) as usize * mw;
            for x in cx0..cx1 {
                let m = mask[row + (x - cx0) as usize];
                if m == 0 {
                    continue;
                }
                let cov = ((m as u32 * a + 127) / 255) as u8;
                blend(&mut data[(y * pw + x) as usize], r, g, b, cov);
            }
        }
    }

    // ── 글자 ─────────────────────────────────────────────────────────

    pub fn text(&mut self, s: &str, x: f32, baseline: f32, px: f32, c: u32, align: Align) {
        let w = text::measure(s, px);
        let x0 = match align {
            Align::Left => x,
            Align::Center => x - w / 2.0,
            Align::Right => x - w,
        };
        let sc = self.scale;
        let (r, g, b) = rgb(c);
        let clip = self.clip_rect.map(|(x, y, w, h)| {
            (
                (x * sc).floor() as i32,
                (y * sc).floor() as i32,
                ((x + w) * sc).ceil() as i32,
                ((y + h) * sc).ceil() as i32,
            )
        });
        let pw = self.pixmap.width() as i32;
        let ph = self.pixmap.height() as i32;
        let data = self.pixmap.pixels_mut();
        text::layout(s, px * sc, x0 * sc, baseline * sc, |m, gx, gy| {
            for my in 0..m.h {
                let py = gy + my as i32;
                if py < 0 || py >= ph || clip.is_some_and(|c| py < c.1 || py >= c.3) {
                    continue;
                }
                for mx in 0..m.w {
                    let a = m.data[my * m.w + mx];
                    if a == 0 {
                        continue;
                    }
                    let px_ = gx + mx as i32;
                    if px_ < 0 || px_ >= pw || clip.is_some_and(|c| px_ < c.0 || px_ >= c.2) {
                        continue;
                    }
                    let i = (py * pw + px_) as usize;
                    blend(&mut data[i], r, g, b, a);
                }
            }
        });
    }

    /// 세로로 돌린 글자 (y축 제목). 왼쪽으로 90도
    pub fn text_vertical(&mut self, s: &str, cx: f32, cy: f32, px: f32, c: u32) {
        let w = text::measure(s, px);
        let (asc, desc) = text::metrics(px);
        let mut tmp = Canvas::new(w.ceil() + 2.0, (asc + desc).ceil() + 2.0, self.scale);
        tmp.text(s, 1.0, asc + 1.0, px, c, Align::Left);
        let tw = tmp.width;
        let th = tmp.height;
        let t = Transform::from_translate(cx - th / 2.0, cy + tw / 2.0)
            .pre_rotate(-90.0)
            .post_scale(self.scale, self.scale)
            .pre_scale(1.0 / self.scale, 1.0 / self.scale);
        self.pixmap
            .draw_pixmap(0, 0, tmp.pixmap.as_ref(), &tiny_skia::PixmapPaint::default(), t, None);
    }

    // ── 점 찍기 (scatter): 미리 그린 원 하나를 픽셀 단위로 겹쳐 놓는다 ─────────────

    pub fn stamp(&self, radius: f32, fill: u32, border: Option<(u32, f32)>, alpha: f32) -> Stamp {
        let r = radius * self.scale;
        let bw = border.map(|b| b.1 * self.scale).unwrap_or(0.0);
        let half = (r + bw).ceil() as i32 + 1;
        let size = (half * 2 + 1) as u32;
        let mut pm = Pixmap::new(size, size).unwrap();
        let c = half as f32 + 0.5;
        if let Some(p) = PathBuilder::from_circle(c, c, r) {
            pm.fill_path(&p, &paint(fill, alpha), FillRule::Winding, Transform::identity(), None);
            if let Some((bc, _)) = border {
                if bw > 0.0 {
                    let stroke = Stroke {
                        width: bw,
                        ..Default::default()
                    };
                    pm.stroke_path(&p, &paint(bc, alpha), &stroke, Transform::identity(), None);
                }
            }
        }
        Stamp {
            half,
            size: size as i32,
            pixels: pm.pixels().to_vec(),
        }
    }

    /// CSS 좌표 (x, y)에 stamp를 놓는다 (clip 사각형 안쪽만)
    #[inline]
    pub fn put(&mut self, s: &Stamp, x: f32, y: f32) {
        let sc = self.scale;
        let cx = (x * sc).floor() as i32;
        let cy = (y * sc).floor() as i32;
        let pw = self.pixmap.width() as i32;
        let ph = self.pixmap.height() as i32;
        let (x0, y0, x1, y1) = match self.clip_rect {
            Some((x, y, w, h)) => (
                ((x * sc).floor() as i32).max(0),
                ((y * sc).floor() as i32).max(0),
                (((x + w) * sc).ceil() as i32).min(pw),
                (((y + h) * sc).ceil() as i32).min(ph),
            ),
            None => (0, 0, pw, ph),
        };
        let data = self.pixmap.pixels_mut();
        for sy in 0..s.size {
            let py = cy - s.half + sy;
            if py < y0 || py >= y1 {
                continue;
            }
            let row = (sy * s.size) as usize;
            for sx in 0..s.size {
                let px = cx - s.half + sx;
                if px < x0 || px >= x1 {
                    continue;
                }
                let src = s.pixels[row + sx as usize];
                let a = src.alpha();
                if a == 0 {
                    continue;
                }
                let d = &mut data[(py * pw + px) as usize];
                if a == 255 {
                    *d = src;
                } else {
                    let inv = 255 - a as u32;
                    let m = |s: u8, d: u8| (s as u32 + (d as u32 * inv + 127) / 255).min(255) as u8;
                    *d = PremultipliedColorU8::from_rgba(
                        m(src.red(), d.red()),
                        m(src.green(), d.green()),
                        m(src.blue(), d.blue()),
                        m(a, d.alpha()),
                    )
                    .unwrap_or(*d);
                }
            }
        }
    }

    /// 화면용 BGRA (premultiplied 알파를 푼다. 바탕이 불투명이라 대부분 그대로다)
    pub fn to_bgra(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixmap.data().len());
        for p in self.pixmap.pixels() {
            let c = p.demultiply();
            out.extend_from_slice(&[c.blue(), c.green(), c.red(), c.alpha()]);
        }
        out
    }

    pub fn encode_png(&self) -> Vec<u8> {
        self.pixmap.encode_png().unwrap_or_default()
    }
}

pub struct Stamp {
    half: i32,
    size: i32,
    pixels: Vec<PremultipliedColorU8>,
}

#[inline]
fn blend(d: &mut PremultipliedColorU8, r: u8, g: u8, b: u8, a: u8) {
    let a32 = a as u32;
    let inv = 255 - a32;
    let m = |s: u8, d: u8| ((s as u32 * a32 + d as u32 * inv + 127) / 255) as u8;
    let na = ((a32 * 255 + d.alpha() as u32 * inv + 127) / 255) as u8;
    *d = PremultipliedColorU8::from_rgba(m(r, d.red()).min(na), m(g, d.green()).min(na), m(b, d.blue()).min(na), na).unwrap_or(*d);
}

/// 다각형 경로
pub fn polygon(points: &[(f32, f32)]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let (first, rest) = points.split_first()?;
    pb.move_to(first.0, first.1);
    for p in rest {
        pb.line_to(p.0, p.1);
    }
    pb.close();
    pb.finish()
}

pub fn polyline(points: &[(f32, f32)]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let (first, rest) = points.split_first()?;
    pb.move_to(first.0, first.1);
    for p in rest {
        pb.line_to(p.0, p.1);
    }
    pb.finish()
}

pub fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    if r < 0.5 {
        return Rect::from_xywh(x, y, w, h).map(PathBuilder::from_rect);
    }
    let mut pb = PathBuilder::new();
    let k = 0.552_284_8 * r;
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}
