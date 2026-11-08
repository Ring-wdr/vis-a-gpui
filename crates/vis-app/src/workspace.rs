//! gallery 탭 하나 = 작업 공간. 왼쪽 설정 패널, 가운데 item grid(페이지), 오른쪽 필터, 아래 job 바.
//! 원본 src/views/workspace + widgets(item grid, 설정 패널, job 하단 바)를 한 화면으로 옮겼다.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, IconName, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use parking_lot::Mutex;

use vis_core::dataset::{ColumnKind, ColumnSchema, Dataset};
use vis_core::history::{History, HistoryEntry};
use vis_core::job::{self, ChartData, Job, JobRequest, JobStage, RenderOptions, RenderedItem};
use vis_core::render::meta::{AxisKind, ItemMeta};
use vis_core::render::theme::{PALETTE, parse_hex};
use vis_core::spec::{AGG_FNS, AggFn, CHART_TYPES, ChartSpec, ChartTheme, ChartType, CommonProps, ItemState, ItemView, LegendPosition};
use vis_core::store::Store;

use crate::filters::FilterPanel;
use crate::ui::{dropdown, field, fmt_int, fmt_ms, hex_color, render_image};

/// 화면에 올린 이미지가 이 바이트를 넘으면 보이지 않는 item부터 내린다 (원본은 PNG를 Cache API에 뒀다)
const IMAGE_BUDGET: usize = 768 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Default,
    Zoom,
    Pan,
}

pub struct ItemImage {
    pub image: Arc<RenderImage>,
    pub meta: Arc<ItemMeta>,
    /// 그린 CSS 크기
    pub css: (f32, f32),
    pub bytes: usize,
    /// 이 item을 그리는 데 걸린 시간
    #[allow(dead_code)]
    pub ms: f64,
}

pub struct Drawn {
    pub spec: ChartSpec,
    pub dataset: Arc<Dataset>,
    pub main: Job,
    /// 일부만 다시 그리는 job (확대·legend·크기 변경)
    pub extra: Vec<Job>,
    pub data: Option<Arc<ChartData>>,
    pub images: HashMap<usize, ItemImage>,
    /// 최근에 쓴 순서 (LRU)
    pub lru: VecDeque<usize>,
    pub image_bytes: usize,
    pub pending: HashSet<usize>,
    pub states: Arc<Mutex<Vec<ItemState>>>,
    pub page: usize,
    pub history_id: Option<String>,
    pub recorded: bool,
    pub big: Option<(usize, ItemImage)>,
    pub big_pending: Option<Job>,
    pub item_ms_total: f64,
    pub item_ms_count: usize,
}

impl Drawn {
    fn len(&self) -> usize {
        self.data.as_ref().map(|d| d.len()).unwrap_or(0)
    }
}

struct Hover {
    item: usize,
    pos: Point<Pixels>,
    lines: Vec<String>,
    color: Option<u32>,
    marker: Option<(f32, f32)>,
}

struct Drag {
    item: usize,
    start: Point<Pixels>,
    now: Point<Pixels>,
}

pub enum WorkspaceEvent {
    /// 탭 제목이 바뀜
    Title,
    /// history가 바뀜
    History,
}

impl EventEmitter<WorkspaceEvent> for Workspace {}

pub struct Workspace {
    pub store: Arc<Store>,
    pub history: Arc<History>,
    pub spec: ChartSpec,
    pub drawn: Option<Drawn>,
    pub read_only: bool,
    mode: Mode,
    filters: Entity<FilterPanel>,
    show_filters: bool,
    grid_bounds: Rc<Cell<Bounds<Pixels>>>,
    item_bounds: Rc<RefCell<HashMap<usize, Bounds<Pixels>>>>,
    big_bounds: Rc<Cell<Bounds<Pixels>>>,
    hover: Option<Hover>,
    drag: Option<Drag>,
    selected: Option<usize>,
    poll: Option<Task<()>>,
    resize: Option<Task<()>>,
    wheel: f32,
    last_cell: (f32, f32),
    /// 다음 draw에 실어 보낼 복원 상태 (history)
    restore: Option<HashMap<String, ItemState>>,
    /// 이어서 기록할 history
    pending_history: Option<String>,
    /// 크게 보는 item
    enlarged: Option<usize>,
    /// 그리드 크기를 알면 바로 draw (history에서 열 때)
    auto_draw: bool,
}

impl Workspace {
    pub fn new(store: Arc<Store>, history: Arc<History>, dataset: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let ds_id = dataset
            .or_else(|| store.ready().first().map(|d| d.id.clone()))
            .unwrap_or_else(|| "iot-100k".into());
        let spec = default_spec(&ds_id, store.get(&ds_id).as_deref());
        let filters = cx.new(|cx| FilterPanel::new(store.clone(), ds_id.clone(), window, cx));
        Self {
            store,
            history,
            spec,
            drawn: None,
            read_only: false,
            mode: Mode::Default,
            filters,
            show_filters: false,
            grid_bounds: Rc::new(Cell::new(Bounds::default())),
            item_bounds: Rc::new(RefCell::new(HashMap::new())),
            big_bounds: Rc::new(Cell::new(Bounds::default())),
            hover: None,
            drag: None,
            selected: None,
            poll: None,
            resize: None,
            wheel: 0.0,
            last_cell: (0.0, 0.0),
            restore: None,
            pending_history: None,
            enlarged: None,
            auto_draw: false,
        }
    }

    /// history 기록으로 연다 (설정 + item 상태 복원 후 바로 draw)
    pub fn from_history(
        store: Arc<Store>,
        history: Arc<History>,
        entry: &HistoryEntry,
        read_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut ws = Workspace::new(store, history, Some(entry.spec.dataset.clone()), window, cx);
        ws.spec = entry.spec.clone();
        ws.read_only = read_only;
        ws.restore = Some(entry.items.clone());
        // 다시 보기 전용 탭은 같은 기록에 덧붙이지 않는다
        ws.pending_history = if read_only { None } else { Some(entry.id.clone()) };
        ws.auto_draw = true;
        let filters = entry.spec.filters.clone();
        ws.filters.update(cx, |f, cx| f.set_filters(filters, cx));
        ws
    }

    pub fn title(&self) -> String {
        match &self.drawn {
            Some(d) => d.spec.summary(),
            None => {
                let name = self
                    .store
                    .list()
                    .into_iter()
                    .find(|(i, _)| i.id == self.spec.dataset)
                    .map(|(i, _)| i.name)
                    .unwrap_or_default();
                format!("새 차트 · {name}")
            }
        }
    }

    fn dataset(&self) -> Option<Arc<Dataset>> {
        self.store.get(&self.spec.dataset)
    }

    fn schema(&self) -> Vec<ColumnSchema> {
        self.dataset().map(|d| d.schema()).unwrap_or_default()
    }

    pub fn is_running(&self) -> bool {
        self.drawn.as_ref().is_some_and(|d| !d.main.progress.is_finished())
    }

    // ── draw ─────────────────────────────────────────────────────────

    fn cell_size(&self, window: &Window) -> (f32, f32) {
        let b = self.grid_bounds.get();
        let (rows, cols) = self.drawn.as_ref().map(|d| d.spec.grid).unwrap_or(self.spec.grid);
        let gap = 8.0;
        let w = f32::from(b.size.width).max(200.0);
        let h = f32::from(b.size.height).max(160.0);
        let _ = window;
        (
            ((w - gap * (cols as f32 - 1.0)) / cols as f32).floor().max(120.0),
            ((h - gap * (rows as f32 - 1.0)) / rows as f32).floor().max(90.0),
        )
    }

