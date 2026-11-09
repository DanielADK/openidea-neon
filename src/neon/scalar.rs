//! What scalars mean (NEON specification, values §2–§6): literals, quoted
//! strings and the canonical keys duplicates are checked with.

use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiteralKind {
    Null,
    Bool,
    Int,
    Float,
    Date,
    /// Anything else is a string equal to its text.
    Text,
}

/// The kind of a literal that is not a key.
pub fn classify(text: &str) -> LiteralKind {
    match text {
        "null" | "Null" | "NULL" => return LiteralKind::Null,
        "true" | "True" | "TRUE" | "yes" | "Yes" | "YES" | "false" | "False" | "FALSE" | "no" | "No" | "NO" => {
            return LiteralKind::Bool;
        }
        _ => {}
    }
    match number(text) {
        Some(Number::Int | Number::Based(_)) => LiteralKind::Int,
        Some(Number::Float) => LiteralKind::Float,
        None if is_date(text) => LiteralKind::Date,
        None => LiteralKind::Text,
    }
}

enum Number {
    Int,
    Float,
    /// `0x…`, `0o…`, `0b…`: the radix.
    Based(u32),
}

fn number(text: &str) -> Option<Number> {
    let b = text.as_bytes();
    if b.len() > 2 && b[0] == b'0' {
        let (radix, ok): (u32, fn(&u8) -> bool) = match b[1] {
            b'x' => (16, u8::is_ascii_hexdigit),
            b'o' => (8, |c| (b'0'..=b'7').contains(c)),
            b'b' => (2, |c| *c == b'0' || *c == b'1'),
            _ => (0, |_| false),
        };
        if radix > 0 {
            return b[2..].iter().all(ok).then_some(Number::Based(radix));
        }
    }
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let digits = |i: &mut usize| {
        let s = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - s
    };
    let int = digits(&mut i);
    let mut float = false;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let frac = digits(&mut i);
        if int == 0 && frac == 0 {
            return None;
        }
        float = true;
    } else if int == 0 {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return None;
        }
        float = true;
    }
    (i == b.len()).then_some(if float { Number::Float } else { Number::Int })
}

/// `YYYY-M-D`, optionally with a time and a zone, that makes a valid date (§6).
fn is_date(text: &str) -> bool {
    let b = text.as_bytes();
    let mut i = 0;
    let num = |i: &mut usize, min: usize, max: usize| -> Option<u32> {
        let s = *i;
        while *i < b.len() && *i - s < max && b[*i].is_ascii_digit() {
            *i += 1;
        }
        if *i - s < min {
            return None;
        }
        text[s..*i].parse().ok()
    };
    let Some(year) = num(&mut i, 4, 4) else { return false };
    if b.get(i) != Some(&b'-') {
        return false;
    }
    i += 1;
    let Some(month) = num(&mut i, 1, 2) else { return false };
    if b.get(i) != Some(&b'-') {
        return false;
    }
    i += 1;
    let Some(day) = num(&mut i, 1, 2) else { return false };
    if !(1..=12).contains(&month) || day == 0 || day > days_in(month, year) {
        return false;
    }
    if i == b.len() {
        return true;
    }
    match b[i] {
        b'T' | b't' => i += 1,
        b' ' => {
            while b.get(i) == Some(&b' ') {
                i += 1;
            }
        }
        _ => return false,
    }
    let Some(hour) = num(&mut i, 1, 2) else { return false };
    if b.get(i) != Some(&b':') {
        return false;
    }
    i += 1;
    let Some(minute) = num(&mut i, 2, 2) else { return false };
    if b.get(i) != Some(&b':') {
        return false;
    }
    i += 1;
    let Some(second) = num(&mut i, 2, 2) else { return false };
    if hour > 23 || minute > 59 || second > 59 {
        return false;
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    while b.get(i) == Some(&b' ') {
        i += 1;
    }
    match b.get(i) {
        None => true,
        Some(b'Z') => i + 1 == b.len(),
        Some(b'+' | b'-') => zone(&text[i + 1..]).is_some_and(|(h, m)| m <= 59 && h * 60 + m <= 14 * 60),
        _ => false,
    }
}

/// The hours and minutes of an offset after its sign, `\d\d?(:?\d\d)?`,
/// split as a regular expression does: two digits of hours first (`+1000`),
/// then one (`+100` is 1:00).
fn zone(s: &str) -> Option<(u32, u32)> {
    let number = |s: &str| s.bytes().all(|c| c.is_ascii_digit()).then(|| s.parse().ok()).flatten();
    [2, 1].into_iter().find_map(|hours| {
        let h = number(s.get(..hours)?)?;
        let rest = s.get(hours..)?;
        if rest.is_empty() {
            return Some((h, 0));
        }
        let m = rest.strip_prefix(':').unwrap_or(rest);
        number(m).filter(|_| m.len() == 2).map(|m| (h, m))
    })
}

fn days_in(month: u32, year: u32) -> u32 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The canonical key of a literal written as a key (§4).
pub fn literal_key(text: &str) -> Cow<'_, str> {
    match number(text) {
        None => Cow::Borrowed(text),
        Some(Number::Int) => Cow::Owned(canonical_int(text)),
        Some(Number::Based(radix)) => match to_decimal(&text[2..], radix) {
            Some(d) => Cow::Owned(d),
            None => Cow::Borrowed(text),
        },
        Some(Number::Float) => Cow::Owned(float_key(text.parse().unwrap_or(f64::NAN))),
    }
}

