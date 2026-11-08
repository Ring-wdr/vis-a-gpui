//! 제조/IoT 센서 데이터 절차적 생성기. vis-a-lot `server/data/generator.ts`를 그대로 옮겼다.
//! 원본은 CSV 조각을 만들어 PGlite에 COPY로 넣었고, 여기서는 열 배열을 바로 채운다.

use std::f64::consts::PI;

use crate::dataset::{CatBuilder, Column, ColumnData, ColumnKind, Dataset};
use crate::random::{Random, derive_seed};

pub const SEED: u32 = 20261020;
const HOUR: f64 = 3_600_000.0;
const DAY: f64 = 24.0 * HOUR;
/// 2026-09-01 00:00 KST
pub const START_MS: f64 = 1_788_188_400_000.0;
pub const DURATION_MS: f64 = 30.0 * DAY;

pub const FACTORIES: [&str; 3] = ["판교", "구미", "울산"];
pub const LINES_PER_FACTORY: usize = 4;
pub const MACHINES_PER_LINE: usize = 10;
pub const SENSOR_TYPES: [&str; 4] = ["온도", "진동", "압력", "전류"];
pub const SENSOR_COUNT: usize = FACTORIES.len() * LINES_PER_FACTORY * MACHINES_PER_LINE * SENSOR_TYPES.len();

pub struct Sample {
    pub id: &'static str,
    pub rows: usize,
    pub name: &'static str,
}

pub const SAMPLES: [Sample; 3] = [
    Sample {
        id: "iot-100k",
        rows: 100_000,
        name: "IoT 센서 · 10만 행",
    },
    Sample {
        id: "iot-500k",
        rows: 500_000,
        name: "IoT 센서 · 50만 행",
    },
    Sample {
        id: "iot-1m",
        rows: 1_000_000,
        name: "IoT 센서 · 100만 행",
    },
];

pub fn sample_of(id: &str) -> Option<&'static Sample> {
    SAMPLES.iter().find(|s| s.id == id)
}

struct Profile {
    base: (f64, f64),
    sigma: f64,
    drift: (f64, f64),
    daily_amp: f64,
    env_coef: f64,
    load_coef: f64,
    digits: i32,
}

const PROFILES: [Profile; 4] = [
    Profile {
        base: (55.0, 8.0),
        sigma: 0.8,
        drift: (-3.0, 3.0),
        daily_amp: 2.5,
        env_coef: 0.6,
        load_coef: 0.05,
        digits: 2,
    },
    Profile {
        base: (2.4, 0.5),
        sigma: 0.25,
        drift: (0.0, 1.2),
        daily_amp: 0.1,
        env_coef: 0.0,
        load_coef: 0.015,
        digits: 3,
    },
    Profile {
        base: (5.0, 0.6),
        sigma: 0.08,
        drift: (-0.3, 0.3),
        daily_amp: 0.05,
        env_coef: 0.02,
        load_coef: 0.004,
        digits: 3,
    },
    Profile {
        base: (18.0, 3.0),
        sigma: 0.9,
        drift: (-1.5, 1.5),
        daily_amp: 0.6,
        env_coef: 0.0,
        load_coef: 0.12,
        digits: 2,
    },
];

const SHIFTS: [&str; 3] = ["주간", "야간", "심야"];
const SHIFT_LOAD: [f64; 3] = [82.0, 71.0, 38.0];
const STATUSES: [&str; 3] = ["정상", "경고", "이상"];

#[derive(Clone, Copy, PartialEq)]
enum AnomalyKind {
    Spike,
    Level,
    Missing,
}

struct Anomaly {
    kind: AnomalyKind,
    start: f64,
    end: f64,
    offset: f64,
}

struct Sensor {
    factory: usize,
    line: usize,
    line_label: usize,
    machine: usize,
    ty: usize,
    base: f64,
    drift: f64,
    anomalies: Vec<Anomaly>,
    cursor: usize,
    rng: Random,
}

fn shift_of(offset_ms: f64) -> usize {
    let hour = ((offset_ms / HOUR).floor() as i64) % 24;
    if (6..14).contains(&hour) {
        0
    } else if (14..22).contains(&hour) {
        1
    } else {
        2
    }
}

/// JS Math.round(v * 10^d) / 10^d
fn round_js(v: f64, digits: i32) -> f64 {
    let p = 10f64.powi(digits);
    (v * p + 0.5).floor() / p
}

