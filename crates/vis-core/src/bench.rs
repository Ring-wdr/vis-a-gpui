//! 벤치마크. vis-a-lot `scripts/bench-db/run.mts`(PGlite 차트 데이터 쿼리)와 같은 케이스를 같은 데이터로 재고,
//! 렌더 job(쿼리 → item 렌더 → (선택) PNG 인코딩)을 끝까지 잰다. CLI(vis-bench)와 앱의 벤치 화면이 함께 쓴다.

use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;

use crate::dataset::Dataset;
use crate::generator::{START_MS, generate, sample_of};
use crate::job::{self, JobRequest, RenderOptions};
use crate::query::category::{CategoryQuery, query_category};
use crate::query::heatmap::query_heatmap;
use crate::query::histogram::query_histogram;
use crate::query::points::gather;
use crate::query::summary::{column_summary, item_rows, legend_values, page_rows};
use crate::query::{LegendCodes, column, group_rows, select_rows, split_count};
use crate::spec::{AggFn, ChartSpec, ChartTheme, ChartType, CommonProps, Filter, ItemState};

#[derive(Clone, Debug, Serialize)]
pub struct CaseResult {
    pub name: String,
    pub median_ms: f64,
    pub p95_ms: f64,
    pub min_ms: f64,
    pub runs: usize,
}

fn stats(name: &str, mut ms: Vec<f64>) -> CaseResult {
    ms.sort_by(|a, b| a.total_cmp(b));
    let n = ms.len();
    let median = if n % 2 == 1 { ms[n / 2] } else { (ms[n / 2 - 1] + ms[n / 2]) / 2.0 };
    let p95 = ms[((n as f64 * 0.95).ceil() as usize).clamp(1, n) - 1];
    CaseResult {
        name: name.into(),
        median_ms: median,
        p95_ms: p95,
        min_ms: ms[0],
        runs: n,
    }
}

/// warmup 1회 후 n회
pub fn measure(name: &str, n: usize, mut f: impl FnMut()) -> CaseResult {
    f();
    let ms = (0..n)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    stats(name, ms)
}