fn canonical_int(text: &str) -> String {
    let negative = text.starts_with('-');
    let digits = text.trim_start_matches(['+', '-']).trim_start_matches('0');
    match (negative, digits.is_empty()) {
        (_, true) => "0".into(),
        (true, false) => format!("-{digits}"),
        (false, false) => digits.into(),
    }
}

/// Digits of `radix` as decimal digits, of any length (long ones are left alone).
fn to_decimal(digits: &str, radix: u32) -> Option<String> {
    if digits.len() > 256 {
        return None;
    }
    let mut decimal: Vec<u8> = vec![0]; // little-endian
    for c in digits.chars() {
        let mut carry = c.to_digit(radix)?;
        for d in decimal.iter_mut() {
            carry += u32::from(*d) * radix;
            *d = (carry % 10) as u8;
            carry /= 10;
        }
        while carry > 0 {
            decimal.push((carry % 10) as u8);
            carry /= 10;
        }
    }
    while decimal.len() > 1 && decimal.last() == Some(&0) {
        decimal.pop();
    }
    Some(decimal.iter().rev().map(|d| char::from(b'0' + d)).collect())
}

/// A float as PHP converts it to a string: 14 significant digits.
fn float_key(f: f64) -> String {
    if f.is_nan() {
        return "NAN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "INF".into() } else { "-INF".into() };
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let sci = format!("{f:.13e}");
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let e: i32 = exponent.parse().unwrap_or(0);
    if (-4..14).contains(&e) {
        let digits = format!("{f:.*}", (13 - e).max(0) as usize);
        if digits.contains('.') { digits.trim_end_matches('0').trim_end_matches('.').to_owned() } else { digits }
    } else {
        let m = mantissa.trim_end_matches('0').trim_end_matches('.');
        let m = if m.contains('.') { m.to_owned() } else { format!("{m}.0") };
        format!("{m}E{}{}", if e < 0 { '-' } else { '+' }, e.abs())
    }
}

/// A float key nette/neon 3 and 4.0.1 may canonicalize differently. 3.x
/// converts it to a string as PHP does, 14 significant digits rounded half to
/// even ([`literal_key`] does the same); 4.0.1 rounds its shortest form half
/// away from zero (`number_format()`), and on an exact tie PHP's conversion
/// keeps a trailing zero (`4.2115363145440E+14`). They can differ only when
/// the shortest form is a tie: 15 significant digits ending in 5.
pub fn is_float_tie(text: &str) -> bool {
    if !matches!(number(text), Some(Number::Float)) {
        return false;
    }
    let f: f64 = text.parse().unwrap_or(f64::NAN);
    if !f.is_finite() || f == 0.0 {
        return false;
    }
    let shortest = format!("{:e}", f.abs());
    let mantissa = shortest.split_once('e').map_or(shortest.as_str(), |(m, _)| m);
    mantissa.ends_with('5') && mantissa.bytes().filter(u8::is_ascii_digit).count() == 15
}

/// An integer key: what PHP turns into an integer array key.
pub fn integer_key(canonical: &str) -> Option<i64> {
    let b = canonical.as_bytes();
    let digits = b.strip_prefix(b"-").unwrap_or(b);
    let plain = !digits.is_empty()
        && digits.iter().all(u8::is_ascii_digit)
        && (digits[0] != b'0' || (digits.len() == 1 && digits.len() == b.len()));
    if plain { canonical.parse().ok() } else { None }
}

/// An escape sequence a double-quoted string may not hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EscapeError {
    pub message: String,
    /// It is `\xHH` and the string is otherwise fine: nette/neon 3.3 reads
    /// it (deprecated), 3.4 and later refuse it.
    pub hex: bool,
}