    fn render_opts(&self, size: (f32, f32), window: &Window, cx: &App) -> RenderOptions {
        RenderOptions {
            width: size.0,
            height: size.1,
            scale: window.scale_factor().clamp(1.0, 3.0),
            theme: if cx.theme().mode.is_dark() {
                ChartTheme::Dark
            } else {
                ChartTheme::Light
            },
            thumbnail: None,
            encode_png: false,
        }
    }

    pub fn draw(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_running() {
            return;
        }
        let Some(ds) = self.dataset() else {
            window.push_notification("데이터를 먼저 등록하세요 (datastore).", cx);
            return;
        };
        let mut spec = self.spec.clone();
        spec.filters = self.filters.read(cx).filters();
        let size = self.cell_size(window);
        self.last_cell = size;
        let states: Arc<Mutex<Vec<ItemState>>> = Arc::new(Mutex::new(Vec::new()));
        let st = states.clone();
        let opts = self.render_opts(size, window, cx);
        let restore = self.restore.take();
        let was_ro = self.read_only;
        let _ = was_ro;
        let job = job::start(JobRequest {
            dataset: ds.clone(),
            spec: spec.clone(),
            data: None,
            states: Arc::new(move |i| st.lock().get(i).cloned().unwrap_or_default()),
            only: None,
            opts,
            workers: job::default_workers(),
        });
        // 쿼리가 끝나면 item 상태 칸을 채운다 (history 복원)
        self.drawn = Some(Drawn {
            spec,
            dataset: ds,
            main: job,
            extra: vec![],
            data: None,
            images: HashMap::new(),
            lru: VecDeque::new(),
            image_bytes: 0,
            pending: HashSet::new(),
            states,
            page: 0,
            history_id: self.pending_history.take(),
            recorded: false,
            big: None,
            big_pending: None,
            item_ms_total: 0.0,
            item_ms_count: 0,
        });
        self.restore = restore;
        self.selected = None;
        self.hover = None;
        self.start_poll(window, cx);
        cx.emit(WorkspaceEvent::Title);
        cx.notify();
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(d) = &self.drawn {
            d.main.cancel();
            for j in &d.extra {
                j.cancel();
            }
        }
        cx.notify();
    }

