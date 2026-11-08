//! 시각 글자. 축·툴팁은 KST(UTC+9)로 읽히게 한다 (원본과 같다).

pub const KST_OFFSET_MS: f64 = 9.0 * 3_600_000.0;
pub const HOUR_MS: f64 = 3_600_000.0;
pub const DAY_MS: f64 = 86_400_000.0;

/// epoch 일수 → (년, 월, 일). Howard Hinnant의 civil_from_days
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m = m as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

/// UTC 기준 epoch ms를 strftime 일부(%Y %m %d %H %M %S %L %y)로
pub fn format_utc(ms: f64, fmt: &str) -> String {
    if !ms.is_finite() {
        return String::new();
    }
    let total = ms.floor() as i64;
    let days = total.div_euclid(86_400_000);
    let rem = total.rem_euclid(86_400_000);
    let (y, mo, d) = civil_from_days(days);
    let h = rem / 3_600_000;
    let mi = (rem / 60_000) % 60;
    let s = (rem / 1000) % 60;
    let l = rem % 1000;
    let mut out = String::with_capacity(fmt.len() + 8);
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('Y') => out.push_str(&format!("{y:04}")),
            Some('y') => out.push_str(&format!("{:02}", y.rem_euclid(100))),
            Some('m') => out.push_str(&format!("{mo:02}")),
            Some('d') => out.push_str(&format!("{d:02}")),
            Some('H') => out.push_str(&format!("{h:02}")),
            Some('M') => out.push_str(&format!("{mi:02}")),
            Some('S') => out.push_str(&format!("{s:02}")),
            Some('L') => out.push_str(&format!("{l:03}")),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

pub fn format_kst(ms: f64, fmt: &str) -> String {
    format_utc(ms + KST_OFFSET_MS, fmt)
}

/// ISO 8601 글자 (JS Date.toISOString과 같은 모양)
pub fn iso(ms: f64) -> String {
    format_utc(ms, "%Y-%m-%dT%H:%M:%S.%LZ")
}

/// 보이는 범위가 좁을수록 자세히 (원본 timeFormatter)
pub fn axis_format(span: f64) -> &'static str {
    if span > 2.0 * DAY_MS {
        "%m/%d"
    } else if span > DAY_MS / 4.0 {
        "%m/%d %H시"
    } else if span > 10.0 * 60_000.0 {
        "%H:%M"
    } else if span > 5_000.0 {
        "%H:%M:%S"
    } else {
        "%H:%M:%S.%L"
    }
}

/// "2026-09-01T00:00:00Z", "2026-09-01 00:00:00", "2026-09-01" 등을 epoch ms로. 시간대가 없으면 UTC로 본다.
pub fn parse_datetime(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.len() < 8 {
        return None;
    }
    let b = s.as_bytes();
    let num = |a: usize, n: usize| -> Option<i64> {
        let part = s.get(a..a + n)?;
        if !part.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let y = num(0, 4)?;
    let sep = *b.get(4)?;
    if sep != b'-' && sep != b'/' && sep != b'.' {
        return None;
    }
    let mo = num(5, 2)? as u32;
    if b.get(7) != Some(&sep) {
        return None;
    }
    let d = num(8, 2)? as u32;
    if !(1..=12).contains(&mo) || d == 0 || d > days_in_month(y, mo) {
        return None;
    }
    let mut ms = days_from_civil(y, mo, d) as f64 * DAY_MS;
    let rest = &s[10..];
    if rest.is_empty() {
        return Some(ms);
    }
    let rest = rest.strip_prefix('T').or_else(|| rest.strip_prefix(' '))?;
    let rb = rest.as_bytes();
    let n2 = |a: usize| -> Option<f64> {
        let p = rest.get(a..a + 2)?;
        if !p.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        p.parse::<f64>().ok()
    };
    let h = n2(0)?;
    if rb.get(2) != Some(&b':') {
        return None;
    }
    let mi = n2(3)?;
    let mut at = 5;
    let mut sec = 0.0;
    if rb.get(5) == Some(&b':') {
        sec = n2(6)?;
        at = 8;
        if rb.get(8) == Some(&b'.') {
            let mut end = 9;
            while end < rb.len() && rb[end].is_ascii_digit() {
                end += 1;
            }
            let frac: f64 = format!("0.{}", &rest[9..end]).parse().ok()?;
            sec += frac;
            at = end;
        }
    }
    if h >= 24.0 || mi >= 60.0 || sec >= 61.0 {
        return None;
    }
    ms += h * HOUR_MS + mi * 60_000.0 + sec * 1000.0;
    let tz = rest[at..].trim();
    if tz.is_empty() || tz == "Z" {
        return Some(ms);
    }
    let sign = match tz.as_bytes()[0] {
        b'+' => 1.0,
        b'-' => -1.0,
        _ => return None,
    };
    let digits: String = tz[1..].chars().filter(|c| c.is_ascii_digit()).collect();
    let (oh, om) = match digits.len() {
        2 => (digits.parse::<f64>().ok()?, 0.0),
        4 => (digits[..2].parse::<f64>().ok()?, digits[2..].parse::<f64>().ok()?),
        _ => return None,
    };
    Some(ms - sign * (oh * HOUR_MS + om * 60_000.0))
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let ms = parse_datetime("2026-08-31T15:00:00Z").unwrap();
        assert_eq!(iso(ms), "2026-08-31T15:00:00.000Z");
        assert_eq!(format_kst(ms, "%Y-%m-%d %H:%M"), "2026-09-01 00:00");
        assert_eq!(
            parse_datetime("2026-09-01 09:00:00+09:00").unwrap(),
            parse_datetime("2026-09-01T00:00:00Z").unwrap()
        );
        assert!(parse_datetime("2026-02-30").is_none());
        assert!(parse_datetime("hello").is_none());
    }
}