/// The value of a string token (§3); escapes are checked in `"…"` strings.
pub fn string_value(raw: &str) -> Result<Cow<'_, str>, EscapeError> {
    if raw.len() < 2 {
        return Ok(Cow::Borrowed(""));
    }
    let double = raw.starts_with('"');
    let body: Cow<str> =
        if raw.contains('\r') { Cow::Owned(body_of(&raw.replace('\r', "")).into_owned()) } else { body_of(raw) };
    if !double || !body.contains('\\') {
        return Ok(body);
    }
    unescape(&body).map(Cow::Owned)
}

/// The text between the quotes: `''` unescaped, a multiline string's indentation removed.
fn body_of(text: &str) -> Cow<'_, str> {
    if text.len() >= 7 && (text.starts_with("'''\n") || text.starts_with("\"\"\"\n")) {
        return Cow::Owned(multiline_body(text));
    }
    let inner = text.get(1..text.len() - 1).unwrap_or("");
    if text.starts_with('\'') && inner.contains("''") {
        Cow::Owned(inner.replace("''", "'"))
    } else {
        Cow::Borrowed(inner)
    }
}

fn multiline_body(text: &str) -> String {
    let content = &text[3..text.len() - 3];
    let after = content.trim_start_matches('\n');
    let indent_len = after.bytes().take_while(|&c| c == b' ' || c == b'\t').count();
    let indent = &after[..indent_len];
    let mut res = if indent.is_empty() { content.to_owned() } else { content.replace(&format!("\n{indent}"), "\n") };
    if res.starts_with('\n') {
        res.remove(0);
    }
    if let Some(nl) = res.rfind('\n')
        && res[nl + 1..].bytes().all(|c| c == b' ' || c == b'\t')
    {
        res.truncate(nl);
    }
    res
}

/// `\uXXXX` (either case of `u` and the digits) at the start of `s`.
fn unicode_escape(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.len() < 6 || b[0] != b'\\' || (b[1] | 0x20) != b'u' || !b[2..6].iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    u32::from_str_radix(&s[2..6], 16).ok()
}

/// Checks the escape sequences of a string token without building its value.
pub fn check_string(raw: &str) -> Result<(), EscapeError> {
    if !raw.starts_with('"') || !raw.contains('\\') {
        return Ok(());
    }
    let body: Cow<str> = if raw.contains('\r') || raw.starts_with("\"\"\"") {
        body_of(&raw.replace('\r', "")).into_owned().into()
    } else {
        Cow::Borrowed(raw.get(1..raw.len() - 1).unwrap_or(""))
    };
    match escapes(&body, None, false) {
        Err(mut e) if e.hex => {
            e.hex = escapes(&body, None, true).is_ok();
            Err(e)
        }
        r => r,
    }
}

fn unescape(s: &str) -> Result<String, EscapeError> {
    let mut out = String::with_capacity(s.len());
    escapes(s, Some(&mut out), false)?;
    Ok(out)
}