    fn start_poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.poll.is_some() {
            return;
        }
        self.poll = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(33)).await;
                let keep = this.update_in(cx, |this, window, cx| this.collect(window, cx)).unwrap_or(false);
                if !keep {
                    break;
                }
            }
            let _ = this.update(cx, |this, _| this.poll = None);
        }));
    }

    /// 끝난 item을 가져와 이미지로 올린다. 할 일이 남았으면 true
    fn collect(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let visible: HashSet<usize> = self.visible_items().into_iter().collect();
        let Some(d) = self.drawn.as_mut() else { return false };
        let mut changed = false;
        if d.data.is_none() {
            if let Some(data) = d.main.data() {
                d.data = Some(data.clone());
                let n = data.len();
                let mut st = d.states.lock();
                *st = vec![ItemState::default(); n];
                if let Some(restore) = self.restore.take() {
                    for i in 0..n {
                        if let Some(s) = restore.get(&data.item_name(i)) {
                            st[i] = s.clone();
                        }
                    }
                }
                changed = true;
            }
        }
        let n = d.len();
        let mut taken: Vec<RenderedItem> = vec![];
        for job in std::iter::once(&d.main).chain(d.extra.iter()) {
            if let Some(slots) = job.slots.get() {
                for &i in &slots.indices {
                    if let Some(item) = job.take(i) {
                        taken.push(item);
                    }
                }
            }
        }
        for item in taken {
            let i = item.index;
            d.pending.remove(&i);
            d.item_ms_total += item.ms;
            d.item_ms_count += 1;
            // 보이지 않고 예산을 넘었으면 올리지 않는다 (보일 때 다시 그린다)
            let bytes = item.bgra.len();
            if !visible.contains(&i) && d.image_bytes + bytes > IMAGE_BUDGET {
                continue;
            }
            let img = ItemImage {
                image: render_image(item.width, item.height, item.bgra),
                meta: Arc::new(item.meta),
                css: item.css,
                bytes,
                ms: item.ms,
            };
            if let Some(old) = d.images.insert(i, img) {
                d.image_bytes -= old.bytes;
                cx.drop_image(old.image, Some(window));
            }
            d.image_bytes += bytes;
            d.lru.retain(|&k| k != i);
            d.lru.push_back(i);
            changed = true;
        }
        // 예산을 넘으면 보이지 않는 오래된 이미지부터 내린다
        while d.image_bytes > IMAGE_BUDGET {
            let Some(pos) = d.lru.iter().position(|k| !visible.contains(k)) else {
                break;
            };
            let k = d.lru.remove(pos).unwrap();
            if let Some(old) = d.images.remove(&k) {
                d.image_bytes -= old.bytes;
                cx.drop_image(old.image, Some(window));
            }
        }
        // 크게 보기
        if let Some(job) = &d.big_pending {
            if let Some(slots) = job.slots.get() {
                if let Some(&i) = slots.indices.first() {
                    if let Some(item) = job.take(i) {
                        let img = ItemImage {
                            image: render_image(item.width, item.height, item.bgra),
                            meta: Arc::new(item.meta),
                            css: item.css,
                            bytes: 0,
                            ms: item.ms,
                        };
                        if let Some((_, old)) = d.big.replace((i, img)) {
                            cx.drop_image(old.image, Some(window));
                        }
                        d.big_pending = None;
                        changed = true;
                    }
                }
            }
        }
        d.extra.retain(|j| {
            !j.progress.is_finished()
                || j.slots
                    .get()
                    .is_some_and(|s| s.indices.iter().any(|&i| s.results[i].lock().is_some()))
        });
        let main_done = d.main.progress.is_finished();
        if main_done && !d.recorded {
            d.recorded = true;
            self.record_history(cx);
            changed = true;
        }
        let d = self.drawn.as_ref().unwrap();
        let busy = !d.main.progress.is_finished() || !d.extra.is_empty() || d.big_pending.is_some();
        let _ = n;
        // 진행 표시를 위해 돌 때마다 다시 그린다
        cx.notify();
        let _ = changed;
        busy
    }

    fn record_history(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.drawn.as_mut() else { return };
        let Some(data) = d.data.clone() else { return };
        let p = &d.main.progress;
        let status = match p.stage() {
            JobStage::Done => "success",
            JobStage::Cancelled => "cancelled",
            _ => "failed",
        };
        if self.read_only {
            return;
        }
        if let Some(id) = &d.history_id {
            let states = d.states.lock().clone();
            self.history.update(id, |e| {
                e.items = item_map(&data, &states);
                e.draw_ms = p.elapsed().as_secs_f64() * 1000.0;
            });
        } else {
            let id = vis_core::history::new_id();
            // 섬네일: 첫 item을 그림 영역만 작게
            let thumb = (!data.is_empty()).then(|| {
                let opts = RenderOptions {
                    width: 256.0,
                    height: 160.0,
                    scale: 1.0,
                    theme: ChartTheme::Light,
                    thumbnail: None,
                    encode_png: false,
                };
                job::render_item(&data, 0, &ItemState::default(), &opts, true, (256.0, 160.0))
                    .canvas
                    .encode_png()
            });
            self.history.add(
                HistoryEntry {
                    id: id.clone(),
                    created_ms: chrono_now(),
                    spec: d.spec.clone(),
                    items: HashMap::new(),
                    item_count: data.len(),
                    draw_ms: p.elapsed().as_secs_f64() * 1000.0,
                    status: status.into(),
                },
                thumb,
            );
            d.history_id = Some(id);
        }
        cx.emit(WorkspaceEvent::History);
    }

    /// 지금 페이지 item 번호
    fn visible_items(&self) -> Vec<usize> {
        let Some(d) = &self.drawn else { return vec![] };
        let per = (d.spec.grid.0 * d.spec.grid.1) as usize;
        let start = d.page * per;
        (start..(start + per).min(d.len())).collect()
    }

    fn page_count(&self) -> usize {
        let Some(d) = &self.drawn else { return 0 };
        let per = (d.spec.grid.0 * d.spec.grid.1) as usize;
        d.len().div_ceil(per).max(1)
    }

    /// item 몇 개만 다시 그린다 (확대·legend·크기 변경). 같은 쿼리 결과를 다시 쓴다
    fn redraw(&mut self, items: Vec<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let size = self.last_cell;
        let opts = self.render_opts(size, window, cx);
        let Some(d) = self.drawn.as_mut() else { return };
        let Some(data) = d.data.clone() else { return };
        let items: Vec<usize> = items.into_iter().filter(|i| *i < data.len() && !d.pending.contains(i)).collect();
        if items.is_empty() {
            return;
        }
        for &i in &items {
            d.pending.insert(i);
        }
        let st = d.states.clone();
        let job = job::start(JobRequest {
            dataset: d.dataset.clone(),
            spec: d.spec.clone(),
            data: Some(data),
            states: Arc::new(move |i| st.lock().get(i).cloned().unwrap_or_default()),
            only: Some(items),
            opts,
            workers: job::default_workers(),
        });
        d.extra.push(job);
        self.start_poll(window, cx);
    }

    fn redraw_big(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let b = self.big_bounds.get();
        let size = (f32::from(b.size.width).max(300.0), f32::from(b.size.height).max(200.0));
        let opts = self.render_opts(size, window, cx);
        let Some(i) = self.big_item() else { return };
        let Some(d) = self.drawn.as_mut() else { return };
        let Some(data) = d.data.clone() else { return };
        let st = d.states.clone();
        d.big_pending = Some(job::start(JobRequest {
            dataset: d.dataset.clone(),
            spec: d.spec.clone(),
            data: Some(data),
            states: Arc::new(move |i| st.lock().get(i).cloned().unwrap_or_default()),
            only: Some(vec![i]),
            opts,
            workers: 1,
        }));
        self.start_poll(window, cx);
    }

    fn big_item(&self) -> Option<usize> {
        self.enlarged
    }

    /// 보이는 item 중 지금 크기와 다르게 그려졌거나 이미지가 없는 것을 다시 그린다
    fn refresh_visible(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let size = self.last_cell;
        let Some(d) = &self.drawn else { return };
        if !d.main.progress.is_finished() && d.main.progress.stage() != JobStage::Render {
            return;
        }
        let main_done = d.main.progress.is_finished();
        let want: Vec<usize> = self
            .visible_items()
            .into_iter()
            .filter(|i| match d.images.get(i) {
                Some(img) => (img.css.0 - size.0).abs() > 0.5 || (img.css.1 - size.1).abs() > 0.5,
                // 첫 draw가 아직 그리지 않은 item은 그 job이 그린다
                None => main_done,
            })
            .collect();
        if !want.is_empty() {
            self.redraw(want, window, cx);
        }
    }

    fn update_state(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>, f: impl FnOnce(&mut ItemState)) {
        if self.read_only {
            return;
        }
        let Some(d) = &self.drawn else { return };
        {
            let mut st = d.states.lock();
            if let Some(s) = st.get_mut(i) {
                f(s);
            }
        }
        // history에 덧붙인다
        if let (Some(id), Some(data)) = (&d.history_id, &d.data) {
            let states = d.states.lock().clone();
            let map = item_map(data, &states);
            self.history.update(id, |e| e.items = map);
        }
        self.redraw(vec![i], window, cx);
        if self.enlarged == Some(i) {
            self.redraw_big(window, cx);
        }
    }

    fn set_page(&mut self, page: usize, window: &mut Window, cx: &mut Context<Self>) {
        let pages = self.page_count();
        if let Some(d) = self.drawn.as_mut() {
            d.page = page.min(pages.saturating_sub(1));
        }
        self.hover = None;
        self.refresh_visible(window, cx);
        cx.notify();
    }

    // ── 마우스 ────────────────────────────────────────────────────────

    /// 창 좌표 → item 이미지(CSS) 좌표
    fn local(&self, item: usize, pos: Point<Pixels>, big: bool) -> Option<(f32, f32, Arc<ItemMeta>)> {
        let d = self.drawn.as_ref()?;
        let (img, b) = if big {
            let (i, img) = d.big.as_ref()?;
            if *i != item {
                return None;
            }
            (img, self.big_bounds.get())
        } else {
            (d.images.get(&item)?, *self.item_bounds.borrow().get(&item)?)
        };
        let lx = f32::from(pos.x - b.origin.x) * img.css.0 / f32::from(b.size.width).max(1.0);
        let ly = f32::from(pos.y - b.origin.y) * img.css.1 / f32::from(b.size.height).max(1.0);
        Some((lx, ly, img.meta.clone()))
    }

    fn on_move(&mut self, item: usize, big: bool, e: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(drag) = &mut self.drag {
            if drag.item == item {
                drag.now = e.position;
                // 버튼을 칸 밖에서 놓쳤으면 끝낸다
                if e.pressed_button != Some(MouseButton::Left) {
                    self.drag = None;
                }
                cx.notify();
                return;
            }
        }
        let Some((x, y, meta)) = self.local(item, e.position, big) else {
            return;
        };
        let mut hover = None;
        if meta.legend_at(x, y).is_none() && meta.plot.contains(x, y) {
            if let Some(points) = &meta.points {
                let xa = meta.x.unwrap();
                let ya = meta.y.unwrap();
                let fmt_x = |v: f64| match xa.kind {
                    AxisKind::Time => vis_core::timefmt::format_kst(v, "%Y-%m-%d %H:%M:%S"),
                    _ => short(v),
                };
                let mut lines = vec![format!("x {}  ·  y {}", fmt_x(xa.invert(x)), short(ya.invert(y)))];
                let mut color = None;
                let mut marker = None;
                if let Some(k) = points.nearest(x, y, 24.0) {
                    lines.push(format!("가까운 점: x {} · y {}", fmt_x(points.x[k]), short(points.y[k])));
                    if let Some((name, c)) = meta.legend_values.get(points.g[k] as usize) {
                        lines.push(name.clone());
                        color = Some(*c);
                    }
                    // 툴팁 추가 컬럼: 행 번호로 데이터셋에서 바로 읽는다 (원본은 서버 호출)
                    if let Some(d) = &self.drawn {
                        for col in &d.spec.tooltip {
                            if let Some(c) = d.dataset.column(col) {
                                lines.push(format!("{}: {}", c.label, c.text_at(points.id[k] as usize)));
                            }
                        }
                    }
                    marker = Some((points.px[k], points.py[k]));
                }
                hover = Some(Hover {
                    item,
                    pos: e.position,
                    lines,
                    color,
                    marker,
                });
            } else if let Some(h) = meta.hit(x, y) {
                hover = Some(Hover {
                    item,
                    pos: e.position,
                    lines: h.lines.clone(),
                    color: h.color,
                    marker: None,
                });
            }
        } else if let Some(h) = meta.hit(x, y) {
            hover = Some(Hover {
                item,
                pos: e.position,
                lines: h.lines.clone(),
                color: h.color,
                marker: None,
            });
        }
        self.hover = hover;
        cx.notify();
    }

    fn on_down(&mut self, item: usize, big: bool, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(item);
        if e.click_count >= 2 && !big {
            self.enlarge(Some(item), window, cx);
            return;
        }
        let Some((x, y, meta)) = self.local(item, e.position, big) else {
            return;
        };
        // legend 항목을 누르면 그 값을 숨기거나 보인다
        if let Some(l) = meta.legend_at(x, y) {
            let value = l.value.clone();
            self.update_state(item, window, cx, |s| {
                if let Some(p) = s.hidden_legend.iter().position(|v| *v == value) {
                    s.hidden_legend.remove(p);
                } else {
                    s.hidden_legend.push(value);
                }
            });
            return;
        }
        if self.mode != Mode::Default && meta.plot.contains(x, y) && meta.x.is_some() && self.zoomable() {
            self.drag = Some(Drag {
                item,
                start: e.position,
                now: e.position,
            });
            self.hover = None;
        }
        cx.notify();
    }

    fn on_up(&mut self, big: bool, e: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        let Some((x0, y0, meta)) = self.local(drag.item, drag.start, big) else {
            return;
        };
        let Some((x1, y1, _)) = self.local(drag.item, e.position, big) else {
            return;
        };
        let (Some(xa), Some(ya)) = (meta.x, meta.y) else { return };
        let view = match self.mode {
            Mode::Zoom => {
                if (x1 - x0).abs() < 4.0 || (y1 - y0).abs() < 4.0 {
                    return;
                }
                let (a, b) = (xa.invert(x0.min(x1)), xa.invert(x0.max(x1)));
                let (c, d) = (ya.invert(y0.max(y1)), ya.invert(y0.min(y1)));
                ItemView {
                    x: (a.min(b), a.max(b)),
                    y: (c.min(d), c.max(d)),
                }
            }
            Mode::Pan => {
                let dx = xa.invert(x0) - xa.invert(x1);
                let dy = ya.invert(y0) - ya.invert(y1);
                ItemView {
                    x: (xa.domain.0 + dx, xa.domain.1 + dx),
                    y: (ya.domain.0 + dy, ya.domain.1 + dy),
                }
            }
            Mode::Default => return,
        };
        self.update_state(drag.item, window, cx, |s| s.view = Some(view));
    }

    fn zoomable(&self) -> bool {
        self.drawn.as_ref().is_some_and(|d| d.spec.chart_type.fields().zoom)
    }

    fn zoom_out(&mut self, item: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(img) = self.drawn.as_ref().and_then(|d| d.images.get(&item)) else {
            return;
        };
        let (Some(xa), Some(ya)) = (img.meta.x, img.meta.y) else { return };
        let grow = |d: (f64, f64)| {
            let c = (d.0 + d.1) / 2.0;
            let h = d.1 - d.0;
            (c - h, c + h)
        };
        let view = ItemView {
            x: grow(xa.domain),
            y: grow(ya.domain),
        };
        self.update_state(item, window, cx, |s| s.view = Some(view));
    }

    fn enlarge(&mut self, item: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.enlarged = item;
        self.hover = None;
        if let Some(d) = self.drawn.as_mut() {
            if let Some((_, old)) = d.big.take() {
                cx.drop_image(old.image, Some(window));
            }
        }
        if let Some(i) = item {
            // 그 item이 있는 페이지로
            if let Some(d) = self.drawn.as_mut() {
                let per = (d.spec.grid.0 * d.spec.grid.1) as usize;
                d.page = i / per;
            }
            cx.on_next_frame(window, |this, window, cx| this.redraw_big(window, cx));
        } else {
            self.refresh_visible(window, cx);
        }
        cx.notify();
    }

    fn on_grid_resized(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.auto_draw {
            self.auto_draw = false;
            let ro = self.read_only;
            self.read_only = false;
            self.draw(window, cx);
            self.read_only = ro;
            return;
        }
        let size = self.cell_size(window);
        if (size.0 - self.last_cell.0).abs() < 1.0 && (size.1 - self.last_cell.1).abs() < 1.0 {
            return;
        }
        // 크기 변경이 멈추고 300ms 뒤 한 번만 다시 그린다 (그동안 이전 이미지를 늘려 보여준다)
        self.resize = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(300)).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.last_cell = this.cell_size(window);
                this.refresh_visible(window, cx);
                if this.enlarged.is_some() {
                    this.redraw_big(window, cx);
                }
            });
        }));
    }
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn short(v: f64) -> String {
    vis_core::render::scale::short_number(v)
}

