//! 렌더 job. 원본은 MSW → chart 서비스 → Job Manager가 DB Worker(PGlite)에 쿼리하고 Render Worker N개
//! (OffscreenCanvas → PNG)에 item을 나눠 줬으며, 화면은 long polling으로 진행을 받았다.
//! 여기서는 같은 단계를 한 프로세스에서 돈다: 쿼리(rayon) → 렌더 스레드 N개가 item을 순서대로 가져가 그린다.
//! 화면은 진행 상태(원자 변수)와 끝난 item을 주기적으로 읽는다.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::dataset::{ColumnKind, Dataset};
use crate::query::candle::{CandleItem, query_candle};
use crate::query::category::{CategoryItem, CategoryQuery, query_category};
use crate::query::gantt::{GanttItem, query_gantt};
use crate::query::heatmap::{HeatItem, query_heatmap};
use crate::query::histogram::{Bins, HistItem, query_histogram};
use crate::query::points::gather;
use crate::query::sankey::{SankeyItem, query_sankey};
use crate::query::{Groups, LegendCodes, QResult, QueryError, column, group_rows, select_rows};
use crate::render::frame::LegendInput;
use crate::render::meta::ItemMeta;
use crate::render::theme::{OTHERS_COLOR, palette, parse_hex};
use crate::render::{ItemData, ItemRender, render};
use crate::spec::{ChartSpec, ChartTheme, ChartType, ItemState};

/// 쿼리 결과 (차트 종류별)
pub enum Payload {
    /// 열 번호만 둔다. 점은 렌더할 때 그룹의 행 번호로 열 배열에서 바로 모은다 (복사 없음)
    Points {
        x: usize,
        y: usize,
        x_time: bool,
        legend: Option<usize>,
    },
    Category(Vec<CategoryItem>),
    Histogram(Bins, Vec<HistItem>),
    Heatmap(Vec<HeatItem>),
    Candle(Vec<CandleItem>),
    Gantt(Vec<GanttItem>),
    Sankey(Vec<SankeyItem>),
}

/// draw 한 번의 데이터. item 다시 그리기(확대·legend 숨김·크기 변경)는 이 결과를 다시 쓴다.
pub struct ChartData {
    pub dataset: Arc<Dataset>,
    pub spec: ChartSpec,
    pub groups: Groups,
    pub payload: Payload,
    pub legend_values: Option<Vec<String>>,
    pub legend_colors: Vec<u32>,
    /// 쿼리에 걸린 시간
    pub query_ms: f64,
}

impl ChartData {
    pub fn len(&self) -> usize {
        self.groups.len()
    }
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
    pub fn item_name(&self, i: usize) -> String {
        self.item_name_with(i, &self.spec.split_separator)
    }
    pub fn item_name_with(&self, i: usize, sep: &str) -> String {
        let l = &self.groups.labels[i];
        if l.is_empty() { self.spec.summary() } else { l.join(sep) }
    }
}