/// Escape sequences, tried as nette/neon does: a surrogate pair, `\uXXXX`,
/// then one character. The value goes to `out`, if given. With `hex`,
/// `\xHH` passes as nette/neon 3.3 lets it.
fn escapes(s: &str, mut out: Option<&mut String>, hex: bool) -> Result<(), EscapeError> {
    let invalid = |seq: &str| EscapeError { message: format!("Invalid escaping sequence {seq}"), hex: false };
    let utf8 = |seq: &str| EscapeError { message: format!("Invalid UTF-8 sequence {seq}"), hex: false };
    let mut rest = s;
    while let Some(k) = rest.find('\\') {
        push_str(&mut out, &rest[..k]);
        rest = &rest[k..];
        let high = unicode_escape(rest);
        let low = rest.get(6..).and_then(unicode_escape);
        if let (Some(h), Some(l)) = (high, low)
            && (0xd800..0xdc00).contains(&h)
            && (0xdc00..0xe000).contains(&l)
        {
            let seq = &rest[..12];
            if seq.as_bytes()[1] != b'u' {
                return Err(invalid(seq));
            }
            if seq.as_bytes()[7] != b'u' {
                return Err(utf8(seq));
            }
            push(&mut out, char::from_u32(0x10000 + ((h - 0xd800) << 10) + (l - 0xdc00)).unwrap_or('\u{fffd}'));
            rest = &rest[12..];
            continue;
        }
        if let Some(code) = high {
            let seq = &rest[..6];
            if seq.as_bytes()[1] != b'u' {
                return Err(invalid(seq));
            }
            match char::from_u32(code) {
                Some(c) => push(&mut out, c),
                None => return Err(utf8(seq)),
            }
            rest = &rest[6..];
            continue;
        }
        let Some(c) = rest[1..].chars().next() else {
            push(&mut out, '\\');
            rest = "";
            break;
        };
        let value = match c {
            // A backslash at the end of a line stands for itself.
            '\n' => {
                push(&mut out, '\\');
                rest = &rest[1..];
                continue;
            }
            't' => '\t',
            'n' => '\n',
            'r' => '\r',
            'f' => '\u{c}',
            'b' => '\u{8}',
            '"' => '"',
            '\\' => '\\',
            '/' => '/',
            '_' => '\u{a0}',
            'x' if rest.len() >= 4 && rest.as_bytes()[2..4].iter().all(u8::is_ascii_hexdigit) => {
                if !hex {
                    return Err(EscapeError { hex: true, ..invalid(&rest[..2]) });
                }
                rest = &rest[4..];
                continue;
            }
            _ => return Err(invalid(&rest[..1 + c.len_utf8()])),
        };
        push(&mut out, value);
        rest = &rest[1 + c.len_utf8()..];
    }
    push_str(&mut out, rest);
    Ok(())
}

fn push(out: &mut Option<&mut String>, c: char) {
    if let Some(out) = out {
        out.push(c);
    }
}