fn item_map(data: &ChartData, states: &[ItemState]) -> HashMap<String, ItemState> {
    states
        .iter()
        .enumerate()
        .filter(|(_, s)| **s != ItemState::default())
        .map(|(i, s)| (data.item_name(i), s.clone()))
        .collect()
}

/// 데이터셋 스키마에 맞는 기본 설정
pub fn default_spec(dataset: &str, ds: Option<&Dataset>) -> ChartSpec {
    let mut spec = ChartSpec {
        dataset: dataset.into(),
        chart_type: ChartType::Scatter,
        x: "ts".into(),
        y: "value".into(),
        z: None,
        agg: AggFn::Avg,
        legend: None,
        split_by: vec![],
        split_separator: "-".into(),
        tooltip: vec![],
        grid: (3, 3),
        common: CommonProps::default(),
        filters: vec![],
    };
    if let Some(ds) = ds {
        fix_spec(&mut spec, &ds.schema());
    }
    spec
}

/// 스키마·차트 종류에 맞지 않는 설정을 타입에 맞는 기본값으로 바로잡는다
pub fn fix_spec(spec: &mut ChartSpec, schema: &[ColumnSchema]) {
    let f = spec.chart_type.fields();
    let pick = |cur: &str, kinds: &[ColumnKind], avoid: Option<&str>| -> String {
        if schema
            .iter()
            .any(|c| c.name == cur && kinds.contains(&c.kind) && Some(cur) != avoid)
        {
            return cur.to_string();
        }
        schema
            .iter()
            .find(|c| kinds.contains(&c.kind) && Some(c.name.as_str()) != avoid)
            .or_else(|| schema.iter().find(|c| kinds.contains(&c.kind)))
            .map(|c| c.name.clone())
            .unwrap_or_default()
    };
    spec.x = pick(&spec.x, f.x, None);
    if let Some(y) = f.y {
        spec.y = pick(&spec.y, y, Some(&spec.x));
    }
    spec.z = f.z.map(|z| pick(spec.z.as_deref().unwrap_or("value"), z, None));
    if !f.legend {
        spec.legend = None;
    } else if let Some(l) = &spec.legend {
        if !schema
            .iter()
            .any(|c| &c.name == l && c.kind == ColumnKind::Categorical && c.distinct.unwrap_or(99) <= 8)
        {
            spec.legend = None;
        }
    }
    spec.split_by.retain(|s| schema.iter().any(|c| &c.name == s));
    spec.tooltip.retain(|s| schema.iter().any(|c| &c.name == s));
}

// ── 화면 ─────────────────────────────────────────────────────────────