/// 쿼리: 필터 → split 조합 → 차트별 집계
pub fn query(dataset: &Arc<Dataset>, spec: &ChartSpec) -> QResult<ChartData> {
    let started = Instant::now();
    let ds: &Dataset = dataset;
    let fields = spec.chart_type.fields();
    let x = column(ds, &spec.x, fields.x)?;
    let y = match fields.y {
        Some(kinds) => Some(column(ds, &spec.y, kinds)?),
        None => None,
    };
    let z = match fields.z {
        Some(kinds) => Some(column(
            ds,
            spec.z.as_deref().ok_or_else(|| QueryError("z 컬럼을 고르세요.".into()))?,
            kinds,
        )?),
        None => None,
    };
    let legend_name = spec.legend_column();
    let legend = match legend_name {
        Some(l) => Some(LegendCodes::of(ds, l)?),
        None => None,
    };
    let split: Vec<_> = spec.split_by.iter().map(|s| column(ds, s, &[])).collect::<QResult<_>>()?;
    let mut need = vec![x];
    need.extend(y);
    need.extend(z);
    let rows = select_rows(ds, &spec.filters, &need)?;
    let groups = group_rows(&split, rows)?;
    let legend_values = legend.as_ref().map(|l| l.values.clone());

    let payload = match spec.chart_type {
        ChartType::Scatter | ChartType::Line => Payload::Points {
            x: ds.column_index(&spec.x).unwrap(),
            y: ds.column_index(&spec.y).unwrap(),
            x_time: x.kind == ColumnKind::Datetime,
            legend: legend_name.and_then(|l| ds.column_index(l)),
        },
        ChartType::Bar
        | ChartType::Boxplot
        | ChartType::Pie
        | ChartType::Radar
        | ChartType::Treemap
        | ChartType::Waterfall
        | ChartType::Funnel => {
            let (items, _) = query_category(
                &CategoryQuery {
                    ds,
                    chart: spec.chart_type,
                    x: &spec.x,
                    y: &spec.y,
                    agg: spec.agg,
                    legend: legend_name,
                },
                &groups,
            )?;
            Payload::Category(items)
        }
        ChartType::Histogram => {
            let (bins, items) = query_histogram(&groups, x.nums().unwrap(), legend.as_ref());
            Payload::Histogram(bins, items)
        }
        ChartType::Heatmap => Payload::Heatmap(query_heatmap(ds, &groups, &spec.x, &spec.y, spec.z.as_deref().unwrap(), spec.agg)?),
        ChartType::Candle => Payload::Candle(query_candle(&groups, x.nums().unwrap(), y.unwrap().nums().unwrap()).1),
        ChartType::Gantt => {
            let (codes, dict) = y.unwrap().cat().unwrap();
            Payload::Gantt(query_gantt(&groups, x.nums().unwrap(), codes, dict, legend.as_ref()))
        }
        ChartType::Sankey => {
            let (xc, xd) = x.cat().unwrap();
            let (yc, yd) = y.unwrap().cat().unwrap();
            Payload::Sankey(query_sankey(&groups, xc, xd, yc, yd, z.unwrap().nums().unwrap(), spec.agg))
        }
    };
    let legend_colors = legend_values
        .as_ref()
        .map(|v| (0..v.len()).map(palette).collect())
        .unwrap_or_default();
    Ok(ChartData {
        dataset: dataset.clone(),
        spec: spec.clone(),
        groups,
        payload,
        legend_values,
        legend_colors,
        query_ms: started.elapsed().as_secs_f64() * 1000.0,
    })
}

pub struct RenderedItem {
    pub index: usize,
    pub width: u32,
    pub height: u32,
    /// CSS 픽셀 크기
    pub css: (f32, f32),
    pub bgra: Vec<u8>,
    pub meta: ItemMeta,
    pub thumb: Option<(u32, u32, Vec<u8>)>,
    pub ms: f64,
    pub worker: usize,
}

#[derive(Clone)]
pub struct RenderOptions {
    /// split 구분 기호 덮어쓰기 (전체 설정에서 바꾸면 쿼리를 다시 하지 않고 제목만 바꾼다)
    pub separator: Option<String>,
    /// 공통 속성 덮어쓰기. 화면에서 전체 설정을 바꾸면 쿼리를 다시 하지 않고 이 값으로 다시 그린다
    pub common: Option<crate::spec::CommonProps>,
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub theme: ChartTheme,
    pub thumbnail: Option<(f32, f32)>,
    /// PNG 인코딩까지 잴지 (원본과 같은 조건 비교용). 화면은 끈다
    pub encode_png: bool,
}