fn push_str(out: &mut Option<&mut String>, s: &str) {
    if let Some(out) = out {
        out.push_str(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_by_kind() {
        use LiteralKind::*;
        for (text, kind) in [
            ("null", Null),
            ("NULL", Null),
            ("Yes", Bool),
            ("off", Text),
            ("on", Text),
            ("0777", Int),
            ("-0", Int),
            ("+1", Int),
            ("0xff", Int),
            ("0o777", Int),
            ("0b0101", Int),
            ("0X10", Text),
            ("-0x1", Text),
            ("1_000", Text),
            (".1", Float),
            ("1.", Float),
            ("1e3", Float),
            ("1e+-1", Text),
            ("1e", Text),
            (".", Text),
            ("2016-06-03", Date),
            ("2016-06-03 19:00:00 +02:00", Date),
            ("2016-06-03T19:00:00Z", Date),
            ("2016-06-03 19:00:00.1234", Date),
            // An offset of three digits is one of hours, then two of minutes.
            ("2020-01-01 10:00:00 +100", Date),
            ("2016-06-03 19:00:00 -053", Date),
            ("2016-06-03 19:00:00 +130", Date),
            ("2016-06-03 19:00:00 +1000", Date),
            ("2016-06-03 19:00:00 +1:30", Date),
            ("2016-06-03 19:00:00 +160", Text),
            ("2016-06-03 19:00:00 +1500", Text),
            ("2016-06-03 19:00:00 +10000", Text),
            ("2016-06-03 19:00:00 +12:", Text),
            ("2024-02-29", Date),
            ("2023-02-29", Text),
            ("2024-01-01 24:00:00", Text),
            ("42 px", Text),
        ] {
            assert_eq!(classify(text), kind, "{text}");
        }
    }

    #[test]
    fn canonical_keys() {
        for (text, key) in [
            ("a", "a"),
            ("007", "7"),
            ("-0", "0"),
            ("+1", "1"),
            ("0x10", "16"),
            ("0b11", "3"),
            ("5.3", "5.3"),
            ("1.0", "1"),
            ("1e3", "1000"),
            ("123456789012345.6", "1.2345678901235E+14"),
            ("1.5e-9", "1.5E-9"),
            ("1e20", "1.0E+20"),
            ("-0.0", "-0"),
            ("1e999", "INF"),
            ("0x12341234123412344564654657845465", "24196472569643293506997087088694023269"),
        ] {
            assert_eq!(literal_key(text), key, "{text}");
        }
        // Ties at 14 significant digits: nette/neon 3 and 4.0.1 differ.
        for (text, tie) in [
            ("12345678901234.5", true),
            ("1234567890123.45", true),
            ("1.00000000000005", true),
            ("-0.000123456789012345", true),
            ("12345678901234.6", false),
            ("1.0000000000000449", false),
            ("2.5", false),
            ("421153631454405", false),
            ("4.21153631454405e14", true),
            ("12345678901234", false),
        ] {
            assert_eq!(is_float_tie(text), tie, "{text}");
        }
        assert_eq!(integer_key("5"), Some(5));
        assert_eq!(integer_key("-5"), Some(-5));
        assert_eq!(integer_key("007"), None);
        assert_eq!(integer_key("-0"), None);
        assert_eq!(integer_key("99999999999999999999"), None);
    }

    #[test]
    fn strings_and_escapes() {
        assert_eq!(string_value("'fo''o'").unwrap(), "fo'o");
        assert_eq!(string_value("'single \\n quote'").unwrap(), "single \\n quote");
        assert_eq!(string_value(r#""\b\f\n\r\t\"\/\\""#).unwrap(), "\u{8}\u{c}\n\r\t\"/\\");
        assert_eq!(string_value(r#""\u011B \_""#).unwrap(), "\u{11b} \u{a0}");
        assert_eq!(string_value(r#""\uD801\uDC01""#).unwrap(), "\u{10401}");
        assert_eq!(string_value(r#""\a""#).unwrap_err().message, "Invalid escaping sequence \\a");
        assert_eq!(string_value(r#""\u202 x""#).unwrap_err().message, "Invalid escaping sequence \\u");
        assert_eq!(string_value(r#""\U0041""#).unwrap_err().message, "Invalid escaping sequence \\U0041");
        assert_eq!(string_value(r#""\uD801""#).unwrap_err().message, "Invalid UTF-8 sequence \\uD801");
        assert_eq!(string_value(r#""\ž""#).unwrap_err().message, "Invalid escaping sequence \\ž");
        // `\xHH`: nette/neon 3.3 reads it, when the rest of the string is fine.
        let hex = |raw: &str| check_string(raw).map_err(|e| (e.message, e.hex));
        assert_eq!(hex(r#""a\x41b""#), Err(("Invalid escaping sequence \\x".into(), true)));
        assert_eq!(hex(r#""\x41é\xfF""#), Err(("Invalid escaping sequence \\x".into(), true)));
        assert_eq!(hex(r#""\x41\q""#), Err(("Invalid escaping sequence \\x".into(), false)));
        assert_eq!(hex(r#""\x4""#), Err(("Invalid escaping sequence \\x".into(), false)));
        assert_eq!(hex(r#""\X41""#), Err(("Invalid escaping sequence \\X".into(), false)));
        assert_eq!(hex(r"'\x41'"), Ok(()));
    }

    #[test]
    fn multiline_strings_lose_the_first_lines_indentation() {
        assert_eq!(string_value("'''\n\tfirst\n\t\tsecond\n\tthird\n\t'''").unwrap(), "first\n\tsecond\nthird");
        assert_eq!(string_value("\"\"\"\r\n  a \\u00A9\r\n  \"\"\"").unwrap(), "a \u{a9}");
        assert_eq!(string_value("'''\n'''").unwrap(), "");
        assert_eq!(string_value("\"\"\"\n\tline \\\n\tnext\n\"\"\"").unwrap(), "line \\\nnext");
    }
}
