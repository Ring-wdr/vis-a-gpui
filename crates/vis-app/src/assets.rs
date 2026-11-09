//! 앱 아이콘 자산.
//! - gpui-kit 기본 `Assets`는 컴포넌트가 쓰는 Lucide 101개만 담는다.
//! - 원본(vis-a-lot, lucide-react)과 같은 아이콘 중 기본 묶음에 없는 것은 `icon_assets!`로 골라 더한다
//!   (gpui-kit에 Lucide 1,818개가 모두 들어 있고, 고른 SVG만 바이너리에 들어간다).
//! - 차트 종류 아이콘은 원본 `entities/chart/ui/ChartIcon.tsx`의 그림을 SVG 파일로 옮긴 것이다.

use std::borrow::Cow;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::Icon;
use gpui_kit::*;

use vis_core::spec::ChartType;

gpui_kit::assets::icon_assets!(pub LucideExtra, [
    Database, Upload, LayoutGrid, RotateCcwClock, Gauge,
    MousePointer2, Move, ScanSearch, ZoomIn, ZoomOut, RotateCcw, Maximize2, Minimize2,
    Table, TableProperties, Grid3x3, Funnel, Trash, FileUp, Ban, Hourglass, CircleDashed,
    PanelTop, Check, Download, X, Split, MessageSquareText, SlidersHorizontal, EyeOff,
    ChevronsLeft, ChevronsRight, Square, Play, ChartNoAxesCombined, Settings, Settings2, Cpu,
    CircleCheck, CircleX, LoaderCircle, SquareCheck, PanelBottom, PanelLeft, PanelRight, TriangleAlert, ChevronLeft, ChevronRight, Plus, Sun, Moon,
    SquareDashed, Lasso, SquareX, Eraser, Paintbrush, Slash, PenLine, Type, Undo2, ArchiveRestore, ImageDown, FileSpreadsheet,
    Star, Palette, Pencil,
]);

const CHART_ICONS: &[(&str, &[u8])] = &[
    ("icons/chart-scatter.svg", include_bytes!("../assets/icons/chart-scatter.svg")),
    ("icons/chart-bar.svg", include_bytes!("../assets/icons/chart-bar.svg")),
    ("icons/chart-boxplot.svg", include_bytes!("../assets/icons/chart-boxplot.svg")),
    ("icons/chart-line.svg", include_bytes!("../assets/icons/chart-line.svg")),
    ("icons/chart-sankey.svg", include_bytes!("../assets/icons/chart-sankey.svg")),
    ("icons/chart-heatmap.svg", include_bytes!("../assets/icons/chart-heatmap.svg")),
    ("icons/chart-treemap.svg", include_bytes!("../assets/icons/chart-treemap.svg")),
    ("icons/chart-candle.svg", include_bytes!("../assets/icons/chart-candle.svg")),
    ("icons/chart-gantt.svg", include_bytes!("../assets/icons/chart-gantt.svg")),
    ("icons/chart-pie.svg", include_bytes!("../assets/icons/chart-pie.svg")),
    ("icons/chart-radar.svg", include_bytes!("../assets/icons/chart-radar.svg")),
    ("icons/chart-histogram.svg", include_bytes!("../assets/icons/chart-histogram.svg")),
    ("icons/chart-waterfall.svg", include_bytes!("../assets/icons/chart-waterfall.svg")),
    ("icons/chart-funnel.svg", include_bytes!("../assets/icons/chart-funnel.svg")),
];

/// 차트 아이콘 → 고른 Lucide → gpui-kit 기본 묶음 순으로 찾는다
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = CHART_ICONS.iter().find(|(p, _)| *p == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some(b) = LucideExtra.load(path)? {
            return Ok(Some(b));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut out: Vec<SharedString> = CHART_ICONS
            .iter()
            .map(|(p, _)| SharedString::from(*p))
            .filter(|p| p.starts_with(path))
            .collect();
        out.extend(LucideExtra.list(path)?);
        out.extend(gpui_kit::assets::Assets.list(path)?);
        out.sort();
        out.dedup();
        Ok(out)
    }
}

/// Lucide 아이콘 (기본 묶음 + 위에서 고른 것)
pub fn icon(name: Lucide) -> Icon {
    Icon::new(name)
}

/// 차트 종류 아이콘 (원본 ChartIcon)
pub fn chart_icon(t: ChartType) -> Icon {
    Icon::empty().path(format!("icons/chart-{}.svg", t.name()))
}