impl Workspace {
    fn render_left(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let schema = self.schema();
        let spec = self.spec.clone();
        let fields = spec.chart_type.fields();
        let entity = cx.entity().downgrade();
        let running = self.is_running();
        let ro = self.read_only;
        let ready: Vec<(SharedString, String)> = self.store.ready().into_iter().map(|d| (SharedString::from(d.name), d.id)).collect();
        let ds_name = ready
            .iter()
            .find(|(_, id)| *id == spec.dataset)
            .map(|(n, _)| n.clone())
            .unwrap_or_else(|| "데이터 없음".into());

        let set = {
            let entity = entity.clone();
            move |f: Box<dyn FnOnce(&mut ChartSpec, &[ColumnSchema])>, cx: &mut App| {
                let _ = entity.update(cx, |this, cx| {
                    let schema = this.schema();
                    f(&mut this.spec, &schema);
                    fix_spec(&mut this.spec, &schema);
                    cx.emit(WorkspaceEvent::Title);
                    cx.notify();
                });
            }
        };
        let set = Rc::new(set);
        let col_options = |kinds: &[ColumnKind]| -> Vec<(SharedString, String)> {
            schema
                .iter()
                .filter(|c| kinds.contains(&c.kind))
                .map(|c| (SharedString::from(format!("{} ({})", c.label, c.name)), c.name.clone()))
                .collect()
        };
        let label_of = |name: &str| {
            schema
                .iter()
                .find(|c| c.name == name)
                .map(|c| c.label.clone())
                .unwrap_or_else(|| name.to_string())
        };

        let mut col = v_flex().gap_3().p_3();
        // 데이터
        {
            let set = set.clone();
            let ent = entity.clone();
            col = col.child(field(
                "데이터",
                dropdown(
                    "dataset",
                    ds_name,
                    ready,
                    Some(spec.dataset.clone()),
                    move |id: String, window, cx| {
                        let id2 = id.clone();
                        set(Box::new(move |s, _| s.dataset = id2), cx);
                        let _ = ent.update(cx, |this, cx| {
                            let schema = this.schema();
                            fix_spec(&mut this.spec, &schema);
                            this.filters.update(cx, |f, cx| f.set_dataset(id.clone(), window, cx));
                        });
                    },
                ),
                cx,
            ));
        }
        // 차트 종류 (아이콘 대신 짧은 이름 버튼)
        {
            let mut grid = div().flex().flex_wrap().gap_1();
            for t in CHART_TYPES {
                let set = set.clone();
                grid = grid.child(
                    Button::new(SharedString::from(format!("ct-{}", t.name())))
                        .label(t.label())
                        .xsmall()
                        .when(t == spec.chart_type, |b| b.primary())
                        .when(t != spec.chart_type, |b| b.ghost())
                        .on_click(move |_, _, cx| set(Box::new(move |s, _| s.chart_type = t), cx)),
                );
            }
            col = col.child(field("차트 종류", grid, cx));
        }
        // grid
        {
            let options: Vec<(SharedString, u32)> = (1..=10).map(|n| (SharedString::from(n.to_string()), n)).collect();
            let (s1, s2) = (set.clone(), set.clone());
            let ent = entity.clone();
            let ent2 = entity.clone();
            col = col.child(field(
                "grid (행 × 열)",
                h_flex()
                    .gap_1()
                    .child(dropdown(
                        "grid-r",
                        spec.grid.0.to_string(),
                        options.clone(),
                        Some(spec.grid.0),
                        move |n, window, cx| {
                            s1(Box::new(move |s, _| s.grid.0 = n), cx);
                            let _ = ent.update(cx, |this, cx| this.apply_grid(window, cx));
                        },
                    ))
                    .child("×")
                    .child(dropdown(
                        "grid-c",
                        spec.grid.1.to_string(),
                        options,
                        Some(spec.grid.1),
                        move |n, window, cx| {
                            s2(Box::new(move |s, _| s.grid.1 = n), cx);
                            let _ = ent2.update(cx, |this, cx| this.apply_grid(window, cx));
                        },
                    )),
                cx,
            ));
        }
        // x / y / z
        {
            let set_x = set.clone();
            col = col.child(field(
                "x",
                dropdown(
                    "x",
                    label_of(&spec.x),
                    col_options(fields.x),
                    Some(spec.x.clone()),
                    move |v, _, cx| set_x(Box::new(move |s, _| s.x = v), cx),
                ),
                cx,
            ));
            if let Some(kinds) = fields.y {
                let set_y = set.clone();
                col = col.child(field(
                    "y",
                    dropdown("y", label_of(&spec.y), col_options(kinds), Some(spec.y.clone()), move |v, _, cx| {
                        set_y(Box::new(move |s, _| s.y = v), cx)
                    }),
                    cx,
                ));
            }
            if let Some(kinds) = fields.z {
                let set_z = set.clone();
                let z = spec.z.clone().unwrap_or_default();
                col = col.child(field(
                    "z (값)",
                    dropdown("z", label_of(&z), col_options(kinds), Some(z), move |v, _, cx| {
                        set_z(Box::new(move |s, _| s.z = Some(v)), cx)
                    }),
                    cx,
                ));
            }
        }
        if fields.legend {
            let mut opts: Vec<(SharedString, Option<String>)> = vec![("없음".into(), None)];
            opts.extend(
                schema
                    .iter()
                    .filter(|c| c.kind == ColumnKind::Categorical && c.distinct.unwrap_or(99) <= 8)
                    .map(|c| (SharedString::from(format!("{} ({})", c.label, c.name)), Some(c.name.clone()))),
            );
            let cur = spec.legend.as_deref().map(label_of).unwrap_or_else(|| "없음".into());
            let set_l = set.clone();
            col = col.child(field(
                "legend (고유값 8개 이하)",
                dropdown("legend", cur, opts, Some(spec.legend.clone()), move |v, _, cx| {
                    set_l(Box::new(move |s, _| s.legend = v), cx)
                }),
                cx,
            ));
        }
        if fields.agg {
            let opts: Vec<(SharedString, AggFn)> = AGG_FNS.iter().map(|a| (SharedString::from(a.label()), *a)).collect();
            let set_a = set.clone();
            col = col.child(field(
                "통계 함수",
                dropdown("agg", spec.agg.label(), opts, Some(spec.agg), move |v, _, cx| {
                    set_a(Box::new(move |s, _| s.agg = v), cx)
                }),
                cx,
            ));
        }
        // split by · tooltip: 누른 순서대로 쌓이는 다중 선택
        for (kind, label) in [("split", "split by (누른 순서)"), ("tooltip", "tooltip 컬럼")] {
            let chosen = if kind == "split" {
                spec.split_by.clone()
            } else {
                spec.tooltip.clone()
            };
            let mut chips = div().flex().flex_wrap().gap_1();
            for c in &schema {
                let pos = chosen.iter().position(|s| *s == c.name);
                let name = c.name.clone();
                let set = set.clone();
                let text = match pos {
                    Some(p) if kind == "split" => format!("{} {}", p + 1, c.label),
                    _ => c.label.clone(),
                };
                chips = chips.child(
                    Button::new(SharedString::from(format!("{kind}-{}", c.name)))
                        .label(text)
                        .xsmall()
                        .when(pos.is_some(), |b| b.primary())
                        .when(pos.is_none(), |b| b.outline())
                        .on_click(move |_, _, cx| {
                            let name = name.clone();
                            set(
                                Box::new(move |s, _| {
                                    let list = if kind == "split" { &mut s.split_by } else { &mut s.tooltip };
                                    if let Some(p) = list.iter().position(|x| *x == name) {
                                        list.remove(p);
                                    } else {
                                        list.push(name);
                                    }
                                }),
                                cx,
                            )
                        }),
                );
            }
            col = col.child(field(label, chips, cx));
        }
        // 공통 속성 (전체 설정): 색 · legend 위치 · 투명도
        {
            let mut swatches = h_flex().gap_1();
            for (i, hex) in PALETTE.iter().enumerate() {
                let set = set.clone();
                let c = parse_hex(hex).unwrap();
                let on = spec.common.color.eq_ignore_ascii_case(hex);
                swatches = swatches.child(
                    div()
                        .id(SharedString::from(format!("sw-{i}")))
                        .size(px(18.))
                        .rounded_full()
                        .bg(hex_color(c))
                        .border_2()
                        .border_color(if on { cx.theme().foreground } else { cx.theme().background })
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            let h = hex.to_string();
                            set(Box::new(move |s, _| s.common.color = h), cx)
                        }),
                );
            }
            let pos_opts: Vec<(SharedString, LegendPosition)> = vec![
                ("위".into(), LegendPosition::Top),
                ("아래".into(), LegendPosition::Bottom),
                ("왼쪽".into(), LegendPosition::Left),
                ("오른쪽".into(), LegendPosition::Right),
            ];
            let cur_pos = pos_opts
                .iter()
                .find(|p| p.1 == spec.common.legend_position)
                .map(|p| p.0.clone())
                .unwrap_or_default();
            let set_p = set.clone();
            let tr_opts: Vec<(SharedString, u32)> = [0u32, 20, 40, 60]
                .iter()
                .map(|t| (SharedString::from(format!("{t}%")), *t))
                .collect();
            let set_t = set.clone();
            col = col.child(field(
                "공통 속성 (색 · legend 위치 · 투명도)",
                v_flex().gap_1().child(swatches).child(
                    h_flex()
                        .gap_1()
                        .child(dropdown(
                            "lpos",
                            cur_pos,
                            pos_opts,
                            Some(spec.common.legend_position),
                            move |v, _, cx| set_p(Box::new(move |s, _| s.common.legend_position = v), cx),
                        ))
                        .child(dropdown(
                            "transp",
                            format!("투명도 {:.0}%", spec.common.transparency * 100.0),
                            tr_opts,
                            Some((spec.common.transparency * 100.0).round() as u32),
                            move |v, _, cx| set_t(Box::new(move |s, _| s.common.transparency = v as f32 / 100.0), cx),
                        )),
                ),
                cx,
            ));
        }

        let ent_draw = entity.clone();
        let footer = h_flex()
            .gap_2()
            .p_3()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(if running {
                Button::new("cancel")
                    .label("cancel")
                    .danger()
                    .w_full()
                    .on_click(move |_, _, cx| {
                        let _ = ent_draw.update(cx, |this, cx| this.cancel(cx));
                    })
                    .into_any_element()
            } else {
                Button::new("draw")
                    .label(if ro { "보기 전용 (history)" } else { "draw" })
                    .primary()
                    .w_full()
                    .disabled(ro)
                    .on_click(move |_, window, cx| {
                        let _ = ent_draw.update(cx, |this, cx| this.draw(window, cx));
                    })
                    .into_any_element()
            });
        v_flex()
            .w(px(272.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .child(div().id("left-scroll").flex_1().overflow_y_scroll().child(col))
            .child(footer)
            .when(ro, |d| d.opacity(0.85))
            .into_any_element()
    }

    fn apply_grid(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // grid 크기는 바로 반영한다 (item 픽셀 크기가 바뀌므로 보이는 item을 다시 그린다)
        if self.is_running() {
            window.push_notification("렌더링 중에는 grid 크기를 바꿀 수 없습니다. cancel 후 바꾸세요.", cx);
            return;
        }
        if let Some(d) = self.drawn.as_mut() {
            d.spec.grid = self.spec.grid;
            d.page = 0;
        }
        cx.on_next_frame(window, |this, window, cx| {
            this.last_cell = this.cell_size(window);
            this.refresh_visible(window, cx);
        });
        cx.notify();
    }

    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let drawn = self.drawn.is_some();
        let zoomable = self.zoomable();
        let entity = cx.entity().downgrade();
        let mode_btn = |id: &'static str, label: &'static str, m: Mode, cur: Mode, enabled: bool| {
            let entity = entity.clone();
            Button::new(id)
                .label(label)
                .xsmall()
                .when(m == cur, |b| b.primary())
                .when(m != cur, |b| b.ghost())
                .disabled(!enabled)
                .on_click(move |_, _, cx| {
                    let _ = entity.update(cx, |this, cx| {
                        this.mode = m;
                        cx.notify();
                    });
                })
        };
        let ent_f = entity.clone();
        let filter_count = self.filters.read(cx).active_count();
        h_flex()
            .px_3()
            .py_1()
            .gap_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(mode_btn("m-default", "기본", Mode::Default, self.mode, true))
            .child(mode_btn(
                "m-pan",
                "이동",
                Mode::Pan,
                self.mode,
                drawn && zoomable && !self.read_only,
            ))
            .child(mode_btn(
                "m-zoom",
                "확대",
                Mode::Zoom,
                self.mode,
                drawn && zoomable && !self.read_only,
            ))
            .child(div().w(px(1.)).h(px(16.)).mx_2().bg(cx.theme().border))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("legend 항목 클릭: 숨기기 · 더블 클릭: 크게 보기 · 휠: 페이지"),
            )
            .child(div().flex_1())
            .child(
                Button::new("toggle-filters")
                    .label(if filter_count > 0 {
                        format!("필터 {filter_count}")
                    } else {
                        "필터".into()
                    })
                    .xsmall()
                    .when(self.show_filters, |b| b.primary())
                    .when(!self.show_filters, |b| b.outline())
                    .on_click(move |_, _, cx| {
                        let _ = ent_f.update(cx, |this, cx| {
                            this.show_filters = !this.show_filters;
                            cx.notify();
                        });
                    }),
            )
            .into_any_element()
    }

    fn render_item(&self, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let d = self.drawn.as_ref().unwrap();
        let theme = cx.theme().clone();
        let entity = cx.entity().downgrade();
        let bounds = self.item_bounds.clone();
        let selected = self.selected == Some(i);
        let img = d.images.get(&i);
        let pending = d.pending.contains(&i);
        let failed = d.main.progress.is_finished() && img.is_none() && !pending && d.main.progress.stage() == JobStage::Done && false;
        let _ = failed;
        let (e1, e2, e3) = (entity.clone(), entity.clone(), entity.clone());
        let has_view = d.states.lock().get(i).is_some_and(|s| s.view.is_some());
        let mut cell = div()
            .id(SharedString::from(format!("item-{i}")))
            .relative()
            .flex_1()
            .h_full()
            .min_w_0()
            .rounded_md()
            .overflow_hidden()
            .border_1()
            .border_color(if selected { theme.primary } else { theme.border })
            .bg(theme.background)
            .group("item")
            .cursor(match self.mode {
                Mode::Zoom => CursorStyle::Crosshair,
                Mode::Pan => CursorStyle::OpenHand,
                Mode::Default => CursorStyle::Arrow,
            })
            .on_mouse_move(move |e, window, cx| {
                let _ = e1.update(cx, |this, cx| this.on_move(i, false, e, window, cx));
            })
            .on_mouse_down(MouseButton::Left, move |e, window, cx| {
                let _ = e2.update(cx, |this, cx| this.on_down(i, false, e, window, cx));
            })
            .on_mouse_up(MouseButton::Left, move |e, window, cx| {
                let _ = e3.update(cx, |this, cx| this.on_up(false, e, window, cx));
            })
            .child(
                canvas(
                    move |b, _, _| {
                        bounds.borrow_mut().insert(i, b);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        match img {
            Some(img) => {
                cell = cell.child(img_el(img.image.clone()).absolute().size_full());
                if pending {
                    cell = cell.child(
                        div()
                            .absolute()
                            .top_1()
                            .left_1()
                            .child(Progress::new(SharedString::from(format!("p-{i}"))).loading(true).w(px(40.))),
                    );
                }
            }
            None => {
                let name = d.data.as_ref().map(|data| data.item_name(i)).unwrap_or_default();
                cell = cell.child(
                    v_flex()
                        .absolute()
                        .size_full()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(div().text_xs().text_color(theme.muted_foreground).child(name))
                        .child(Progress::new(SharedString::from(format!("p-{i}"))).loading(true).w(px(120.))),
                );
            }
        }
        // item 툴바: 축소 · 확대 초기화 · 크게 보기 (커서를 올리면 보인다)
        let (z1, z2, z3) = (entity.clone(), entity.clone(), entity);
        let zoomable = d.spec.chart_type.fields().zoom && !self.read_only;
        cell = cell.child(
            h_flex()
                .absolute()
                .top_1()
                .right_1()
                .gap_0p5()
                .invisible()
                .group_hover("item", |s| s.visible())
                .when(zoomable, |t| {
                    t.child(
                        Button::new(SharedString::from(format!("zo-{i}")))
                            .icon(IconName::Minimize)
                            .xsmall()
                            .ghost()
                            .tooltip("축소")
                            .on_click(move |_, window, cx| {
                                let _ = z1.update(cx, |this, cx| this.zoom_out(i, window, cx));
                            }),
                    )
                    .child(
                        Button::new(SharedString::from(format!("zr-{i}")))
                            .icon(IconName::RefreshCw)
                            .xsmall()
                            .ghost()
                            .disabled(!has_view)
                            .tooltip("확대 초기화")
                            .on_click(move |_, window, cx| {
                                let _ = z2.update(cx, |this, cx| this.update_state(i, window, cx, |s| s.view = None));
                            }),
                    )
                })
                .child(
                    Button::new(SharedString::from(format!("en-{i}")))
                        .icon(IconName::Maximize)
                        .xsmall()
                        .ghost()
                        .tooltip("크게 보기")
                        .on_click(move |_, window, cx| {
                            let _ = z3.update(cx, |this, cx| this.enlarge(Some(i), window, cx));
                        }),
                ),
        );
        cell = self.overlays(cell, i, false, cx);
        cell.into_any_element()
    }

    /// 툴팁 · 드래그 영역 · 가까운 점 표시
    fn overlays(&self, mut cell: Stateful<Div>, i: usize, big: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = cx.theme();
        let b = if big {
            self.big_bounds.get()
        } else {
            self.item_bounds.borrow().get(&i).copied().unwrap_or_default()
        };
        if let Some(drag) = self.drag.as_ref().filter(|d| d.item == i) {
            if self.mode == Mode::Zoom {
                let x0 = drag.start.x.min(drag.now.x) - b.origin.x;
                let y0 = drag.start.y.min(drag.now.y) - b.origin.y;
                let w = (drag.start.x - drag.now.x).abs();
                let h = (drag.start.y - drag.now.y).abs();
                cell = cell.child(
                    div()
                        .absolute()
                        .left(x0)
                        .top(y0)
                        .w(w)
                        .h(h)
                        .border_1()
                        .border_color(theme.primary)
                        .bg(theme.primary.opacity(0.08)),
                );
            }
        }
        if let Some(h) = self.hover.as_ref().filter(|h| h.item == i) {
            if let (Some((mx, my)), Some(img)) = (
                h.marker,
                self.drawn
                    .as_ref()
                    .and_then(|d| if big { d.big.as_ref().map(|b| &b.1) } else { d.images.get(&i) }),
            ) {
                let sx = f32::from(b.size.width) / img.css.0.max(1.0);
                let sy = f32::from(b.size.height) / img.css.1.max(1.0);
                cell = cell.child(
                    div()
                        .absolute()
                        .left(px(mx * sx - 6.))
                        .top(px(my * sy - 6.))
                        .size(px(12.))
                        .rounded_full()
                        .border_2()
                        .border_color(theme.foreground),
                );
            }
            let mut tip = v_flex()
                .px_2()
                .py_1()
                .gap_0p5()
                .rounded_md()
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .border_1()
                .border_color(theme.border)
                .shadow_md()
                .text_xs()
                .max_w(px(360.));
            for (k, line) in h.lines.iter().enumerate() {
                let mut row = h_flex().gap_1();
                if k == 0 {
                    if let Some(c) = h.color {
                        row = row.child(div().size(px(8.)).rounded_full().bg(hex_color(c)));
                    }
                }
                tip = tip.child(row.child(line.clone()));
            }
            // 칸 밖으로 넘쳐도 보이게 창 좌표에 띄운다
            let tip = deferred(
                anchored()
                    .position(h.pos + point(px(14.), px(16.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(tip),
            )
            .with_priority(1);
            cell = cell.child(tip);
        }
        cell
    }

    fn render_grid(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let entity = cx.entity().downgrade();
        let gb = self.grid_bounds.clone();
        let ent_measure = entity.clone();
        let measure = canvas(
            move |b, window, cx| {
                let old = gb.get();
                gb.set(b);
                if old.size != b.size {
                    let ent = ent_measure.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = ent.update(cx, |this, cx| this.on_grid_resized(window, cx));
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();

        let Some(d) = &self.drawn else {
            return v_flex()
                .flex_1()
                .relative()
                .items_center()
                .justify_center()
                .gap_2()
                .child(measure)
                .child(div().text_lg().child("왼쪽에서 설정을 고르고 draw를 누르세요."))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("split by로 나눈 조합마다 차트 한 장(item)을 그립니다."),
                )
                .into_any_element();
        };
        if let Some(e) = d.main.progress.error.lock().clone() {
            return v_flex()
                .flex_1()
                .relative()
                .items_center()
                .justify_center()
                .child(measure)
                .child(div().text_color(theme.danger).child(format!("그릴 수 없습니다: {e}")))
                .into_any_element();
        }
        if let Some(i) = self.enlarged {
            return self.render_big(i, measure, window, cx);
        }
        let (rows, cols) = d.spec.grid;
        let items = self.visible_items();
        let mut grid = v_flex().flex_1().h_full().min_w_0().gap_2().relative().child(measure);
        for r in 0..rows as usize {
            let mut row = div().flex().flex_row().flex_1().gap_2().min_h_0();
            for c in 0..cols as usize {
                let k = r * cols as usize + c;
                match items.get(k) {
                    Some(&i) => row = row.child(self.render_item(i, cx)),
                    None => row = row.child(div().flex_1().h_full()),
                }
            }
            grid = grid.child(row);
        }
        let pages = self.page_count();
        let page = d.page;
        let (p1, p2, p3) = (entity.clone(), entity.clone(), entity.clone());
        let ent_wheel = entity.clone();
        div()
            .flex()
            .flex_row()
            .id("grid-area")
            .flex_1()
            .min_h_0()
            .p_2()
            .gap_2()
            .on_scroll(move |e, window, cx| {
                let dy = f32::from(e.delta.pixel_delta(px(40.)).y);
                let _ = ent_wheel.update(cx, |this, cx| {
                    this.wheel += dy;
                    let cur = this.drawn.as_ref().map(|d| d.page).unwrap_or(0);
                    if this.wheel <= -120.0 {
                        this.wheel = 0.0;
                        this.set_page(cur + 1, window, cx);
                    } else if this.wheel >= 120.0 {
                        this.wheel = 0.0;
                        this.set_page(cur.saturating_sub(1), window, cx);
                    }
                });
            })
            .child(grid)
            .child(
                // 오른쪽 페이지 막대: ▲ 쪽수 ▼
                v_flex()
                    .w(px(36.))
                    .h_full()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new("pg-first")
                            .icon(IconName::ChevronUp)
                            .xsmall()
                            .ghost()
                            .disabled(page == 0)
                            .on_click(move |_, window, cx| {
                                let _ = p1.update(cx, |this, cx| this.set_page(page.saturating_sub(1), window, cx));
                            }),
                    )
                    .child(
                        div().flex_1().w(px(6.)).rounded_full().bg(theme.muted).relative().child(
                            div()
                                .absolute()
                                .left_0()
                                .w_full()
                                .rounded_full()
                                .bg(theme.primary)
                                .top(relative(page as f32 / pages as f32))
                                .h(relative((1.0 / pages as f32).max(0.03))),
                        ),
                    )
                    .child(
                        Button::new("pg-next")
                            .icon(IconName::ChevronDown)
                            .xsmall()
                            .ghost()
                            .disabled(page + 1 >= pages)
                            .on_click(move |_, window, cx| {
                                let _ = p2.update(cx, |this, cx| this.set_page(page + 1, window, cx));
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!("{}/{}", page + 1, pages)),
                    )
                    .child(
                        Button::new("pg-last")
                            .label("끝")
                            .xsmall()
                            .ghost()
                            .disabled(page + 1 >= pages)
                            .on_click(move |_, window, cx| {
                                let _ = p3.update(cx, |this, cx| this.set_page(usize::MAX, window, cx));
                            }),
                    ),
            )
            .into_any_element()
    }

    fn render_big(&mut self, i: usize, measure: Canvas<()>, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let entity = cx.entity().downgrade();
        let d = self.drawn.as_ref().unwrap();
        let n = d.len();
        let bb = self.big_bounds.clone();
        let (e1, e2, e3, e4, e5) = (entity.clone(), entity.clone(), entity.clone(), entity.clone(), entity.clone());
        let mut cell = div()
            .id("big-item")
            .relative()
            .flex_1()
            .w_full()
            .rounded_md()
            .overflow_hidden()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .cursor(match self.mode {
                Mode::Zoom => CursorStyle::Crosshair,
                Mode::Pan => CursorStyle::OpenHand,
                Mode::Default => CursorStyle::Arrow,
            })
            .on_mouse_move(move |e, window, cx| {
                let _ = e1.update(cx, |this, cx| this.on_move(i, true, e, window, cx));
            })
            .on_mouse_down(MouseButton::Left, move |e, window, cx| {
                let _ = e2.update(cx, |this, cx| {
                    if e.click_count >= 2 {
                        this.enlarge(None, window, cx);
                    } else {
                        this.on_down(i, true, e, window, cx)
                    }
                });
            })
            .on_mouse_up(MouseButton::Left, move |e, window, cx| {
                let _ = e3.update(cx, |this, cx| this.on_up(true, e, window, cx));
            })
            .child(canvas(move |b, _, _| bb.set(b), |_, _, _, _| {}).absolute().size_full());
        match d.big.as_ref().filter(|b| b.0 == i) {
            Some((_, img)) => cell = cell.child(img_el(img.image.clone()).absolute().size_full()),
            None => match d.images.get(&i) {
                Some(img) => cell = cell.child(img_el(img.image.clone()).absolute().size_full().opacity(0.6)),
                None => {
                    cell = cell.child(
                        div()
                            .absolute()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(Progress::new("big-p").loading(true).w(px(160.))),
                    )
                }
            },
        }
        cell = self.overlays(cell, i, true, cx);
        let name = d.data.as_ref().map(|data| data.item_name(i)).unwrap_or_default();
        v_flex()
            .flex_1()
            .min_h_0()
            .p_2()
            .gap_2()
            .relative()
            .child(measure)
            .child(cell)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("big-first")
                            .label("<<")
                            .xsmall()
                            .disabled(i == 0)
                            .on_click(move |_, window, cx| {
                                let _ = e4.update(cx, |this, cx| this.enlarge(Some(0), window, cx));
                            }),
                    )
                    .child(
                        Button::new("big-prev")
                            .icon(IconName::ChevronLeft)
                            .xsmall()
                            .disabled(i == 0)
                            .on_click({
                                let e = entity.clone();
                                move |_, window, cx| {
                                    let _ = e.update(cx, |this, cx| this.enlarge(Some(i.saturating_sub(1)), window, cx));
                                }
                            }),
                    )
                    .child(div().flex_1().text_center().text_sm().child(format!("{name}  ({}/{n})", i + 1)))
                    .child(
                        Button::new("big-next")
                            .icon(IconName::ChevronRight)
                            .xsmall()
                            .disabled(i + 1 >= n)
                            .on_click({
                                let e = entity.clone();
                                move |_, window, cx| {
                                    let _ = e.update(cx, |this, cx| this.enlarge(Some((i + 1).min(n - 1)), window, cx));
                                }
                            }),
                    )
                    .child(Button::new("big-last").label(">>").xsmall().disabled(i + 1 >= n).on_click({
                        let e = entity.clone();
                        move |_, window, cx| {
                            let _ = e.update(cx, |this, cx| this.enlarge(Some(n - 1), window, cx));
                        }
                    }))
                    .child(
                        Button::new("big-close")
                            .label("grid로 (더블 클릭)")
                            .xsmall()
                            .outline()
                            .on_click(move |_, window, cx| {
                                let _ = e5.update(cx, |this, cx| this.enlarge(None, window, cx));
                            }),
                    ),
            )
            .into_any_element()
    }

    fn render_job_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let mut bar = h_flex()
            .h(px(34.))
            .px_3()
            .gap_3()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.status_bar)
            .text_xs();
        let Some(d) = &self.drawn else {
            return bar
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(format!("렌더 스레드 {}개 대기", job::default_workers())),
                )
                .into_any_element();
        };
        let p = &d.main.progress;
        let total = p.total.load(Ordering::Acquire);
        let done = p.done.load(Ordering::Acquire);
        let running = p.running.load(Ordering::Acquire);
        let failed = p.failed.load(Ordering::Acquire);
        let elapsed = p.elapsed().as_secs_f64();
        let stage = p.stage();
        let ratio = if total > 0 { done as f32 / total as f32 } else { 0.0 };
        let rate = if elapsed > 0.0 { done as f64 / elapsed } else { 0.0 };
        let stage_label = match stage {
            JobStage::Query => "쿼리 중",
            JobStage::Render => "렌더링 중",
            JobStage::Done => "완료",
            JobStage::Failed => "실패",
            JobStage::Cancelled => "취소됨",
        };
        let remaining = if rate > 0.0 && done < total && stage == JobStage::Render {
            format!(" · 남은 시간 약 {}", fmt_ms((total - done) as f64 / rate * 1000.0))
        } else {
            String::new()
        };
        let avg_item = if d.item_ms_count > 0 {
            d.item_ms_total / d.item_ms_count as f64
        } else {
            0.0
        };
        bar = bar
            .child(div().font_weight(FontWeight::SEMIBOLD).child(stage_label))
            .child(
                Progress::new("job-progress")
                    .value(ratio * 100.0)
                    .loading(stage == JobStage::Query)
                    .w(px(160.)),
            )
            .child(format!("{:.0}%", ratio * 100.0))
            .child(format!("✓ {} / {}", fmt_int(done - failed.min(done)), fmt_int(total)))
            .when(running > 0, |b| b.child(format!("◐ {running}")))
            .when(failed > 0, |b| b.child(format!("✕ {failed}")))
            .when(stage == JobStage::Cancelled, |b| b.child(format!("취소 {}", fmt_int(total - done))))
            .child(format!(
                "{} · {} item/s{}",
                fmt_ms(elapsed * 1000.0),
                fmt_int(rate.round() as usize),
                remaining
            ))
            .child(div().flex_1())
            .child(div().text_color(theme.muted_foreground).child(format!(
                "쿼리 {} · item 평균 {} · 렌더 스레드 {} · 화면 이미지 {}개 {}",
                p.query_ms.lock().map(fmt_ms).unwrap_or("-".into()),
                fmt_ms(avg_item),
                p.workers,
                d.images.len(),
                crate::ui::fmt_bytes(d.image_bytes),
            )));
        bar.into_any_element()
    }
}

fn img_el(image: Arc<RenderImage>) -> Img {
    img(ImageSource::Render(image)).object_fit(ObjectFit::Fill)
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let left = self.render_left(window, cx);
        let toolbar = self.render_toolbar(cx);
        let grid = self.render_grid(window, cx);
        let bar = self.render_job_bar(cx);
        h_flex()
            .size_full()
            .min_h_0()
            .child(left)
            .child(v_flex().flex_1().min_w_0().h_full().child(toolbar).child(grid).child(bar))
            .when(self.show_filters, |d| d.child(self.filters.clone()))
    }
}
