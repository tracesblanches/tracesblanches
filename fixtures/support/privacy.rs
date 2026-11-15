//! Third-party personal data patterns (Q1, #101): the fixture generator refuses to write,
//! and the fixtures test refuses to accept, any text with an e-mail address or a phone
//! number. Shared by `skimap fixtures` and `fixtures/support/mod.rs`.

/// First e-mail address or phone number in `s` (French, Swiss and Italian formats), if
/// any.
pub fn personal_pattern(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let is_word = |c: u8| c.is_ascii_alphanumeric();
    // E-mail: local@domain.tld.
    for (i, _) in s.match_indices('@') {
        let local = b[..i]
            .iter()
            .rev()
            .take_while(|c| c.is_ascii_alphanumeric() || b"._%+-".contains(c))
            .count();
        let dom = &s[i + 1..];
        let dlen = dom
            .bytes()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == b'.' || *c == b'-')
            .count();
        let d = dom[..dlen].trim_end_matches('.');
        if local > 0
            && d.rsplit_once('.').is_some_and(|(h, tld)| {
                !h.is_empty() && tld.len() >= 2 && tld.bytes().all(|c| c.is_ascii_alphabetic())
            })
        {
            return Some(s[i - local..i + 1 + d.len()].to_owned());
        }
    }
    // Phone: 0X XX XX XX XX (separators space, dot, dash or none), +33 / +41 / +39 or
    // 0033 / 0041 / 0039 and 9 digits, not inside a longer word or number.
    for start in 0..b.len() {
        if start > 0 && (is_word(b[start - 1]) || b[start - 1] == b'.' || b[start - 1] == b'+') {
            continue;
        }
        let (prefix, mut i) = if b[start] == b'+' {
            (true, start + 1)
        } else if b[start] == b'0' {
            (false, start)
        } else {
            continue;
        };
        // Digit groups separated by one space, dot or dash.
        let mut groups: Vec<String> = vec![String::new()];
        while i < b.len() && groups.iter().map(String::len).sum::<usize>() < 14 {
            match b[i] {
                c if c.is_ascii_digit() => groups.last_mut().expect("one").push(char::from(c)),
                b' ' | b'.' | b'-'
                    if i + 1 < b.len()
                        && b[i + 1].is_ascii_digit()
                        && !groups.last().expect("one").is_empty() =>
                {
                    groups.push(String::new())
                }
                _ => break,
            }
            i += 1;
        }
        if i < b.len() && is_word(b[i]) {
            continue;
        }
        let digits: String = groups.concat();
        // A single run, or groups of 1 to 4 digits (no decimal number looks like that:
        // "0.123456789" is a group of 1 then 9).
        let grouped = groups.len() == 1 || groups.iter().all(|g| (1..=4).contains(&g.len()));
        let intl = |d: &str| ["33", "41", "39"].iter().any(|c| d.starts_with(c)) && d.len() == 11;
        let national = !prefix && digits.len() == 10 && digits.as_bytes()[1] != b'0';
        let long = !prefix && digits.starts_with("00") && intl(&digits[2..]);
        if grouped && (national || long || (prefix && intl(&digits))) {
            return Some(s[start..i].to_owned());
        }
    }
    None
}