fn build_anomalies(rng: &mut Random, sigma: f64, interval_ms: f64) -> Vec<Anomaly> {
    let mut list = Vec::new();
    let count = rng.int(2, 8);
    for _ in 0..count {
        let roll = rng.next();
        let start = rng.range(0.0, DURATION_MS);
        let sign = if rng.next() < 0.5 { -1.0 } else { 1.0 };
        if roll < 0.5 {
            let n = rng.int(1, 3) as f64;
            let off = sign * rng.range(5.0, 8.0) * sigma;
            list.push(Anomaly {
                kind: AnomalyKind::Spike,
                start,
                end: start + interval_ms * n,
                offset: off,
            });
        } else if roll < 0.8 {
            let len = rng.range(2.0, 12.0) * HOUR;
            let off = sign * rng.range(2.5, 4.0) * sigma;
            list.push(Anomaly {
                kind: AnomalyKind::Level,
                start,
                end: start + len,
                offset: off,
            });
        } else {
            let len = rng.range(1.0, 6.0) * HOUR;
            list.push(Anomaly {
                kind: AnomalyKind::Missing,
                start,
                end: start + len,
                offset: 0.0,
            });
        }
    }
    list.sort_by(|a, b| a.start.total_cmp(&b.start));
    list
}

fn build_sensors(interval_ms: f64) -> Vec<Sensor> {
    let mut sensors = Vec::with_capacity(SENSOR_COUNT);
    let mut machine_no = 0;
    for f in 0..FACTORIES.len() {
        for l in 0..LINES_PER_FACTORY {
            for _m in 0..MACHINES_PER_LINE {
                machine_no += 1;
                let machine = format!("M{machine_no:03}");
                for (t, ty) in SENSOR_TYPES.iter().enumerate() {
                    let mut rng = Random::new(derive_seed(SEED, &format!("{machine}:{ty}")));
                    let p = &PROFILES[t];
                    let base = p.base.0 + rng.normal() * p.base.1;
                    let drift = rng.range(p.drift.0, p.drift.1);
                    let anomalies = build_anomalies(&mut rng, p.sigma, interval_ms);
                    sensors.push(Sensor {
                        factory: f,
                        line: f * LINES_PER_FACTORY + l,
                        line_label: l,
                        machine: machine_no - 1,
                        ty: t,
                        base,
                        drift,
                        anomalies,
                        cursor: 0,
                        rng,
                    });
                }
            }
        }
    }
    sensors
}

fn build_lots(line_count: usize) -> Vec<Vec<f64>> {
    (0..line_count)
        .map(|line| {
            let mut rng = Random::new(derive_seed(SEED, &format!("lot:{line}")));
            let mut ends = Vec::new();
            let mut t = 0.0;
            while t < DURATION_MS {
                t += rng.range(1.0, 2.5) * HOUR;
                ends.push(t);
            }
            ends
        })
        .collect()
}

fn active_anomaly(s: &mut Sensor, offset: f64) -> Option<usize> {
    while s.cursor < s.anomalies.len() && s.anomalies[s.cursor].end <= offset {
        s.cursor += 1;
    }
    for i in s.cursor..s.anomalies.len() {
        let a = &s.anomalies[i];
        if a.start > offset {
            break;
        }
        if a.end > offset {
            return Some(i);
        }
    }
    None
}

