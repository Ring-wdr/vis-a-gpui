//! scatter·line 원자료 (행 하나 = 점 하나). 원본은 split 조합마다 x·y·행 id를 글자로 이어 받아
//! 다시 나눴다 (queryChartData). 여기서는 같은 프로세스의 열 배열에서 그룹의 행 번호로 바로 모은다.

use super::LegendCodes;

pub struct PointsData {
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
    /// 행 번호 (툴팁 추가 컬럼 조회)
    pub ids: Vec<u32>,
    pub legend: Option<Vec<u8>>,
}

pub fn gather(rows: &[u32], x: &[f64], y: &[f64], legend: Option<&LegendCodes>) -> PointsData {
    let n = rows.len();
    let mut xs = Vec::with_capacity(n);
    let mut ys = Vec::with_capacity(n);
    for &r in rows {
        xs.push(x[r as usize]);
        ys.push(y[r as usize]);
    }
    PointsData {
        xs,
        ys,
        ids: rows.to_vec(),
        legend: legend.map(|l| rows.iter().map(|&r| l.at(r).min(255) as u8).collect()),
    }
}
