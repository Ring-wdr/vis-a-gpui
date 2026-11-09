//! 차트 도구의 다운로드 (원본 4.7.1): 차트 PNG, 모든 차트 엑셀(300개까지), 데이터 CSV(BOM, 선택이면 `_선택`).
//! 파일 저장 위치는 OS 저장 대화상자로 고른다. 무거운 일(엑셀 그리기·인코딩)은 백그라운드 스레드에서 한다.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

use vis_core::dataset::Dataset;
use vis_core::job::{ChartData, RenderOptions, render_item};
use vis_core::spec::ItemState;

/// 엑셀에 넣는 차트 수 상한
pub const EXCEL_MAX: usize = 300;

/// 파일 이름에 쓸 수 없는 글자를 바꾼다
pub fn safe_name(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() {
        "chart".into()
    } else {
        s.chars().take(80).collect()
    }
}

fn downloads_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join("Downloads"))
        .filter(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

/// 저장 위치를 물은 뒤 make()로 만든 바이트를 백그라운드에서 쓴다
pub fn save_with<V: 'static>(
    name: String,
    make: impl FnOnce() -> Result<Vec<u8>, String> + Send + 'static,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let rx = cx.prompt_for_new_path(&downloads_dir(), Some(&name));
    cx.spawn_in(window, async move |_, cx| {
        let Ok(Ok(Some(path))) = rx.await else { return };
        let (tx, done) = std::sync::mpsc::channel();
        let p = path.clone();
        std::thread::spawn(move || {
            let r = make().and_then(|bytes| std::fs::write(&p, bytes).map_err(|e| e.to_string()));
            let _ = tx.send(r);
        });
        loop {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            if let Ok(r) = done.try_recv() {
                let msg = match r {
                    Ok(()) => format!("저장했습니다: {}", path.display()),
                    Err(e) => format!("저장하지 못했습니다: {e}"),
                };
                let _ = cx.update(|window, cx| window.push_notification(msg, cx));
                break;
            }
        }
    })
    .detach();
}

/// CSV (UTF-8 BOM, 엑셀이 한글을 바로 읽게). 값에 쉼표·따옴표·줄바꿈이 있으면 따옴표로 감싼다
pub fn csv_bytes(ds: &Dataset, rows: &[u32]) -> Vec<u8> {
    let quote = |s: &str| {
        if s.contains([',', '"', '\n', '\r']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    let mut out = String::with_capacity(rows.len() * 64 + 256);
    out.push('\u{feff}');
    let header: Vec<String> = ds.columns.iter().map(|c| quote(&c.name)).collect();
    out.push_str(&header.join(","));
    out.push_str("\r\n");
    for &r in rows {
        let line: Vec<String> = ds.columns.iter().map(|c| quote(&c.text_at(r as usize))).collect();
        out.push_str(&line.join(","));
        out.push_str("\r\n");
    }
    out.into_bytes()
}

/// 모든 차트 엑셀: 시트 하나에 item 이름과 PNG를 위에서부터 차례로 놓는다
pub fn excel_bytes(data: Arc<ChartData>, states: Vec<ItemState>, opts: RenderOptions) -> Result<Vec<u8>, String> {
    use rust_xlsxwriter::{Format, Image, Workbook};
    let n = data.len().min(EXCEL_MAX);
    let (w, h) = (opts.width, opts.height);
    // 그리기는 병렬로 (렌더 스레드와 같은 rayon 풀)
    use rayon::prelude::*;
    let pngs: Vec<(String, Vec<u8>)> = (0..n)
        .into_par_iter()
        .map(|i| {
            let st = states.get(i).cloned().unwrap_or_default();
            let r = render_item(&data, i, &st, &opts, false, (w, h));
            (data.item_name(i), r.canvas.encode_png())
        })
        .collect();
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("charts").map_err(|e| e.to_string())?;
    ws.set_column_width(0, 28).map_err(|e| e.to_string())?;
    let bold = Format::new().set_bold();
    ws.write_string_with_format(0, 0, "item", &bold).map_err(|e| e.to_string())?;
    ws.write_string_with_format(0, 1, "차트", &bold).map_err(|e| e.to_string())?;
    // 기본 행 높이 20px
    let rows_per = ((h * opts.scale) / 20.0).ceil() as u32 + 1;
    for (k, (name, png)) in pngs.iter().enumerate() {
        let row = 1 + k as u32 * rows_per;
        ws.write_string(row, 0, name).map_err(|e| e.to_string())?;
        let img = Image::new_from_buffer(png).map_err(|e| e.to_string())?;
        ws.insert_image(row, 1, &img).map_err(|e| e.to_string())?;
    }
    wb.save_to_buffer().map_err(|e| e.to_string())
}
