//! 헤드리스 벤치마크. vis-a-lot `pnpm bench:db`와 같은 케이스 + 렌더 job을 잰다.
//!
//! 사용: vis-bench [--rows 100000|500000|1000000] [--iterations 5] [--workers N] [--png] [--out result.json]
//!                 [--skip-data] [--skip-draw]

use std::time::Instant;

use vis_core::bench::{self, DrawResult};
use vis_core::job::default_workers;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let has = |k: &str| args.iter().any(|a| a == k);
    let rows: usize = get("--rows").and_then(|v| v.parse().ok()).unwrap_or(1_000_000);
    let n: usize = get("--iterations").and_then(|v| v.parse().ok()).unwrap_or(5);
    let workers: usize = get("--workers").and_then(|v| v.parse().ok()).unwrap_or_else(default_workers);
    let id = match rows {
        100_000 => "iot-100k",
        500_000 => "iot-500k",
        _ => "iot-1m",
    };
    let fonts = vis_core::render::text::fonts();
    eprintln!("fonts: {:?}", fonts.names);
    eprintln!(
        "threads: {} (render workers {workers})",
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0)
    );

    let (ds, gen_ms) = bench::generate_sample(id);
    if let Some(dir) = get("--samples") {
        render_samples(&ds, &dir);
        return;
    }
    println!("dataset {id} generated in {gen_ms:.0} ms ({} rows)", ds.rows);
    let tmp = std::env::temp_dir().join(format!("vis-bench-{id}.bin"));
    let t = Instant::now();
    ds.save(&tmp).unwrap();
    let save_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let _ = vis_core::dataset::Dataset::load(&tmp).unwrap();
    let load_ms = t.elapsed().as_secs_f64() * 1000.0;
    let _ = std::fs::remove_file(&tmp);
    println!("save {save_ms:.0} ms, reopen {load_ms:.0} ms");

    let mut report = serde_json::json!({
        "rows": rows, "iterations": n, "workers": workers,
        "threads": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        "generateMs": gen_ms, "saveMs": save_ms, "reopenMs": load_ms,
    });
    if !has("--skip-data") {
        let res = bench::data_cases(&ds, n, |r| {
            println!("{:<42} median {:>9.2} ms  p95 {:>9.2} ms", r.name, r.median_ms, r.p95_ms);
        });
        report["data"] = serde_json::to_value(&res).unwrap();
    }
    if !has("--skip-draw") {
        let png = has("--png");
        let mut draws: Vec<DrawResult> = vec![];
        for s in bench::draw_scenarios(id) {
            // warmup 1회 후 중앙값 대신 3회 중 최솟값·중앙값
            let _ = bench::run_draw(&ds, &s.spec, workers, (400.0, 300.0), png);
            let mut runs: Vec<DrawResult> = (0..3)
                .map(|_| bench::run_draw(&ds, &s.spec, workers, (400.0, 300.0), png))
                .collect();
            runs.sort_by(|a, b| a.total_ms.total_cmp(&b.total_ms));
            let mut r = runs.swap_remove(1);
            r.name = s.name.into();
            println!(
                "draw {:<48} {:>6} items  query {:>8.1} ms  total {:>9.1} ms  {:>8.0} item/s{}",
                r.name,
                r.items,
                r.query_ms,
                r.total_ms,
                r.items_per_sec,
                if png { "  (+png)" } else { "" }
            );
            draws.push(r);
        }
        report["draw"] = serde_json::to_value(&draws).unwrap();
    }
    if let Some(out) = get("--out") {
        std::fs::write(&out, serde_json::to_string_pretty(&report).unwrap()).unwrap();
        println!("wrote {out}");
    }
}

/// 차트 종류마다 PNG 한 장 (렌더러 눈으로 확인)
fn render_samples(ds: &std::sync::Arc<vis_core::dataset::Dataset>, dir: &str) {
    use vis_core::job::{RenderOptions, query, render_item};
    use vis_core::spec::{AggFn, ChartTheme, ChartType, ItemState};
    std::fs::create_dir_all(dir).unwrap();
    let id = ds.id.clone();
    let mk = |c: ChartType, x: &str, y: &str, split: &[&str], legend: Option<&str>| bench::spec(&id, c, x, y, split, legend);
    let mut specs = vec![
        mk(ChartType::Scatter, "ts", "value", &["sensor_type"], Some("status")),
        mk(ChartType::Line, "ts", "value", &["machine"], Some("sensor_type")),
        mk(ChartType::Bar, "line", "value", &["sensor_type"], Some("factory")),
        mk(ChartType::Boxplot, "factory", "value", &["sensor_type"], Some("shift")),
        mk(ChartType::Histogram, "value", "value", &["sensor_type"], Some("status")),
        mk(ChartType::Candle, "ts", "value", &["sensor_type"], None),
        mk(ChartType::Gantt, "ts", "machine", &["factory"], Some("status")),
        mk(ChartType::Pie, "machine", "value", &["sensor_type"], None),
        mk(ChartType::Radar, "line", "value", &["sensor_type"], Some("factory")),
        mk(ChartType::Treemap, "line", "value", &["sensor_type"], Some("factory")),
        mk(ChartType::Waterfall, "line", "value", &["sensor_type"], None),
        mk(ChartType::Funnel, "machine", "value", &["sensor_type"], None),
    ];
    let mut heat = mk(ChartType::Heatmap, "ts", "line", &["sensor_type"], None);
    heat.z = Some("value".into());
    specs.push(heat);
    let mut sankey = mk(ChartType::Sankey, "factory", "machine", &["sensor_type"], None);
    sankey.z = Some("value".into());
    sankey.agg = AggFn::Count;
    specs.push(sankey);
    let opts = RenderOptions {
        separator: None,
        common: None,
        width: 520.0,
        height: 360.0,
        scale: 2.0,
        theme: ChartTheme::Light,
        thumbnail: None,
        encode_png: false,
    };
    for s in specs {
        let t = std::time::Instant::now();
        let data = query(ds, &s).unwrap();
        let q = t.elapsed().as_secs_f64() * 1000.0;
        let t = std::time::Instant::now();
        let r = render_item(&data, 0, &ItemState::default(), &opts, false, (opts.width, opts.height));
        let rm = t.elapsed().as_secs_f64() * 1000.0;
        let path = format!("{dir}/{}.png", s.chart_type.name());
        std::fs::write(&path, r.canvas.encode_png()).unwrap();
        println!(
            "{path}: {} items, query {q:.1} ms, render {rm:.1} ms, hits {}",
            data.len(),
            r.meta.hits.len()
        );
    }
}
