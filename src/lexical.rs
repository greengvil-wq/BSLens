//! Conservative authored-input lexical subset, not BSL semantic validation.
use std::collections::BTreeMap;
pub const MAX_BYTES: usize = 1024 * 1024;
pub const MAX_LINES: usize = 65536;
pub const MAX_TOKENS: usize = 100000;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Limit,
    Utf8,
    UnsupportedCharacter,
    InvalidControl,
    ReservedName,
    MultilineString,
    DirectiveUnsupported,
    AnnotationUnsupported,
    AsyncUnsupported,
    UnsupportedHeader,
    AmbiguousBoundary,
    Delimiters,
    MissingEnd,
    UnmatchedEnd,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub reason: Reason,
    pub line: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Boundary {
    pub name: String,
    pub kind: &'static str,
    pub start_line: usize,
    pub start_byte: usize,
    pub header_start_byte: usize,
    pub header_end_byte: usize,
    pub header_end_line: usize,
    pub end_line: usize,
    pub end_byte: usize,
    pub occurrences: usize,
    pub duplicate: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analysis {
    pub boundaries: Vec<Boundary>,
    pub completeness: &'static str,
}
#[derive(Debug)]
struct Token {
    text: String,
    start: usize,
    end: usize,
    identifier: bool,
}
fn fail<T>(reason: Reason, line: usize) -> Result<T, Refusal> {
    Err(Refusal { reason, line })
}
fn letter(c: char) -> bool {
    c.is_ascii_alphabetic() || ('А'..='я').contains(&c) || matches!(c, 'Ё' | 'ё' | '_')
}
fn start(s: &str) -> Option<&'static str> {
    match s {
        "procedure" | "процедура" => Some("procedure"),
        "function" | "функция" => Some("function"),
        _ => None,
    }
}
fn end(s: &str) -> Option<&'static str> {
    match s {
        "endprocedure" | "конецпроцедуры" => Some("procedure"),
        "endfunction" | "конецфункции" => Some("function"),
        _ => None,
    }
}
fn asynchronous(s: &str) -> bool {
    matches!(s, "async" | "асинх")
}
fn export(s: &str) -> bool {
    matches!(s, "export" | "экспорт")
}
// Conservative explicit deny-list, not a complete platform keyword inventory.
fn reserved_name(s: &str) -> bool {
    matches!(
        s,
        "if" | "then"
            | "elsif"
            | "else"
            | "endif"
            | "for"
            | "each"
            | "in"
            | "to"
            | "do"
            | "enddo"
            | "while"
            | "break"
            | "continue"
            | "return"
            | "var"
            | "try"
            | "except"
            | "endtry"
            | "raise"
            | "new"
            | "execute"
            | "true"
            | "false"
            | "undefined"
            | "null"
            | "and"
            | "or"
            | "not"
            | "если"
            | "тогда"
            | "иначеесли"
            | "иначе"
            | "конецесли"
            | "для"
            | "каждого"
            | "из"
            | "по"
            | "цикл"
            | "конеццикла"
            | "пока"
            | "прервать"
            | "продолжить"
            | "возврат"
            | "перем"
            | "попытка"
            | "исключение"
            | "конецпопытки"
            | "вызватьисключение"
            | "новый"
            | "выполнить"
            | "истина"
            | "ложь"
            | "неопределено"
            | "и"
            | "или"
            | "не"
    )
}
fn tokens(text: &str, base: usize, line: usize, count: &mut usize) -> Result<Vec<Token>, Refusal> {
    let mut out = vec![];
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if matches!(c, ' ' | '\t') {
            continue;
        }
        if c == '/' && chars.peek().is_some_and(|(_, c)| *c == '/') {
            break;
        }
        if c == '#' {
            return fail(Reason::DirectiveUnsupported, line);
        }
        if c == '&' {
            return fail(Reason::AnnotationUnsupported, line);
        }
        let (value, finish, id) = if c == '"' {
            let mut finish = None;
            while let Some((j, d)) = chars.next() {
                if d == '"' {
                    if chars.peek().is_some_and(|(_, v)| *v == '"') {
                        chars.next();
                    } else {
                        finish = Some(j + 1);
                        break;
                    }
                }
            }
            let finish = finish.ok_or(Refusal {
                reason: Reason::MultilineString,
                line,
            })?;
            ("@string".into(), finish, false)
        } else if letter(c) {
            let mut finish = i + c.len_utf8();
            while let Some((j, d)) = chars.peek().copied() {
                if letter(d) || d.is_ascii_digit() {
                    chars.next();
                    finish = j + d.len_utf8();
                } else {
                    break;
                }
            }
            (text[i..finish].to_lowercase(), finish, true)
        } else if c.is_ascii_digit() || "()[]+-*/%=<>.,;:?!".contains(c) {
            (c.to_string(), i + c.len_utf8(), false)
        } else {
            return fail(Reason::UnsupportedCharacter, line);
        };
        *count += 1;
        if *count > MAX_TOKENS {
            return fail(Reason::Limit, line);
        }
        out.push(Token {
            text: value,
            start: base + i,
            end: base + finish,
            identifier: id,
        });
    }
    Ok(out)
}
/// All-or-nothing lexical result. Any unsupported state discards all candidates.
/// Empty boundaries never prove absence of methods outside this declared subset.
pub fn analyze(raw: &[u8]) -> Result<Analysis, Refusal> {
    if raw.len() > MAX_BYTES {
        return fail(Reason::Limit, 1);
    }
    let text = std::str::from_utf8(raw).map_err(|_| Refusal {
        reason: Reason::Utf8,
        line: 1,
    })?;
    // Check the complete decoded buffer before comments/literals can mask bytes.
    let mut control_line = 1;
    for (index, c) in text.char_indices() {
        if (c == '\r' && raw.get(index + 1) != Some(&b'\n'))
            || (c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
        {
            return fail(Reason::InvalidControl, control_line);
        }
        if c == '\n' {
            control_line += 1;
        }
    }
    let mut current: Option<Boundary> = None;
    let mut found = vec![];
    let mut stack = vec![];
    let mut count = 0;
    let mut offset = 0;
    let mut last_line = 1;
    for (i, physical) in text.split_inclusive('\n').enumerate() {
        let line = i + 1;
        last_line = line;
        if line > MAX_LINES {
            return fail(Reason::Limit, line);
        }
        let mut body = physical.strip_suffix('\n').unwrap_or(physical);
        body = body.strip_suffix('\r').unwrap_or(body);
        let mut base = offset;
        offset += physical.len();
        if line == 1 {
            if let Some(s) = body.strip_prefix('\u{feff}') {
                body = s;
                base += 3;
            }
        }
        let row = tokens(body, base, line, &mut count)?;
        if row.is_empty() {
            continue;
        }
        if row.iter().any(|t| asynchronous(&t.text)) {
            return fail(Reason::AsyncUnsupported, line);
        }
        if let Some(kind) = start(&row[0].text) {
            if current.is_some() || !stack.is_empty() {
                return fail(Reason::AmbiguousBoundary, line);
            }
            if !(row.len() == 4 || row.len() == 5 && export(&row[4].text))
                || !row[1].identifier
                || start(&row[1].text).is_some()
                || end(&row[1].text).is_some()
                || export(&row[1].text)
                || row[2].text != "("
                || row[3].text != ")"
            {
                return fail(Reason::UnsupportedHeader, line);
            }
            if reserved_name(&row[1].text) {
                return fail(Reason::ReservedName, line);
            }
            current = Some(Boundary {
                name: row[1].text.clone(),
                kind,
                start_line: line,
                start_byte: row[0].start,
                header_start_byte: row[0].start,
                header_end_byte: row.last().unwrap().end,
                header_end_line: line,
                end_line: 0,
                end_byte: 0,
                occurrences: 0,
                duplicate: false,
            });
            continue;
        }
        if let Some(kind) = end(&row[0].text) {
            if row.len() != 1
                || !stack.is_empty()
                || current.as_ref().is_none_or(|b| b.kind != kind)
            {
                return fail(Reason::UnmatchedEnd, line);
            }
            let mut b = current.take().unwrap();
            b.end_line = line;
            b.end_byte = row[0].end;
            found.push(b);
            continue;
        }
        if row
            .iter()
            .any(|t| start(&t.text).is_some() || end(&t.text).is_some())
        {
            return fail(Reason::AmbiguousBoundary, line);
        }
        for t in row {
            match t.text.as_str() {
                "(" => stack.push(")"),
                "[" => stack.push("]"),
                ")" | "]" => {
                    if stack.pop() != Some(t.text.as_str()) {
                        return fail(Reason::Delimiters, line);
                    }
                }
                _ => {}
            }
            if stack.len() > 128 {
                return fail(Reason::Limit, line);
            }
        }
    }
    if let Some(b) = current {
        return fail(Reason::MissingEnd, b.start_line);
    }
    if !stack.is_empty() {
        return fail(Reason::Delimiters, last_line);
    }
    let mut counts = BTreeMap::new();
    for b in &found {
        *counts.entry(b.name.clone()).or_insert(0usize) += 1;
    }
    for b in &mut found {
        b.occurrences = counts[&b.name];
        b.duplicate = b.occurrences > 1;
    }
    Ok(Analysis {
        boundaries: found,
        completeness:
            "lexical zero-argument subset; absence unknown; no body/call/provenance validation",
    })
}
