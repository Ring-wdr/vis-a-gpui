//! vis-a-gpui: vis-a-lot 시각화 화면을 GPUI(gpui-kit)로 옮긴 Windows 데스크톱 앱.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod bench_view;
mod filters;
mod ui;
mod workspace;

use gpui_kit::*;

fn main() {
    // 글꼴은 첫 렌더 전에 미리 찾아 둔다 (시스템 글꼴 목록 읽기)
    std::thread::spawn(|| {
        let _ = vis_core::render::text::fonts();
    });
    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(|cx| {
        gpui_kit::init(cx);
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("vis-a-gpui".into()),
                ..Default::default()
            }),
            window_min_size: Some(size(px(960.), px(600.))),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| app::VisApp::new(window, cx))).expect("failed to open window");
        cx.activate(true);
    });
}