/// item 하나 그리기
pub fn render_item(
    data: &ChartData,
    index: usize,
    state: &ItemState,
    opts: &RenderOptions,
    compact: bool,
    size: (f32, f32),
) -> crate::render::Rendered {
    let spec_owned;
    let spec = match &opts.common {
        Some(c) => {
            let mut sp = data.spec.clone();
            sp.common = c.clone();
            spec_owned = sp;
            &spec_owned
        }
        None => &data.spec,
    };
    // 색 결정 순서: item 개별 → 공통 속성 → 팔레트 기본 색 (원본 4.7)
    let color = state
        .color
        .as_deref()
        .and_then(parse_hex)
        .or_else(|| parse_hex(&spec.common.color))
        .unwrap_or(palette(0));
    let o = crate::options::resolve(spec.chart_type, opts.theme, &spec.common.options, &state.options);
    let alpha = (1.0 - spec.common.transparency.clamp(0.0, 0.95)) * (1.0 - o.shape.transparency.clamp(0.0, 0.95));
    // 팔레트: 그라데이션이면 시작 → 끝 단계 색, 아니면 기본 팔레트. '기타'는 팔레트 › 기타 색
    let others = parse_hex(&o.palette.others_color).unwrap_or(OTHERS_COLOR);
    let base_colors = |values: &[String]| -> Vec<u32> {
        let grad = o
            .palette
            .gradient
            .then(|| crate::render::theme::gradient_palette(&o.palette, values.len()));
        values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                if v == crate::query::category::OTHERS {
                    others
                } else {
                    grad.as_ref().map(|g| g[i]).unwrap_or_else(|| palette(i))
                }
            })
            .collect()
    };
    let hidden_of = |values: &[String]| values.iter().map(|v| state.hidden_legend.contains(v)).collect::<Vec<_>>();
    let mk_legend = |title: &str, values: Vec<String>, colors: Vec<u32>| {
        let hidden = hidden_of(&values);
        let colors = values
            .iter()
            .zip(colors)
            .map(|(v, c)| {
                state
                    .legend_colors
                    .get(v)
                    .or_else(|| spec.common.legend_colors.get(v))
                    .and_then(|h| parse_hex(h))
                    .unwrap_or(c)
            })
            .collect();
        Some(LegendInput {
            title: title.into(),
            values,
            colors,
            hidden,
            position: state.legend_position.unwrap_or(spec.common.legend_position),
        })
    };
    let mut legend = data
        .legend_values
        .as_ref()
        .and_then(|v| mk_legend(spec.legend.as_deref().unwrap_or(""), v.clone(), base_colors(v)));
    let rows = data.groups.rows_of(index);
    let points_store;
    let item_data = match &data.payload {
        Payload::Points { x, y, x_time, legend: lg } => {
            let ds = &data.dataset;
            let codes = lg.and_then(|l| LegendCodes::of(ds, &ds.columns[l].name).ok());
            let mut pd = gather(rows, ds.columns[*x].nums().unwrap(), ds.columns[*y].nums().unwrap(), codes.as_ref());
            let mut x_time = *x_time;
            // 식 (x, y에 적용). 값이 유한하지 않은 점은 뺀다
            let ex = crate::analytics::Expr::compile(&o.expression.x).ok().flatten();
            let ey = crate::analytics::Expr::compile(&o.expression.y).ok().flatten();
            if ex.is_some() || ey.is_some() {
                for i in 0..pd.xs.len() {
                    let (vx, vy) = (pd.xs[i], pd.ys[i]);
                    if let Some(e) = &ex {
                        pd.xs[i] = e.eval(vx, vy);
                    }
                    if let Some(e) = &ey {
                        pd.ys[i] = e.eval(vx, vy);
                    }
                }
                pd.retain(|xs, ys, i| xs[i].is_finite() && ys[i].is_finite());
                if ex.is_some() {
                    x_time = false;
                }
            }
            // 값 › 이상치 빼기 (y 기준)
            if o.value.outlier {
                let (lo, hi) = crate::analytics::outlier_bounds(&pd.ys, o.value.outlier_method, o.value.outlier_k);
                pd.retain(|_, ys, i| ys[i] >= lo && ys[i] <= hi);
            }
            // 지운 영역 (scatter, line)
            if !state.deletions.is_empty() && spec.can_delete() {
                let tests: Vec<_> = state.deletions.iter().map(|r| r.tester()).collect();
                pd.retain(|xs, ys, i| !tests.iter().any(|t| t(xs[i], ys[i])));
            }
            // line 분포 하위 차트: x = 원래 y 값, y = 누적 비율 등
            if let Some(kind) = spec.distribution() {
                let d = crate::analytics::to_distribution(kind, &pd.ys, pd.legend.as_deref());
                pd = crate::query::points::PointsData {
                    xs: d.iter().map(|p| p.1).collect(),
                    ys: d.iter().map(|p| p.2).collect(),
                    ids: d.iter().map(|p| pd.ids[p.0]).collect(),
                    legend: pd.legend.as_ref().map(|l| d.iter().map(|p| l[p.0]).collect()),
                };
                x_time = false;
            }
            // 새 범례: 영역 안의 점을 새 legend 항목으로 (legend 컬럼이 없으면 나머지는 '기타')
            if !state.legend_groups.is_empty() {
                let mut values = data.legend_values.clone().unwrap_or_else(|| vec!["기타".to_string()]);
                let base = values.len();
                let mut colors = base_colors(&values);
                if data.legend_values.is_none() {
                    colors[0] = color;
                }
                let tests: Vec<_> = state.legend_groups.iter().map(|g| g.region.tester()).collect();
                let mut codes = pd.legend.take().unwrap_or_else(|| vec![0; pd.xs.len()]);
                for i in 0..pd.xs.len() {
                    if let Some(k) = tests.iter().rposition(|t| t(pd.xs[i], pd.ys[i])) {
                        codes[i] = (base + k).min(255) as u8;
                    }
                }
                pd.legend = Some(codes);
                for (k, g) in state.legend_groups.iter().enumerate() {
                    values.push(g.name.clone());
                    colors.push(palette(base + k));
                }
                legend = mk_legend(spec.legend.as_deref().unwrap_or(""), values, colors);
            }
            points_store = pd;
            ItemData::Points {
                line: spec.chart_type == ChartType::Line,
                x_time,
                data: &points_store,
            }
        }
        Payload::Category(items) => {
            let item = &items[index];
            match spec.chart_type {
                ChartType::Pie | ChartType::Funnel => {
                    // sunburst는 legend 컬럼 값별 색 (바깥 고리)도 함께 쓴다: 조각 색은 범주, 바깥은 legend 값
                    let colors = base_colors(&item.categories);
                    legend = mk_legend(&spec.x, item.categories.clone(), colors);
                }
                ChartType::Waterfall => {
                    legend = mk_legend(
                        "",
                        vec!["증가".into(), "감소".into(), "합계".into()],
                        vec![palette(2), palette(7), palette(0)],
                    );
                }
                _ => {}
            }
            ItemData::Category { item, agg: spec.agg }
        }
        Payload::Histogram(bins, items) => ItemData::Histogram {
            bins: *bins,
            item: &items[index],
        },
        Payload::Heatmap(items) => ItemData::Heatmap(&items[index]),
        Payload::Candle(items) => {
            legend = mk_legend("", vec!["상승".into(), "하락".into()], vec![0xe34948, 0x2a78d6]);
            ItemData::Candle(&items[index])
        }
        Payload::Gantt(items) => ItemData::Gantt(&items[index]),
        Payload::Sankey(items) => {
            let item = &items[index];
            let colors = base_colors(&item.sources);
            legend = mk_legend(&spec.x, item.sources.clone(), colors);
            ItemData::Sankey(item)
        }
    };
    let input = ItemRender {
        chart: spec.chart_type,
        width: size.0,
        height: size.1,
        scale: opts.scale,
        theme: opts.theme,
        compact,
        title: data.item_name_with(index, opts.separator.as_deref().unwrap_or(&data.spec.split_separator)),
        color,
        alpha,
        legend,
        view: state.view,
        x_title: match spec.distribution() {
            Some(_) => spec.y.clone(),
            None => spec.x.clone(),
        },
        y_title: match spec.distribution() {
            Some(k) => k.y_title().to_string(),
            None => spec.y.clone(),
        },
        data: item_data,
        options: o.clone(),
        sub_type: spec.sub_type(),
        selection: state.selection.clone(),
        shapes: state.shapes.clone(),
        aux_labels: data.legend_values.clone().unwrap_or_default(),
        agg: spec.chart_type.fields().agg.then_some(spec.agg),
    };
    render(&input, &|_| {})
}

