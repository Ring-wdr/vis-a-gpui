//! 하위 차트·차트 속성·선택·도형을 PNG로 그려 본다: cargo run --release -p vis-core --example variants -- <출력 폴더>
use std::sync::Arc;

use serde_json::json;
use vis_core::bench;
use vis_core::job::{RenderOptions, query, render_item};
use vis_core::region::{NamedRegion, Region, Shape};
use vis_core::spec::{ChartTheme, ChartType, ItemState, SubType};

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    std::fs::create_dir_all(&out).unwrap();
    let (ds, _) = bench::generate_sample("iot-100k");
    let ds = Arc::new(ds);
    let names: Vec<String> = ds.columns.iter().map(|c| format!("{}:{:?}", c.name, c.kind)).collect();
    println!("{}", names.join(", "));
    let opts = RenderOptions {
        separator: None,
        common: None,
        width: 480.0,
        height: 320.0,
        scale: 1.0,
        theme: ChartTheme::Light,
        thumbnail: None,
        encode_png: true,
    };
    let mut cases: Vec<(&str, vis_core::spec::ChartSpec, ItemState)> = vec![];
    let base = |c: ChartType, x: &str, y: &str, legend: Option<&str>| bench::spec("iot-100k", c, x, y, &[], legend);
    let st = ItemState::default;
    for (name, sub) in [("bar-stack", SubType::Stack), ("bar-stack100", SubType::Stack100)] {
        let mut s = base(ChartType::Bar, "factory", "value", Some("status"));
        s.sub_type = Some(sub);
        cases.push((name, s, st()));
    }
    for (name, sub) in [
        ("pie-doughnut", SubType::Doughnut),
        ("pie-3d", SubType::Pie3d),
        ("pie-sunburst", SubType::Sunburst),
    ] {
        let mut s = base(ChartType::Pie, "factory", "value", Some("status"));
        s.agg = vis_core::spec::AggFn::Count;
        s.sub_type = Some(sub);
        cases.push((name, s, st()));
    }
    for (name, sub) in [("line-ecdf", SubType::Ecdf), ("line-probability", SubType::Probability)] {
        let mut s = base(ChartType::Line, "ts", "value", Some("factory"));
        s.sub_type = Some(sub);
        cases.push((name, s, st()));
    }
    let mut s = base(ChartType::Boxplot, "factory", "value", None);
    s.sub_type = Some(SubType::Dot);
    cases.push(("boxplot-dot", s, st()));
    // 선택 + 도형 + 새 범례 + 지운 영역 (scatter)
    let s = base(ChartType::Scatter, "temperature_env", "value", None);
    let mut state = st();
    state.selection = Some(Region::Lasso {
        points: vec![(20.0, 20.0), (30.0, 80.0), (40.0, 30.0)],
    });
    state.shapes = vec![
        Shape::Rect {
            x0: 10.0,
            y0: 10.0,
            x1: 15.0,
            y1: 40.0,
        },
        Shape::Text {
            x: 12.0,
            y: 60.0,
            text: "메모".into(),
        },
    ];
    state.legend_groups = vec![NamedRegion {
        name: "고온".into(),
        region: Region::Rect {
            x0: 35.0,
            y0: 0.0,
            x1: 50.0,
            y1: 200.0,
        },
    }];
    state.deletions = vec![Region::Rect {
        x0: 0.0,
        y0: 0.0,
        x1: 15.0,
        y1: 15.0,
    }];
    cases.push(("scatter-selection", s, state));
    // 차트 속성: 제목·축·패널·점수·예측·강조
    let mut s = base(ChartType::Line, "ts", "value", None);
    s.common.options = json!({
        "title": { "text": "사용자 제목", "subtitle": "부제목", "align": "center" },
        "axes": { "x": { "title": { "visible": true }, "grid": { "visible": true } }, "y": { "title": { "visible": true }, "decimals": 1 } },
        "panel": { "border": true, "gradient": true },
        "score": { "visible": true, "kinds": ["rscore", "zscore"], "ucl": 80, "lcl": 10 },
        "prediction": { "enabled": true, "method": "des", "period": 40, "showInterval": true },
        "focus": { "enabled": true, "operator": ">", "value": 70 }
    });
    s.filters = vec![];
    cases.push(("line-options", s, st()));
    let mut s = base(ChartType::Bar, "factory", "value", None);
    s.common.options = json!({
        "pointLabel": { "visible": true, "box": { "visible": true } },
        "shape": { "cornerRadius": 0, "style": "border", "borderWidth": 2 },
        "axes": { "x": { "label": { "angle": -40 } }, "y": { "log": true } },
        "focus": { "enabled": true, "value": 50 }
    });
    let mut state = st();
    state.selection = Some(Region::Rect {
        x0: -0.5,
        y0: 0.0,
        x1: 1.5,
        y1: 1000.0,
    });
    cases.push(("bar-options", s, state));
    for (name, spec, state) in cases {
        let data = match query(&ds, &spec) {
            Ok(d) => d,
            Err(e) => {
                println!("{name}: {}", e.0);
                continue;
            }
        };
        let r = render_item(&data, 0, &state, &opts, false, (opts.width, opts.height));
        std::fs::write(format!("{out}/{name}.png"), r.canvas.encode_png()).unwrap();
        println!("{name}: {} · {}", r.meta.summary, r.meta.hits.len());
    }
}
