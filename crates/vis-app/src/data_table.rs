//! 데이터 테이블 (원본 4.7.2): 고른 item의 원본 행을 데이터셋의 모든 컬럼으로 본다.
//! gpui-kit DataTable(가상 스크롤·열 크기 조절·정렬)을 쓴다. 정렬은 열 배열을 바로 비교한다.

use std::sync::Arc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::table::{Column, ColumnSort, DataTable, TableDelegate, TableState};
use gpui_kit::*;

use vis_core::dataset::{ColumnData, ColumnKind, Dataset, NULL_CODE};

pub struct RowsDelegate {
    ds: Arc<Dataset>,
    /// 원래 순서 (정렬 없음으로 되돌릴 때)
    original: Vec<u32>,
    rows: Vec<u32>,
}

impl RowsDelegate {
    pub fn new(ds: Arc<Dataset>, rows: Vec<u32>) -> Self {
        Self {
            ds,
            original: rows.clone(),
            rows,
        }
    }
}

impl TableDelegate for RowsDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.ds.columns.len() + 1
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        if col_ix == 0 {
            return Column::new("#", "#").width(70.).resizable(false).movable(false);
        }
        let c = &self.ds.columns[col_ix - 1];
        let col = Column::new(SharedString::from(c.name.clone()), SharedString::from(c.label.clone())).sortable();
        match c.kind {
            ColumnKind::Datetime => col.width(170.),
            ColumnKind::Numeric => col.width(100.).text_right(),
            ColumnKind::Categorical => col.width(110.),
        }
    }

    fn perform_sort(&mut self, col_ix: usize, sort: ColumnSort, _: &mut Window, _: &mut Context<TableState<Self>>) {
        if col_ix == 0 {
            return;
        }
        let c = &self.ds.columns[col_ix - 1];
        match sort {
            ColumnSort::Default => self.rows = self.original.clone(),
            ColumnSort::Ascending | ColumnSort::Descending => {
                let desc = matches!(sort, ColumnSort::Descending);
                match &c.data {
                    ColumnData::Num(v) => self.rows.sort_by(|&a, &b| {
                        let o = v[a as usize].total_cmp(&v[b as usize]);
                        if desc { o.reverse() } else { o }
                    }),
                    // 사전이 정렬돼 있어 코드 순서가 값 순서다 (null은 맨 뒤)
                    ColumnData::Cat { codes, .. } => self.rows.sort_by(|&a, &b| {
                        let o = codes[a as usize].cmp(&codes[b as usize]);
                        if desc && codes[a as usize] != NULL_CODE && codes[b as usize] != NULL_CODE {
                            o.reverse()
                        } else {
                            o
                        }
                    }),
                }
            }
        }
    }

    fn render_td(&mut self, row_ix: usize, col_ix: usize, _: &mut Window, _: &mut Context<TableState<Self>>) -> impl IntoElement {
        let r = self.rows[row_ix] as usize;
        if col_ix == 0 {
            return SharedString::from((row_ix + 1).to_string());
        }
        SharedString::from(self.ds.columns[col_ix - 1].text_at(r))
    }
}

/// 다이얼로그로 연다
pub fn open(title: String, ds: Arc<Dataset>, rows: Vec<u32>, filters: usize, window: &mut Window, cx: &mut App) {
    let subtitle = format!(
        "{}행 · draw 필터 {}개 적용 · 체크박스로 끈 legend 값·지운 점은 빠짐",
        vis_core::render::charts_fmt_int(rows.len()),
        filters
    );
    open_with(format!("데이터 테이블 · {title}"), subtitle, ds, rows, window, cx);
}

/// 전체 데이터 테이블 (원본 TableProperties): 데이터셋 전체에서 고르게 뽑은 30,000행 표본
pub const FULL_SAMPLE: usize = 30_000;

pub fn open_full(ds: Arc<Dataset>, window: &mut Window, cx: &mut App) {
    let total = ds.rows;
    let rows: Vec<u32> = if total <= FULL_SAMPLE {
        (0..total as u32).collect()
    } else {
        (0..FULL_SAMPLE).map(|k| (k * total / FULL_SAMPLE) as u32).collect()
    };
    let subtitle = format!(
        "전체 {}행 중 {}행 (고르게 뽑은 표본, 필터·split 미적용)",
        vis_core::render::charts_fmt_int(total),
        vis_core::render::charts_fmt_int(rows.len())
    );
    open_with(format!("전체 데이터 · {}", ds.name), subtitle, ds, rows, window, cx);
}

fn open_with(title: String, subtitle: String, ds: Arc<Dataset>, rows: Vec<u32>, window: &mut Window, cx: &mut App) {
    let state = cx.new(|cx| {
        TableState::new(RowsDelegate::new(ds, rows), window, cx)
            .sortable(true)
            .col_resizable(true)
    });
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(title.clone())
            .width(px(1000.))
            .child(div().text_xs().pb_2().child(subtitle.clone()))
            .child(div().h(px(520.)).child(DataTable::new(&state).stripe(true).bordered(true)))
    });
}