// ── job: 진행 상태와 렌더 스레드 ────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum JobStage {
    Query = 0,
    Render = 1,
    Done = 2,
    Failed = 3,
    Cancelled = 4,
}

pub struct JobProgress {
    pub total: AtomicUsize,
    pub done: AtomicUsize,
    pub running: AtomicUsize,
    pub failed: AtomicUsize,
    stage: AtomicU8,
    pub cancel: AtomicBool,
    pub started: Instant,
    pub query_ms: Mutex<Option<f64>>,
    pub finished: Mutex<Option<Duration>>,
    pub error: Mutex<Option<String>>,
    pub workers: usize,
}

impl JobProgress {
    pub fn stage(&self) -> JobStage {
        match self.stage.load(Ordering::Acquire) {
            0 => JobStage::Query,
            1 => JobStage::Render,
            2 => JobStage::Done,
            3 => JobStage::Failed,
            _ => JobStage::Cancelled,
        }
    }
    fn set_stage(&self, s: JobStage) {
        self.stage.store(s as u8, Ordering::Release);
    }
    pub fn is_finished(&self) -> bool {
        matches!(self.stage(), JobStage::Done | JobStage::Failed | JobStage::Cancelled)
    }
    pub fn elapsed(&self) -> Duration {
        self.finished.lock().unwrap_or_else(|| self.started.elapsed())
    }
}

