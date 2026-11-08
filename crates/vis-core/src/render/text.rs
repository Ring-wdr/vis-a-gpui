//! 글자 그리기. 시스템 글꼴을 찾아 ab_glyph로 글리프를 래스터화하고, 스레드마다 글리프 마스크를 캐시한다.
//! Windows: 맑은 고딕, macOS: Apple SD Gothic Neo, Linux: Noto Sans CJK / 나눔고딕 순으로 찾는다.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use ab_glyph::{Font, FontArc, GlyphId, PxScale, ScaleFont};

pub struct Fonts {
    pub fonts: Vec<FontArc>,
    pub names: Vec<String>,
}

const FAMILIES: [&str; 10] = [
    "Malgun Gothic",
    "맑은 고딕",
    "Apple SD Gothic Neo",
    "Noto Sans CJK KR",
    "Noto Sans KR",
    "NanumGothic",
    "나눔고딕",
    "Segoe UI",
    "DejaVu Sans",
    "Arial",
];

pub fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut fonts = Vec::new();
        let mut names = Vec::new();
        for fam in FAMILIES {
            let q = fontdb::Query {
                families: &[fontdb::Family::Name(fam)],
                weight: fontdb::Weight::NORMAL,
                ..Default::default()
            };
            if let Some(id) = db.query(&q) {
                let mut loaded = None;
                db.with_face_data(id, |data, index| {
                    loaded = ab_glyph::FontVec::try_from_vec_and_index(data.to_vec(), index)
                        .ok()
                        .map(FontArc::new);
                });
                if let Some(f) = loaded {
                    fonts.push(f);
                    names.push(fam.to_string());
                }
            }
        }
        if fonts.is_empty() {
            // 어떤 글꼴이든 하나는 쓴다
            for face in db.faces() {
                let mut loaded = None;
                db.with_face_data(face.id, |data, index| {
                    loaded = ab_glyph::FontVec::try_from_vec_and_index(data.to_vec(), index)
                        .ok()
                        .map(FontArc::new);
                });
                if let Some(f) = loaded {
                    names.push(face.families.first().map(|f| f.0.clone()).unwrap_or_default());
                    fonts.push(f);
                    break;
                }
            }
        }
        Fonts { fonts, names }
    })
}

#[derive(Clone, Copy)]
struct Glyph {
    font: usize,
    id: GlyphId,
}

fn glyph_of(f: &Fonts, ch: char) -> Option<Glyph> {
    for (i, font) in f.fonts.iter().enumerate() {
        let id = font.glyph_id(ch);
        if id.0 != 0 {
            return Some(Glyph { font: i, id });
        }
    }
    f.fonts.first().map(|font| Glyph {
        font: 0,
        id: font.glyph_id(ch),
    })
}

pub struct Mask {
    pub left: i32,
    pub top: i32,
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

thread_local! {
    static CACHE: RefCell<HashMap<(usize, u16, u32), Option<Rc<Mask>>>> = RefCell::new(HashMap::new());
}

fn mask(f: &Fonts, g: Glyph, px: f32) -> Option<Rc<Mask>> {
    let key = (g.font, g.id.0, (px * 4.0).round() as u32);
    CACHE.with(|c| {
        if let Some(m) = c.borrow().get(&key) {
            return m.clone();
        }
        let font = &f.fonts[g.font];
        let glyph = g.id.with_scale_and_position(PxScale::from(px), ab_glyph::point(0.0, 0.0));
        let m = font.outline_glyph(glyph).map(|og| {
            let b = og.px_bounds();
            let w = b.width().ceil() as usize + 1;
            let h = b.height().ceil() as usize + 1;
            let mut data = vec![0u8; w * h];
            og.draw(|x, y, cov| {
                let (x, y) = (x as usize, y as usize);
                if x < w && y < h {
                    data[y * w + x] = (cov.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            });
            Rc::new(Mask {
                left: b.min.x.floor() as i32,
                top: b.min.y.floor() as i32,
                w,
                h,
                data,
            })
        });
        c.borrow_mut().insert(key, m.clone());
        m
    })
}

/// 글자 폭 (px 크기 기준)
pub fn measure(text: &str, px: f32) -> f32 {
    let f = fonts();
    let mut w = 0.0;
    for ch in text.chars() {
        if let Some(g) = glyph_of(f, ch) {
            w += f.fonts[g.font].as_scaled(PxScale::from(px)).h_advance(g.id);
        }
    }
    w
}

/// 글자의 위·아래 높이 (ascent, descent는 양수)
pub fn metrics(px: f32) -> (f32, f32) {
    let f = fonts();
    match f.fonts.first() {
        Some(font) => {
            let s = font.as_scaled(PxScale::from(px));
            (s.ascent(), -s.descent())
        }
        None => (px * 0.8, px * 0.2),
    }
}

/// 글리프를 하나씩 놓는다. visit(마스크, 픽셀 x, 픽셀 y)
pub fn layout(text: &str, px: f32, x: f32, baseline: f32, mut visit: impl FnMut(&Mask, i32, i32)) {
    let f = fonts();
    let mut pen = x;
    for ch in text.chars() {
        let Some(g) = glyph_of(f, ch) else { continue };
        let adv = f.fonts[g.font].as_scaled(PxScale::from(px)).h_advance(g.id);
        if !ch.is_whitespace() {
            if let Some(m) = mask(f, g, px) {
                visit(&m, pen.round() as i32 + m.left, baseline.round() as i32 + m.top);
            }
        }
        pen += adv;
    }
}

/// 폭을 넘치는 글자는 끝을 …로 줄인다
pub fn truncate(text: &str, px: f32, max: f32) -> String {
    if measure(text, px) <= max {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut end = chars.len();
    while end > 1 {
        end -= 1;
        let s: String = chars[..end].iter().collect::<String>() + "…";
        if measure(&s, px) <= max {
            return s;
        }
    }
    "…".into()
}
