// OCR text → values. The terminal font confuses a few letters with digits, so
// numeric cells map them back before parsing.

use crate::model::CARGO_SIZES;

/// Letters this font's digits get misread as, in numeric cells only.
fn digitise(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'O' | 'o' | 'D' | 'Q' => '0',
            'I' | 'l' | 'L' | '|' | 'i' => '1',
            'S' | 's' => '5',
            'B' => '8',
            'Z' => '2',
            _ => c,
        })
        .collect()
}

fn multiplier(s: &str) -> (&str, f64) {
    match s.chars().last() {
        Some('K' | 'k') => (&s[..s.len() - 1], 1e3),
        Some('M') => (&s[..s.len() - 1], 1e6),
        _ => (s, 1.0),
    }
}

fn strip_units(s: &str) -> String {
    let up = s.to_uppercase();
    let mut t = up.replace("AUEC", "").replace("/SCU", "").replace("SCU", "").replace("UEC", "");
    t.retain(|c| !c.is_whitespace() && c != '/');
    t
}

/// Characters OCR may turn into a digit (see `digitise`); anything else before
/// the first digit is the currency glyph (¤) or noise.
const DIGIT_LIKE: &str = "0123456789OoDQIlL|iSsBZ";

/// Parses "10,138", "1.234.567", "3602.29", "3,602.29", "10.14K", "¤135,876/SCU".
/// Returns the value and whether it was read cleanly (no K/M, no repair).
pub fn parse_number(text: &str) -> Option<(f64, bool)> {
    let t = strip_units(text);
    let t = t.trim_start_matches(|c: char| !DIGIT_LIKE.contains(c));
    let (body, mul) = multiplier(t);
    let mut body = digitise(body);
    let mut clean = true;
    // "8135,876": a thousands group can't be 4 digits long, so the first one is
    // the ¤ glyph read as a digit.
    if let Some(i) = body.find([',', '.']) {
        let next_group = body[i + 1..].split([',', '.']).next().unwrap_or("");
        if i == 4 && next_group.len() == 3 && body[..i].chars().all(|c| c.is_ascii_digit()) {
            body.remove(0);
            clean = false;
        }
    }
    if body.is_empty() || !body.chars().all(|c| c.is_ascii_digit() || c == ',' || c == '.') {
        return None;
    }
    if !body.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    // The last separator is a decimal point if 1–2 digits follow it; every other
    // separator groups thousands.
    let last_sep = body.rfind([',', '.']);
    let (int_part, frac_part) = match last_sep {
        Some(i) if (1..=2).contains(&(body.len() - i - 1)) => (&body[..i], &body[i + 1..]),
        Some(i) if mul != 1.0 && body.len() - i - 1 == 3 && body[..i].chars().all(|c| c.is_ascii_digit()) => {
            // "10.138K": a decimal before a multiplier.
            (&body[..i], &body[i + 1..])
        }
        _ => (body.as_str(), ""),
    };
    // Thousands groups after the first must be 3 digits; "10,2392" is a misread.
    if int_part.split([',', '.']).skip(1).any(|g| g.len() != 3) {
        clean = false;
    }
    let int_digits: String = int_part.chars().filter(|c| c.is_ascii_digit()).collect();
    let n: f64 = format!("{}.{}", if int_digits.is_empty() { "0" } else { &int_digits }, if frac_part.is_empty() { "0" } else { frac_part })
        .parse()
        .ok()?;
    Some((n * mul, clean && mul == 1.0))
}

pub fn parse_price(text: &str) -> Option<(i64, bool)> {
    parse_number(text).map(|(n, exact)| (n.round() as i64, exact))
}

pub fn parse_quantity(text: &str) -> Option<(f64, bool)> {
    parse_number(text).map(|(n, exact)| ((n * 100.0).round() / 100.0, exact))
}

pub fn parse_sizes(text: &str) -> Vec<u32> {
    let mut out: Vec<u32> = digitise(&text.to_uppercase().replace("SCU", " "))
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|t| t.parse::<u32>().ok())
        .filter(|n| CARGO_SIZES.contains(n))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(parse_price("10,138"), Some((10138, true)));
        assert_eq!(parse_price("10,138 aUEC"), Some((10138, true)));
        assert_eq!(parse_price("1.234.567"), Some((1234567, true)));
        assert_eq!(parse_price("7O47"), Some((7047, true)));
        assert_eq!(parse_price("10.14K"), Some((10140, false)));
        assert_eq!(parse_price("10.138K"), Some((10138, false)));
        assert_eq!(parse_quantity("3602.29"), Some((3602.29, true)));
        assert_eq!(parse_quantity("3,602.29 SCU"), Some((3602.29, true)));
        assert_eq!(parse_quantity("1,924"), Some((1924.0, true)));
        assert_eq!(parse_price("¤135,876/SCU"), Some((135876, true)));
        assert_eq!(parse_price("a10,232/SCU"), Some((10232, true)));
        assert_eq!(parse_price("8135,876/SCU"), Some((135876, false)));
        assert_eq!(parse_price("10,2392").map(|p| p.1), Some(false));
        assert_eq!(parse_price("aUEC"), None);
        assert_eq!(parse_price("AGRICIUM"), None);
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_sizes("1 2 4 8"), vec![1, 2, 4, 8]);
        assert_eq!(parse_sizes("1SCU 32SCU 3"), vec![1, 32]);
        assert_eq!(parse_sizes("I6 24"), vec![16, 24]);
    }
}