/// 쿼리가 끝나면 채워지는 칸: 데이터, item 결과, 그릴 item 번호
pub struct JobSlots {
    pub data: Arc<ChartData>,
    /// 끝난 item. 화면은 take()로 가져가 비트맵을 GPU 이미지로 옮긴다 (복사 없음)
    pub results: Vec<Mutex<Option<RenderedItem>>>,
    pub indices: Vec<usize>,
}

/// draw 한 번 (또는 다시 그리기). 결과는 item마다 끝나는 대로 채워진다.
#[derive(Clone)]
pub struct Job {
    pub progress: Arc<JobProgress>,
    pub slots: Arc<OnceLock<JobSlots>>,
}

impl Job {
    pub fn data(&self) -> Option<&Arc<ChartData>> {
        self.slots.get().map(|s| &s.data)
    }
    /// 끝난 item을 가져간다 (한 번만)
    pub fn take(&self, i: usize) -> Option<RenderedItem> {
        self.slots.get().and_then(|s| s.results.get(i)).and_then(|c| c.lock().take())
    }
    pub fn cancel(&self) {
        self.progress.cancel.store(true, Ordering::Release);
    }
    /// 끝날 때까지 기다린다 (벤치마크·테스트)
    pub fn wait(&self) {
        while !self.progress.is_finished() {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

pub fn default_workers() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .saturating_sub(1)
        .max(1)
}

pub type StateFn = Arc<dyn Fn(usize) -> ItemState + Send + Sync>;

pub struct JobRequest {
    pub dataset: Arc<Dataset>,
    pub spec: ChartSpec,
    /// 이미 쿼리한 데이터가 있으면 다시 쓴다 (확대·크기 변경)
    pub data: Option<Arc<ChartData>>,
    pub states: StateFn,
    /// 그릴 item (None이면 전체)
    pub only: Option<Vec<usize>>,
    pub opts: RenderOptions,
    pub workers: usize,
}

fn make_slots(data: Arc<ChartData>, only: Option<Vec<usize>>) -> JobSlots {
    let indices: Vec<usize> = only.unwrap_or_else(|| (0..data.len()).collect());
    let results = (0..data.len()).map(|_| Mutex::new(None)).collect();
    JobSlots { data, results, indices }
}

/// job을 시작한다. 쿼리는 job 스레드에서 하고, 끝나면 렌더 스레드 N개가 item을 순서대로 가져간다.
pub fn start(req: JobRequest) -> Job {
    let workers = req.workers.max(1);
    let progress = Arc::new(JobProgress {
        total: AtomicUsize::new(0),
        done: AtomicUsize::new(0),
        running: AtomicUsize::new(0),
        failed: AtomicUsize::new(0),
        stage: AtomicU8::new(JobStage::Query as u8),
        cancel: AtomicBool::new(false),
        started: Instant::now(),
        query_ms: Mutex::new(None),
        finished: Mutex::new(None),
        error: Mutex::new(None),
        workers,
    });
    let slots: Arc<OnceLock<JobSlots>> = Arc::new(OnceLock::new());
    let JobRequest {
        dataset,
        spec,
        data,
        states,
        only,
        opts,
        ..
    } = req;
    let mut only = Some(only);
    if let Some(d) = data {
        let s = make_slots(d, only.take().unwrap());
        progress.total.store(s.indices.len(), Ordering::Release);
        let _ = slots.set(s);
    }
    let job = Job {
        progress: progress.clone(),
        slots: slots.clone(),
    };
    std::thread::Builder::new()
        .name("vis-job".into())
        .spawn(move || {
            let p = progress;
            if slots.get().is_none() {
                match query(&dataset, &spec) {
                    Ok(d) => {
                        *p.query_ms.lock() = Some(d.query_ms);
                        let s = make_slots(Arc::new(d), only.take().unwrap());
                        p.total.store(s.indices.len(), Ordering::Release);
                        let _ = slots.set(s);
                    }
                    Err(e) => {
                        *p.error.lock() = Some(e.0);
                        *p.finished.lock() = Some(p.started.elapsed());
                        p.set_stage(JobStage::Failed);
                        return;
                    }
                }
            }
            if p.cancel.load(Ordering::Acquire) {
                *p.finished.lock() = Some(p.started.elapsed());
                p.set_stage(JobStage::Cancelled);
                return;
            }
            p.set_stage(JobStage::Render);
            let s = slots.get().unwrap();
            let next = AtomicUsize::new(0);
            std::thread::scope(|scope| {
                for w in 0..workers {
                    let (s, next, p, states, opts) = (s, &next, &p, &states, &opts);
                    std::thread::Builder::new()
                        .name(format!("vis-render-{w}"))
                        .spawn_scoped(scope, move || {
                            loop {
                                if p.cancel.load(Ordering::Acquire) {
                                    break;
                                }
                                let k = next.fetch_add(1, Ordering::AcqRel);
                                if k >= s.indices.len() {
                                    break;
                                }
                                let i = s.indices[k];
                                p.running.fetch_add(1, Ordering::AcqRel);
                                let t0 = Instant::now();
                                let state = states(i);
                                let out =
                                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render_once(&s.data, i, &state, opts, w)));
                                p.running.fetch_sub(1, Ordering::AcqRel);
                                match out {
                                    Ok(mut item) => {
                                        item.ms = t0.elapsed().as_secs_f64() * 1000.0;
                                        *s.results[i].lock() = Some(item);
                                    }
                                    Err(_) => {
                                        p.failed.fetch_add(1, Ordering::AcqRel);
                                    }
                                }
                                p.done.fetch_add(1, Ordering::AcqRel);
                            }
                        })
                        .expect("spawn render thread");
                }
            });
            *p.finished.lock() = Some(p.started.elapsed());
            p.set_stage(if p.cancel.load(Ordering::Acquire) {
                JobStage::Cancelled
            } else {
                JobStage::Done
            });
        })
        .expect("spawn job thread");
    job
}