/// 진행률 콜백(0..1)을 받으며 `rows`개 행을 만든다.
pub fn generate(id: &str, name: &str, rows: usize, mut progress: impl FnMut(f64)) -> Dataset {
    let steps = rows.div_ceil(SENSOR_COUNT);
    let interval_ms = DURATION_MS / steps as f64;
    let mut sensors = build_sensors(interval_ms);
    let line_count = FACTORIES.len() * LINES_PER_FACTORY;
    let lots = build_lots(line_count);
    let mut lot_cursor = vec![0usize; line_count];
    let mut env_rng = Random::new(derive_seed(SEED, "env"));
    let mut load_rng = Random::new(derive_seed(SEED, "load"));
    let line_load_offset: Vec<f64> = (0..line_count).map(|_| load_rng.range(-6.0, 6.0)).collect();

    let mut ts = Vec::with_capacity(rows);
    let mut value = Vec::with_capacity(rows);
    let mut temp_env = Vec::with_capacity(rows);
    let mut load_pct = Vec::with_capacity(rows);
    let mut factory = CatBuilder::with_capacity(rows);
    let mut line = CatBuilder::with_capacity(rows);
    let mut machine = CatBuilder::with_capacity(rows);
    let mut sensor = CatBuilder::with_capacity(rows);
    let mut shift_col = CatBuilder::with_capacity(rows);
    let mut lot = CatBuilder::with_capacity(rows);
    let mut status = CatBuilder::with_capacity(rows);

    // 사전 값은 미리 넣어 두고 코드만 push한다 (행마다 문자열을 만들지 않는다)
    let f_codes: Vec<u32> = FACTORIES.iter().map(|v| factory.intern(v)).collect();
    let l_codes: Vec<u32> = (1..=LINES_PER_FACTORY).map(|l| line.intern(&format!("L{l}"))).collect();
    let m_codes: Vec<u32> = (1..=FACTORIES.len() * LINES_PER_FACTORY * MACHINES_PER_LINE)
        .map(|m| machine.intern(&format!("M{m:03}")))
        .collect();
    let t_codes: Vec<u32> = SENSOR_TYPES.iter().map(|v| sensor.intern(v)).collect();
    let sh_codes: Vec<u32> = SHIFTS.iter().map(|v| shift_col.intern(v)).collect();
    let st_codes: Vec<u32> = STATUSES.iter().map(|v| status.intern(v)).collect();
    // LOT 코드: [라인][순번] → 코드
    let lot_codes: Vec<Vec<u32>> = lots
        .iter()
        .enumerate()
        .map(|(li, ends)| {
            let f = li / LINES_PER_FACTORY;
            let l = li % LINES_PER_FACTORY + 1;
            (0..ends.len())
                .map(|k| lot.intern(&format!("{}L{}-{:04}", f + 1, l, k + 1)))
                .collect()
        })
        .collect();

    let mut produced = 0usize;
    let mut env = [0.0f64; 3];
    let mut load = vec![0.0f64; line_count];
    let mut lot_now = vec![0u32; line_count];
    for step in 0..steps {
        if produced >= rows {
            break;
        }
        let offset = step as f64 * interval_ms;
        let t_ms = (START_MS + offset).trunc();
        let hour_of_day = (offset / HOUR) % 24.0;
        let shift = shift_of(offset);
        for (f, e) in env.iter_mut().enumerate() {
            *e = 22.0
                + f as f64 * 1.5
                + 4.0 * ((2.0 * PI * (hour_of_day - 9.0)) / 24.0).sin()
                + 2.0 * ((2.0 * PI * offset) / DURATION_MS).sin()
                + env_rng.normal() * 0.3;
        }
        for (li, o) in line_load_offset.iter().enumerate() {
            load[li] = (SHIFT_LOAD[shift] + o + load_rng.normal() * 4.0).clamp(5.0, 100.0);
        }
        for (li, ends) in lots.iter().enumerate() {
            while lot_cursor[li] < ends.len() - 1 && ends[lot_cursor[li]] <= offset {
                lot_cursor[li] += 1;
            }
            lot_now[li] = lot_codes[li][lot_cursor[li]];
        }
        let env_r = env.map(|e| round_js(e, 2));
        let load_r: Vec<f64> = load.iter().map(|&v| round_js(v, 1)).collect();

        for s in sensors.iter_mut() {
            if produced >= rows {
                break;
            }
            let p = &PROFILES[s.ty];
            let noise = s.rng.normal();
            let anomaly = active_anomaly(s, offset).map(|i| (s.anomalies[i].kind, s.anomalies[i].offset));
            let mut v = f64::NAN;
            let mut st = 0;
            if matches!(anomaly, Some((AnomalyKind::Missing, _))) {
                st = 2;
            } else {
                let raw = s.base
                    + s.drift * (offset / DURATION_MS)
                    + p.daily_amp * ((2.0 * PI * (hour_of_day - 14.0)) / 24.0).sin()
                    + p.env_coef * (env[s.factory] - 22.0)
                    + p.load_coef * (load[s.line] - 60.0)
                    + noise * p.sigma
                    + anomaly.map(|a| a.1).unwrap_or(0.0);
                v = round_js(raw, p.digits);
                if anomaly.is_some() {
                    st = 2;
                } else if noise.abs() > 2.0 {
                    st = 1;
                }
            }
            ts.push(t_ms);
            factory.push_code(f_codes[s.factory]);
            line.push_code(l_codes[s.line_label]);
            machine.push_code(m_codes[s.machine]);
            sensor.push_code(t_codes[s.ty]);
            shift_col.push_code(sh_codes[shift]);
            lot.push_code(lot_now[s.line]);
            status.push_code(st_codes[st]);
            value.push(v);
            temp_env.push(env_r[s.factory]);
            load_pct.push(load_r[s.line]);
            produced += 1;
        }
        if step % 64 == 0 {
            progress(produced as f64 / rows as f64);
        }
    }
    progress(1.0);

    let col = |name: &str, label: &str, kind, data| Column {
        name: name.into(),
        label: label.into(),
        kind,
        data,
    };
    let columns = vec![
        col("ts", "측정 시각", ColumnKind::Datetime, ColumnData::Num(ts)),
        col("factory", "공장", ColumnKind::Categorical, factory.finish()),
        col("line", "라인", ColumnKind::Categorical, line.finish()),
        col("machine", "설비", ColumnKind::Categorical, machine.finish()),
        col("sensor_type", "센서 종류", ColumnKind::Categorical, sensor.finish()),
        col("shift", "근무조", ColumnKind::Categorical, shift_col.finish()),
        col("lot_id", "LOT", ColumnKind::Categorical, lot.finish()),
        col("status", "상태", ColumnKind::Categorical, status.finish()),
        col("value", "측정값", ColumnKind::Numeric, ColumnData::Num(value)),
        col("temperature_env", "주변 온도", ColumnKind::Numeric, ColumnData::Num(temp_env)),
        col("load_pct", "부하율(%)", ColumnKind::Numeric, ColumnData::Num(load_pct)),
    ];
    Dataset {
        id: id.into(),
        name: name.into(),
        rows: produced,
        columns,
        created_ms: chrono::Utc::now().timestamp_millis(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_is_kst_midnight() {
        assert_eq!(crate::timefmt::format_kst(START_MS, "%Y-%m-%d %H:%M"), "2026-09-01 00:00");
    }

    #[test]
    fn generates_rows() {
        let d = generate("t", "t", 10_000, |_| {});
        assert_eq!(d.rows, 10_000);
        let (_, dict) = d.column("factory").unwrap().cat().unwrap();
        assert_eq!(dict.len(), 3);
        let (_, m) = d.column("machine").unwrap().cat().unwrap();
        assert_eq!(m.len(), 120);
    }
}

/// 원본 JS 생성기(`server/data/generator.ts`)와 같은 행을 만드는지 확인한다. 기대값은 원본을 tsx로 돌려 얻었다.
#[cfg(test)]
mod parity {
    use super::*;
    use crate::timefmt::iso;

    fn row(d: &Dataset, i: usize) -> String {
        let c = |n: &str| d.column(n).unwrap();
        let num = |n: &str| {
            let v = c(n).nums().unwrap()[i];
            if v.is_nan() { String::new() } else { format!("{v}") }
        };
        format!(
            "{},{},{},{},{},{},{},{},{},{},{}",
            iso(c("ts").nums().unwrap()[i]),
            c("factory").text_at(i),
            c("line").text_at(i),
            c("machine").text_at(i),
            c("sensor_type").text_at(i),
            c("shift").text_at(i),
            c("lot_id").text_at(i),
            c("status").text_at(i),
            num("value"),
            num("temperature_env"),
            num("load_pct"),
        )
    }

    #[test]
    fn same_rows_as_js() {
        let d = generate("iot-100k", "t", 100_000, |_| {});
        let expect = [
            (0, "2026-08-31T15:00:00.000Z,판교,L1,M001,온도,심야,1L1-0001,정상,57.94,19,33.4"),
            (1, "2026-08-31T15:00:00.000Z,판교,L1,M001,진동,심야,1L1-0001,정상,1.867,19,33.4"),
            (
                479,
                "2026-08-31T15:00:00.000Z,울산,L4,M120,전류,심야,3L4-0001,정상,18.33,22.44,35.3",
            ),
            (480, "2026-08-31T18:26:41.913Z,판교,L1,M001,온도,심야,1L1-0003,정상,55.43,18.34,34"),
            (
                12345,
                "2026-09-04T05:07:27.846Z,울산,L1,M087,진동,야간,3L1-0053,정상,3.074,30.14,76.2",
            ),
            (
                99999,
                "2026-09-30T11:33:18.086Z,판교,L4,M040,전류,야간,1L4-0412,경고,22.85,22.78,77.5",
            ),
        ];
        for (i, want) in expect {
            assert_eq!(row(&d, i), want, "row {i}");
        }
        let v = d.column("value").unwrap().nums().unwrap();
        let nulls = v.iter().filter(|x| x.is_nan()).count();
        let sum: f64 = v.iter().filter(|x| !x.is_nan()).sum();
        assert_eq!(nulls, 495);
        assert!((sum - 2046526.5290).abs() < 0.01, "sum {sum}");
    }
}