pub fn generate_sample(id: &str) -> (Arc<Dataset>, f64) {
    let s = sample_of(id).expect("sample id");
    let t = Instant::now();
    let d = generate(s.id, s.name, s.rows, |_| {});
    (Arc::new(d), t.elapsed().as_secs_f64() * 1000.0)
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// bench-db와 같은 이름·조건의 데이터 케이스
pub fn data_cases(ds: &Arc<Dataset>, n: usize, mut on: impl FnMut(&CaseResult)) -> Vec<CaseResult> {
    let week = 7.0 * 86_400_000.0;
    let ts_filter = Filter::Range {
        column: "ts".into(),
        min: START_MS + week,
        max: START_MS + 2.0 * week,
        inverse: false,
        include_null: false,
    };
    let exclude = Filter::Exclude {
        column: "status".into(),
        values: names(&["정상"]),
    };
    let d: &Dataset = ds;
    let mut out = vec![];
    let mut push = |r: CaseResult, out: &mut Vec<CaseResult>| {
        on(&r);
        out.push(r);
    };

    // scatter: 원본처럼 split 조합마다 x·y·행 id(·legend) 배열을 만들 때까지
    let scatter = |split: &[&str], legend: Option<&str>, filters: &[Filter]| {
        let x = column(d, "ts", &[]).unwrap();
        let y = column(d, "value", &[]).unwrap();
        let sc: Vec<_> = split.iter().map(|s| column(d, s, &[]).unwrap()).collect();
        let rows = select_rows(d, filters, &[x, y]).unwrap();
        let g = group_rows(&sc, rows).unwrap();
        let l = legend.map(|l| LegendCodes::of(d, l).unwrap());
        let items: Vec<_> = (0..g.len())
            .map(|i| gather(g.rows_of(i), x.nums().unwrap(), y.nums().unwrap(), l.as_ref()))
            .collect();
        std::hint::black_box(items);
    };
    push(measure("scatter ts×value", n, || scatter(&[], None, &[])), &mut out);
    push(
        measure("scatter split factory×line + legend", n, || {
            scatter(&["factory", "line"], Some("sensor_type"), &[])
        }),
        &mut out,
    );
    push(
        measure("scatter ts 1주 필터", n, || scatter(&[], None, std::slice::from_ref(&ts_filter))),
        &mut out,
    );

    let cat = |chart: ChartType, x: &str, agg: AggFn, split: &[&str], legend: Option<&str>| {
        let y = column(d, "value", &[]).unwrap();
        let sc: Vec<_> = split.iter().map(|s| column(d, s, &[]).unwrap()).collect();
        let rows = select_rows(d, &[], &[y]).unwrap();
        let g = group_rows(&sc, rows).unwrap();
        std::hint::black_box(
            query_category(
                &CategoryQuery {
                    ds: d,
                    chart,
                    x,
                    y: "value",
                    agg,
                    legend,
                },
                &g,
            )
            .unwrap(),
        );
    };
    push(
        measure("bar avg value by machine split factory", n, || {
            cat(ChartType::Bar, "machine", AggFn::Avg, &["factory"], None)
        }),
        &mut out,
    );
    push(
        measure("bar median by line legend sensor", n, || {
            cat(ChartType::Bar, "line", AggFn::Median, &[], Some("sensor_type"))
        }),
        &mut out,
    );
    push(
        measure("boxplot value by sensor_type", n, || {
            cat(ChartType::Boxplot, "sensor_type", AggFn::Avg, &[], None)
        }),
        &mut out,
    );
    push(
        measure("histogram value split sensor", n, || {
            let x = column(d, "value", &[]).unwrap();
            let rows = select_rows(d, &[], &[x]).unwrap();
            let g = group_rows(&[column(d, "sensor_type", &[]).unwrap()], rows).unwrap();
            std::hint::black_box(query_histogram(&g, x.nums().unwrap(), None));
        }),
        &mut out,
    );
    push(
        measure("heatmap ts×machine avg", n, || {
            let need = [
                column(d, "ts", &[]).unwrap(),
                column(d, "machine", &[]).unwrap(),
                column(d, "value", &[]).unwrap(),
            ];
            let rows = select_rows(d, &[], &need).unwrap();
            let g = group_rows(&[], rows).unwrap();
            std::hint::black_box(query_heatmap(d, &g, "ts", "machine", "value", AggFn::Avg).unwrap());
        }),
        &mut out,
    );
    push(
        measure("legendValues sensor_type", n, || {
            drop(std::hint::black_box(legend_values(d, "sensor_type").unwrap()))
        }),
        &mut out,
    );
    push(
        measure("columnSummary machine", n, || {
            drop(std::hint::black_box(column_summary(d, "machine").unwrap()))
        }),
        &mut out,
    );
    push(
        measure("columnSummary ts", n, || {
            drop(std::hint::black_box(column_summary(d, "ts").unwrap()))
        }),
        &mut out,
    );
    push(
        measure("splitCount 3컬럼", n, || {
            let _ = std::hint::black_box(split_count(d, &names(&["factory", "line", "lot_id"]), &[], 30_000).unwrap());
        }),
        &mut out,
    );
    push(
        measure("splitCount 필터", n, || {
            let _ = std::hint::black_box(split_count(d, &names(&["machine", "lot_id"]), std::slice::from_ref(&exclude), 30_000).unwrap());
        }),
        &mut out,
    );
    push(
        measure("sampleRows 필터 1페이지", n, || {
            drop(std::hint::black_box(page_rows(d, std::slice::from_ref(&exclude), 0, 100).unwrap()))
        }),
        &mut out,
    );
    push(
        measure("itemRows split 조합 1페이지", n, || {
            drop(std::hint::black_box(
                item_rows(d, &names(&["factory", "line"]), &names(&["판교", "L1"]), &[], 0, 100).unwrap(),
            ))
        }),
        &mut out,
    );
    out
}

/// 렌더 job 시나리오: 화면에서 draw를 눌렀을 때와 같은 일 (쿼리 + 모든 item 렌더 + 섬네일)
pub struct DrawScenario {
    pub name: &'static str,
    pub spec: ChartSpec,
}

pub fn spec(dataset: &str, chart: ChartType, x: &str, y: &str, split: &[&str], legend: Option<&str>) -> ChartSpec {
    ChartSpec {
        dataset: dataset.into(),
        chart_type: chart,
        x: x.into(),
        y: y.into(),
        z: None,
        agg: AggFn::Avg,
        legend: legend.map(Into::into),
        split_by: names(split),
        split_separator: "-".into(),
        tooltip: vec![],
        grid: (3, 3),
        common: CommonProps::default(),
        filters: vec![],
        sub_type: None,
        show_nearest: true,
    }
}

pub fn draw_scenarios(dataset: &str) -> Vec<DrawScenario> {
    vec![
        DrawScenario {
            name: "scatter ts×value (1 item)",
            spec: spec(dataset, ChartType::Scatter, "ts", "value", &[], None),
        },
        DrawScenario {
            name: "scatter split factory×line + legend (12 items)",
            spec: spec(
                dataset,
                ChartType::Scatter,
                "ts",
                "value",
                &["factory", "line"],
                Some("sensor_type"),
            ),
        },
        DrawScenario {
            name: "line split machine (120 items)",
            spec: spec(dataset, ChartType::Line, "ts", "value", &["machine"], None),
        },
        DrawScenario {
            name: "bar avg machine split sensor×shift (12 items)",
            spec: spec(dataset, ChartType::Bar, "machine", "value", &["sensor_type", "shift"], None),
        },
        DrawScenario {
            name: "boxplot sensor split machine (120 items)",
            spec: spec(dataset, ChartType::Boxplot, "sensor_type", "value", &["machine"], None),
        },
        DrawScenario {
            name: "histogram value split lot_id (~5,000 items)",
            spec: spec(dataset, ChartType::Histogram, "value", "value", &["lot_id"], None),
        },
    ]
}

#[derive(Clone, Debug, Serialize)]
pub struct DrawResult {
    pub name: String,
    pub items: usize,
    pub workers: usize,
    pub query_ms: f64,
    pub total_ms: f64,
    pub items_per_sec: f64,
    pub png: bool,
}

/// 한 번 draw (쿼리 + 렌더 전부). workers는 렌더 스레드 수
pub fn run_draw(ds: &Arc<Dataset>, spec: &ChartSpec, workers: usize, size: (f32, f32), png: bool) -> DrawResult {
    let job = job::start(JobRequest {
        dataset: ds.clone(),
        spec: spec.clone(),
        data: None,
        states: Arc::new(|_| ItemState::default()),
        only: None,
        opts: RenderOptions {
            separator: None,
            common: None,
            width: size.0,
            height: size.1,
            scale: 1.0,
            theme: ChartTheme::Light,
            thumbnail: Some((128.0, 80.0)),
            encode_png: png,
        },
        workers,
    });
    job.wait();
    let p = &job.progress;
    if let Some(e) = p.error.lock().clone() {
        panic!("draw failed: {e}");
    }
    let total = p.elapsed().as_secs_f64() * 1000.0;
    let items = p.total.load(std::sync::atomic::Ordering::Acquire);
    DrawResult {
        name: String::new(),
        items,
        workers,
        query_ms: p.query_ms.lock().unwrap_or(0.0),
        total_ms: total,
        items_per_sec: items as f64 / (total / 1000.0),
        png,
    }
}