/// item 하나: 원래 크기 + (요청하면) 섬네일. encode_png면 원본처럼 PNG 인코딩까지 한다
pub fn render_once(data: &ChartData, i: usize, state: &ItemState, opts: &RenderOptions, worker: usize) -> RenderedItem {
    let r = render_item(data, i, state, opts, false, (opts.width, opts.height));
    let thumb = opts.thumbnail.map(|size| {
        let tr = render_item(data, i, state, opts, true, size);
        if opts.encode_png {
            let _ = tr.canvas.encode_png();
        }
        (tr.canvas.pixmap.width(), tr.canvas.pixmap.height(), tr.canvas.to_bgra())
    });
    let bgra = if opts.encode_png {
        let _ = r.canvas.encode_png();
        Vec::new()
    } else {
        r.canvas.to_bgra()
    };
    RenderedItem {
        index: i,
        width: r.canvas.pixmap.width(),
        height: r.canvas.pixmap.height(),
        css: (opts.width, opts.height),
        bgra,
        meta: r.meta,
        thumb,
        ms: 0.0,
        worker,
    }
}

/// 데이터 표·다운로드의 대상 행 (원본 server/data/item-rows.ts).
/// item의 행에서 숨긴 legend 값과 지운 영역의 점을 빼고, 선택 영역이 있고 그 차트가 선택으로 행을 거르면
/// 선택 안의 행만 남긴다. 두 번째 값은 선택으로 걸렀는지 (파일 이름 `_선택`).
pub fn item_rows(data: &ChartData, index: usize, state: &ItemState) -> (Vec<u32>, bool) {
    let ds = &data.dataset;
    let spec = &data.spec;
    let mut rows = data.groups.rows_of(index).to_vec();
    let legend = spec.legend_column().and_then(|l| ds.column(l));
    if let Some(col) = legend.filter(|_| !state.hidden_legend.is_empty()) {
        rows.retain(|&r| !state.hidden_legend.contains(&col.text_at(r as usize)));
    }
    let xy = |r: u32| -> Option<(f64, f64)> {
        let x = ds.column(&spec.x)?.nums()?[r as usize];
        let y = ds.column(&spec.y)?.nums()?[r as usize];
        Some((x, y))
    };
    if spec.can_delete() && !state.deletions.is_empty() {
        let tests: Vec<_> = state.deletions.iter().map(|r| r.tester()).collect();
        rows.retain(|&r| xy(r).is_none_or(|(x, y)| !tests.iter().any(|t| t(x, y))));
    }
    let Some(sel) = state.selection.as_ref().filter(|_| spec.filters_rows_by_selection()) else {
        return (rows, false);
    };
    let test = sel.tester();
    match (&data.payload, spec.chart_type) {
        (Payload::Points { .. }, _) => rows.retain(|&r| xy(r).is_some_and(|(x, y)| test(x, y))),
        (Payload::Category(items), ChartType::Waterfall) => {
            // 막대 끝 = 누적 값
            let item = &items[index];
            let mut cum = 0.0;
            let mut keep = std::collections::HashSet::new();
            for (i, b) in item.bars.iter().enumerate() {
                cum += b[0].unwrap_or(0.0);
                if test(i as f64, cum) {
                    keep.insert(item.categories[i].clone());
                }
            }
            if let Some(xc) = ds.column(&spec.x) {
                rows.retain(|&r| keep.contains(&xc.text_at(r as usize)));
            }
        }
        (Payload::Category(items), _) => {
            // 막대 끝 · 상자 중앙값이 영역 안인 (범주, legend 값)
            let item = &items[index];
            let lvals = data.legend_values.clone().unwrap_or_default();
            let mut keep = std::collections::HashSet::new();
            for (i, c) in item.categories.iter().enumerate() {
                let width = item
                    .bars
                    .get(i)
                    .map(|b| b.len())
                    .or(item.boxes.get(i).map(|b| b.len()))
                    .unwrap_or(1);
                for g in 0..width {
                    let v = match spec.chart_type {
                        ChartType::Boxplot => item.boxes[i][g].as_ref().map(|b| b.median),
                        _ => item.bars[i][g],
                    };
                    if v.is_some_and(|v| test(i as f64, v)) {
                        keep.insert((c.clone(), lvals.get(g).cloned()));
                    }
                }
            }
            if let Some(xc) = ds.column(&spec.x) {
                rows.retain(|&r| {
                    let l = legend.map(|col| col.text_at(r as usize));
                    keep.contains(&(xc.text_at(r as usize), l))
                });
            }
        }
        (Payload::Histogram(bins, items), _) => {
            let item = &items[index];
            let lvals = data.legend_values.clone().unwrap_or_default();
            let mut keep = std::collections::HashSet::new();
            for (b, cell) in item.counts.iter().enumerate() {
                for (g, &n) in cell.iter().enumerate() {
                    let cx = (bins.edge(b) + bins.edge(b + 1)) / 2.0;
                    if n > 0 && test(cx, n as f64) {
                        keep.insert((b, g));
                    }
                }
            }
            if let Some(xs) = ds.column(&spec.x).and_then(|c| c.nums()) {
                rows.retain(|&r| {
                    let v = xs[r as usize];
                    let b = (((v - bins.lo) / bins.step).floor().max(0.0) as usize).min(bins.count.saturating_sub(1));
                    let g = legend
                        .map(|col| col.text_at(r as usize))
                        .and_then(|t| lvals.iter().position(|l| *l == t))
                        .unwrap_or(0);
                    keep.contains(&(b, g))
                });
            }
        }
        _ => return (rows, false),
    }
    (rows, true)
}
