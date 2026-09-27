//! Header timestamps → Unix milliseconds (UTC), without a date library: FITS/XISF
//! `DATE-OBS` (ISO 8601, UTC by definition) and EXIF `DateTimeOriginal` (local time; the
//! zone comes from `OffsetTimeOriginal` or a GPS time, see `exif`).

/// Days from 1970-01-01 to a proleptic Gregorian date (H. Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn num(s: &str) -> Option<i64> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// `YYYY-MM-DD[T ]hh:mm:ss[.fff…]` (FITS/ISO, optional trailing `Z`) or
/// `YYYY:MM:DD hh:mm:ss` (EXIF) → Unix ms, **read as UTC** (callers apply any zone). A
/// date without a time, blanks (EXIF writes `    :  :     :  :  ` for unknown) or
/// anything out of range is None: a wrong time is worse than none.
pub(crate) fn civil_ms(s: &str) -> Option<i64> {
    let s = s.trim().trim_end_matches('Z');
    if s.len() < 19 || !s.is_char_boundary(19) {
        return None;
    }
    let (head, frac) = s.split_at(19);
    let b = head.as_bytes();
    let date_sep = b[4];
    if !matches!(date_sep, b'-' | b':')
        || b[7] != date_sep
        || !matches!(b[10], b'T' | b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let (y, mo, d) = (num(&head[0..4])?, num(&head[5..7])?, num(&head[8..10])?);
    let (h, mi, sec) = (
        num(&head[11..13])?,
        num(&head[14..16])?,
        num(&head[17..19])?,
    );
    if !((1900..=2200).contains(&y)
        && (1..=12).contains(&mo)
        && (1..=31).contains(&d)
        && h < 24
        && mi < 60
        && sec <= 60)
    {
        return None;
    }
    let ms = match frac.strip_prefix('.') {
        None if frac.is_empty() => 0,
        Some(f) => subsec_ms(f)?,
        None => return None,
    };
    Some(((days_from_civil(y, mo, d) * 24 + h) * 60 + mi) * 60_000 + sec * 1000 + ms)
}

/// Fractional-second digits (`.5`, `.123456`, EXIF SubSecTime `"12"`) → milliseconds.
pub(crate) fn subsec_ms(digits: &str) -> Option<i64> {
    let d = digits.trim();
    if d.is_empty() || !d.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut padded: String = d.chars().take(3).collect();
    while padded.len() < 3 {
        padded.push('0');
    }
    padded.parse().ok()
}

/// EXIF OffsetTime `±HH:MM` → milliseconds east of UTC. Anything else (blanks, `Z`,
/// missing colon) is None.
pub(crate) fn offset_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() != 6 || b[3] != b':' {
        return None;
    }
    let sign = match b[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let (h, m) = (num(&s[1..3])?, num(&s[4..6])?);
    (h <= 14 && m < 60).then_some(sign * (h * 60 + m) * 60_000)
}

/// Mid-exposure from the start time: plus half the exposure when it is known, since a 600 s
/// frame's midpoint is 5 minutes later and the moon moves 2.5′ in that time. (A header
/// that states the midpoint, FITS `DATE-AVG`, is used as is by the caller.)
pub(crate) fn mid_exposure(start_ms: Option<i64>, exposure_s: Option<f64>) -> Option<i64> {
    let start = start_ms?;
    Some(start + exposure_s.map_or(0, |e| (e * 500.0).round() as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_and_exif_forms_agree_with_known_epochs() {
        // 2026-03-19T00:03:09Z = 1773878589 s
        assert_eq!(civil_ms("2026-03-19T00:03:09"), Some(1_773_878_589_000));
        assert_eq!(civil_ms("2026:03:19 00:03:09"), Some(1_773_878_589_000));
        assert_eq!(
            civil_ms("2026-03-19T00:03:09.250Z"),
            Some(1_773_878_589_250)
        );
        assert_eq!(civil_ms("1970-01-01T00:00:00"), Some(0));
        // Leap day and a century non-leap year boundary
        assert_eq!(civil_ms("2024-02-29T12:00:00"), Some(1_709_208_000_000));
        assert_eq!(civil_ms("2100-03-01T00:00:00"), Some(4_107_542_400_000));
    }

    #[test]
    fn malformed_or_unknown_times_are_absent() {
        for s in [
            "",
            "2026-03-19",
            "    :  :     :  :  ",
            "0000:00:00 00:00:00",
            "2026-13-01T00:00:00",
            "2026-03-19T24:00:00",
            "2026/03/19 00:03:09",
            "2026-03-19T00:03:09+08:00",
            "2026-03-19T00:03:09.",
            "19/03/26",
        ] {
            assert_eq!(civil_ms(s), None, "{s:?}");
        }
    }

    #[test]
    fn offsets_and_subseconds() {
        assert_eq!(offset_ms("+08:00"), Some(8 * 3_600_000));
        assert_eq!(offset_ms("-05:30"), Some(-(5 * 60 + 30) * 60_000));
        assert_eq!(offset_ms("   :  "), None);
        assert_eq!(offset_ms("+0800"), None);
        assert_eq!(offset_ms("+15:00"), None);
        assert_eq!(subsec_ms("5"), Some(500));
        assert_eq!(subsec_ms("123456"), Some(123));
        assert_eq!(subsec_ms("x"), None);
        assert_eq!(mid_exposure(Some(1000), Some(600.0)), Some(301_000));
        assert_eq!(mid_exposure(Some(1000), None), Some(1000));
        assert_eq!(mid_exposure(None, Some(1.0)), None);
    }
}
