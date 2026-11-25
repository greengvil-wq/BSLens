//! Conservative authored-input lexical subset, not BSL semantic validation.
use std::collections::BTreeMap;
mod anomaly;
mod condition;
pub use anomaly::{analyze_navigator_agreement_v1, analyze_source_anomalies_v1, SourceAnomaly};
pub use condition::Branch;
pub const MAX_BYTES: usize = 1024 * 1024;
pub const MAX_LINES: usize = 65536;
pub const MAX_TOKENS: usize = 100000;
pub const LEXICAL_TAIL_LARGE_TOKEN_THRESHOLD_BYTES: usize = 2 * 1024 * 1024;
pub const LEXICAL_TAIL_LARGE_MAX_TOKENS: usize = 200000;
pub const MAX_REGION_DEPTH: usize = 64;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Limit,
    Utf8,
    UnsupportedCharacter,
    InvalidControl,
    ReservedName,
    MultilineString,
    DirectiveUnsupported,
    RegionHeader,
    RegionInsideMethod,
    RegionUnmatchedEnd,
    RegionMissingEnd,
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
    pub branch_path: Vec<Branch>,
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
// A non-boundary lexical witness, not member or call resolution.
fn is_dotted_procedure_member(row: &[Token], index: usize, allow_selector: bool) -> bool {
    let [.., receiver, dot, member] = &row[..=index] else {
        return false;
    };
    if member.text != "процедура" || dot.text != "."
        || receiver.end != dot.start || dot.end != member.start
    {
        return false;
    }
    let plain_suffix = row.get(index + 1).is_none_or(|t| {
            matches!(t.text.as_str(),
                "(" | ")" | "[" | "]" | ";" | "," | "=" | "<" | ">"
                | "+" | "-" | "*" | "/" | "%" | "и" | "или" | "and" | "or")
    });
    let selected_assignment = allow_selector
        && row.get(index + 1..).is_some_and(|suffix| {
            let [next_dot, selector, assignment, rhs, terminal] = suffix else { return false };
            next_dot.text == "." && member.end == next_dot.start && next_dot.end == selector.start
                && variable_name(selector) && assignment.text == "="
                // A separately tokenized terminal separator proves that an
                // opaque quoted RHS closed on this physical line.
                && (variable_name(rhs) || rhs.text == "@string") && terminal.text == ";"
        });
    if !plain_suffix && !selected_assignment {
        return false;
    }
    // Validate only the adjacent receiver witness. The existing delimiter
    // pass still checks any closing paren/bracket against the full module.
    let mut i = index - 2;
    for _ in 0..64 {
        let t = &row[i];
        if matches!(t.text.as_str(), ")" | "]") {
            return true;
        }
        if !t.identifier || reserved_name(&t.text) || start(&t.text).is_some()
            || end(&t.text).is_some() || asynchronous(&t.text) || export(&t.text)
        {
            return false;
        }
        let Some(p) = i.checked_sub(1).map(|n| &row[n]).filter(|p| p.text == ".") else {
            return true;
        };
        if i < 2 || p.end != t.start || row[i - 2].end != p.start {
            return false;
        }
        i -= 2;
    }
    false
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
// Only standalone structural directives; never a general preprocessor.
fn region(text: &str, line: usize) -> Result<Option<bool>, Refusal> {
    let text = text.trim_start_matches([' ', '\t']);
    let Some(text) = text.strip_prefix('#') else {
        return Ok(None);
    };
    if text.starts_with([' ', '\t']) {
        return fail(Reason::DirectiveUnsupported, line);
    }
    let text = text.split_once("//").map_or(text, |(before, _)| before);
    let parts: Vec<_> = text
        .split([' ', '\t'])
        .filter(|p| !p.is_empty())
        .take(3)
        .collect();
    let Some(keyword) = parts.first() else {
        return fail(Reason::DirectiveUnsupported, line);
    };
    match keyword.to_lowercase().as_str() {
        "region" | "область" => {
            if parts.len() != 2 {
                return fail(Reason::RegionHeader, line);
            }
            let mut chars = parts[1].chars();
            if !chars.next().is_some_and(letter) || !chars.all(|c| letter(c) || c.is_ascii_digit())
            {
                return fail(Reason::RegionHeader, line);
            }
            Ok(Some(true))
        }
        "endregion" | "конецобласти" => {
            if parts.len() != 1 {
                return fail(Reason::RegionHeader, line);
            }
            Ok(Some(false))
        }
        _ => fail(Reason::DirectiveUnsupported, line),
    }
}
fn charge_tokens(count: &mut usize, added: usize, cap: usize, line: usize) -> Result<(), Refusal> {
    match count.checked_add(added) {
        Some(next) if next <= cap => {
            *count = next;
            Ok(())
        }
        _ => fail(Reason::Limit, line),
    }
}
fn tokens(
    text: &str,
    base: usize,
    line: usize,
    count: &mut usize,
    token_cap: usize,
    strings: bool,
    dates_members: bool,
    allow_dashed_date: bool,
    opaque: &mut Option<usize>,
) -> Result<Vec<Token>, Refusal> {
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
            let finish = match finish {
                Some(n) => n,
                None if strings => {
                    *opaque = Some(line);
                    text.len()
                }
                None => return fail(Reason::MultilineString, line),
            };
            ("@string".into(), finish, false)
        } else if c == '\'' && dates_members {
            // Previous compact shapes plus exactly ASCII YYYY-MM-DD in this
            // profile. Inspect at most the ten interior bytes for separators.
            let start = i + 1;
            let dashed = allow_dashed_date && text.as_bytes().get(start..start + 10).is_some_and(|b| {
                b.iter().enumerate().all(|(n, x)| {
                    if n == 4 || n == 7 { *x == b'-' } else { x.is_ascii_digit() }
                })
            });
            let content_bytes = if dashed {
                // The ten ASCII bytes were checked; require the quote next.
                for _ in 0..10 {
                    chars.next();
                }
                10
            } else {
                let mut digits = 0usize;
                while chars.peek().is_some_and(|(_, d)| d.is_ascii_digit()) {
                    if digits == 14 {
                        return fail(Reason::UnsupportedCharacter, line);
                    }
                    chars.next();
                    digits += 1;
                }
                if !matches!(digits, 8 | 12 | 14) {
                    return fail(Reason::UnsupportedCharacter, line);
                }
                digits
            };
            let Some((close, '\'')) = chars.next() else {
                return fail(Reason::UnsupportedCharacter, line);
            };
            let joins = |d: char| letter(d) || d.is_ascii_digit() || matches!(d, '\'' | '"');
            if close != start + content_bytes
                || text[..i].chars().next_back().is_some_and(joins)
                || chars.peek().is_some_and(|(_, d)| joins(*d))
            {
                return fail(Reason::UnsupportedCharacter, line);
            }
            ((if dashed { "@date-dashed" } else { "@date" }).into(), close + 1, false)
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
        charge_tokens(count, 1, token_cap, line)?;
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
    analyze_mode(raw, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, anomaly::View::Strict)
}
/// Exact Russian no-argument context annotations; physical adjacency, no semantic inference.
pub fn analyze_context_v1(raw: &[u8]) -> Result<Analysis, Refusal> {
    analyze_mode(raw, true, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, anomaly::View::Strict)
}
/// Single physical line, bounded identifier parameters; no defaults or expressions.
pub fn analyze_parameters_v1(raw: &[u8]) -> Result<Analysis, Refusal> {
    analyze_mode(raw, true, true, false, false, false, false, false, false, false, false, false, false, false, false, false, false, anomaly::View::Strict)
}
/// Physical conditional arms, syntax only; no active-context evaluation.
pub fn analyze_conditionals_v1(raw: &[u8]) -> Result<Analysis, Refusal> {
    analyze_mode(raw, true, true, true, false, false, false, false, false, false, false, false, false, false, false, false, false, anomaly::View::Strict)
}
/// Opaque pipe-continuation strings, without value evaluation or byte rewriting.
pub fn analyze_pipe_strings_v1(raw: &[u8]) -> Result<Analysis, Refusal> {
    analyze_mode(raw, true, true, true, true, false, false, false, false, false, false, false, false, false, false, false, false, anomaly::View::Strict)
}
/// Exact combined RU annotation and bounded scalar defaults on single-line headers.
pub fn analyze_literal_defaults_v1(raw: &[u8]) -> Result<Analysis, Refusal> {
    analyze_mode(raw, true, true, true, true, true, false, false, false, false, false, false, false, false, false, false, false, anomaly::View::Strict)
}

/// Exactly two adjacent physical lines in a plain-parameter header.
pub fn analyze_wrapped_headers_v1(raw: &[u8]) -> Result<Analysis, Refusal> {
    analyze_mode(raw, true, true, true, true, true, true, false, false, false, false, false, false, false, false, false, false, anomaly::View::Strict)
}

pub const MAX_REFINED_HEADER_LINES: usize = 16;

/// Bounded comma-separated headers and two additional physical condition atoms.
pub fn analyze_corpus_refinements_v1(raw: &[u8]) -> Result<Analysis, Refusal> {
    analyze_mode(raw, true, true, true, true, true, true, true, false, false, false, false, false, false, false, false, false, anomaly::View::Strict)
}

pub const MAX_ANNOTATED_VAR_NAMES: usize = 64;

/// Recognize a module variable declaration only in the exact adjacent annotation slot.
pub fn analyze_annotated_vars_v1(raw: &[u8]) -> Result<Analysis, Refusal> {
    analyze_mode(raw, true, true, true, true, true, true, true, true, false, false, false, false, false, false, false, false, anomaly::View::Strict)
}

/// Extended lexical inventory; does not evaluate literals.
pub fn analyze_dates_members_v1(raw: &[u8]) -> Result<Analysis, Refusal> {
    analyze_mode(raw, true, true, true, true, true, true, true, true, true, false, false, false, false, false, false, false, anomaly::View::Strict)
}

/// Latest narrow lexical profile: no scheduling or context semantics.
pub fn analyze_async_casing_v1(raw: &[u8]) -> Result<Analysis, Refusal> {
    analyze_mode(raw, true, true, true, true, true, true, true, true, true, true, false, false, false, false, false, false, anomaly::View::Strict)
}

/// Latest bounded structural profile; variables do not emit method facts.
pub fn analyze_mobile_vars_v1(raw: &[u8]) -> Result<Analysis, Refusal> {
    analyze_mode(raw, true, true, true, true, true, true, true, true, true, true, true, false, false, false, false, false, anomaly::View::Strict)
}

/// Local method regions do not alter structural inventory or branch semantics.
pub fn analyze_method_regions_v1(raw: &[u8]) -> Result<Analysis, Refusal> {
    analyze_mode(raw, true, true, true, true, true, true, true, true, true, true, true, true, false, false, false, false, anomaly::View::Strict)
}

/// Bounded additional header forms without expression evaluation.
pub fn analyze_header_forms_v1(raw: &[u8]) -> Result<Analysis, Refusal> {
    analyze_mode(raw, true, true, true, true, true, true, true, true, true, true, true, true, true, false, false, false, anomaly::View::Strict)
}

/// Explicit Linux pilot source budget with narrowly evidenced lexical forms.
pub fn analyze_source_limits_v1(raw: &[u8]) -> Result<Analysis, Refusal> {
    analyze_mode(raw, true, true, true, true, true, true, true, true, true, true, true, true, true, true, false, false, anomaly::View::Strict)
}

/// Opt-in for long ordinary wrapped declarations; async keeps its old limit.
pub fn analyze_long_headers_v1(raw: &[u8]) -> Result<Analysis, Refusal> {
    analyze_mode(raw, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, false, anomaly::View::Strict)
}

/// Opt-in continuation of the bounded lexical inventory pilot.
pub fn analyze_lexical_tail_v1(raw: &[u8]) -> Result<Analysis, Refusal> {
    analyze_mode(raw, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, anomaly::View::Strict)
}

pub const MAX_LONG_ORDINARY_HEADER_LINES: usize = 20;
pub const MAX_LONG_FORMAL_BYTES: usize = 130;

const LARGE_MODULE_BYTES: usize = 4 * 1024 * 1024;
const MAX_CLIENT_ANNOTATION_COMMENT_LINES: usize = 16;

const MAX_ANNOTATED_VAR_LINES: usize = 5;
const MAX_ANNOTATED_VAR_LIST_BYTES: usize = 8192;

fn variable_name(t: &Token) -> bool {
    t.identifier
        && !reserved_name(&t.text)
        && start(&t.text).is_none()
        && end(&t.text).is_none()
        && !export(&t.text)
        && !asynchronous(&t.text)
        && !matches!(t.text.as_str(), "val" | "знач")
}

/// Validate one physical fragment; punctuation may never start a line.
fn variable_fragment(row: &[Token]) -> Option<bool> {
    if row.len() < 2 || row.len() % 2 != 0 {
        return None;
    }
    let terminal = row.last()?.text.as_str();
    if !matches!(terminal, "," | ";")
        || !row.chunks_exact(2).enumerate().all(|(i, pair)| {
            variable_name(&pair[0])
                && (i == row.len() / 2 - 1 || pair[1].text == ",")
        })
    {
        return None;
    }
    Some(terminal == ";")
}

fn annotated_module_variables(row: &[Token], line: usize) -> Result<bool, Refusal> {
    let mut names = std::collections::BTreeSet::new();
    let mut i = 1;
    loop {
        let Some(t) = row.get(i) else {
            return Ok(false);
        };
        if !variable_name(t) || !names.insert(t.text.as_str()) {
            return Ok(false);
        }
        if names.len() > MAX_ANNOTATED_VAR_NAMES || t.end - t.start > 128 {
            return fail(Reason::Limit, line);
        }
        i += 1;
        match row.get(i).map(|t| t.text.as_str()) {
            Some(",") => i += 1,
            Some(";") => return Ok(i + 1 == row.len()),
            Some("экспорт") if names.len() == 1 => {
                return Ok(i + 2 == row.len() && row[i + 1].text == ";");
            }
            _ => return Ok(false),
        }
    }
}

/// The last visible token is produced by the quote-aware tokenizer. A suffix
/// after it can be horizontal space or a real lexer-stopped line comment.
fn header_line_end(body: &str, base: usize, last: &Token, allow_comment: bool) -> Option<usize> {
    let tail = body.get((last.end - base)..)?.trim_start_matches([' ', '\t']);
    if tail.is_empty() {
        Some(last.end)
    } else if allow_comment && tail.starts_with("//") {
        Some(base + body.trim_end_matches([' ', '\t']).len())
    } else {
        None
    }
}

/// Advance over one supported literal token or a contiguous signed decimal.
fn header_literal_end(row: &[Token], mut i: usize, header_forms: bool) -> Option<usize> {
    let t = row.get(i)?;
    if matches!(t.text.as_str(), "@string" | "@date")
        || (header_forms && t.identifier && t.text == "null")
        || (t.identifier
            && matches!(
                t.text.as_str(),
                "неопределено" | "undefined" | "истина" | "true" | "ложь" | "false"
            ))
    {
        return Some(i + 1);
    }
    let digit = |t: &Token| t.text.len() == 1 && t.text.as_bytes()[0].is_ascii_digit();
    let mut last = if matches!(t.text.as_str(), "+" | "-") {
        i += 1;
        Some(t.end)
    } else {
        None
    };
    let mut digits = 0;
    let mut fraction = false;
    let mut fraction_digits = 0;
    while let Some(t) = row.get(i) {
        if last.is_some_and(|end| t.start != end) {
            break;
        }
        if digit(t) {
            digits += 1;
            if digits > 32 {
                return None;
            }
            if fraction {
                fraction_digits += 1;
            }
        } else if t.text == "." && digits != 0 && !fraction {
            fraction = true;
        } else {
            break;
        }
        last = Some(t.end);
        i += 1;
    }
    (digits != 0 && (!fraction || fraction_digits != 0)).then_some(i)
}

fn parameter_header(
    row: &[Token],
    line: usize,
    preserve_method_name: bool,
    literals: bool,
    modifier_byte: Option<usize>,
    header_forms: bool,
    long_ordinary: bool,
) -> Result<bool, Refusal> {
    let name_bytes = if preserve_method_name && row.len() > 1 {
        row[1].end - row[1].start
    } else {
        0
    };
    // The declaration grammar consumes a view without the modifier, but its
    // lexical budget covers every token and byte from that original prefix.
    let first_byte = modifier_byte.unwrap_or(row[0].start);
    let token_cap = if long_ordinary { MAX_LONG_FORMAL_BYTES } else { 128 };
    if row.len() + usize::from(modifier_byte.is_some()) > 200
        || row.last().unwrap().end - first_byte - name_bytes > 8192
        || row
            .iter()
            .enumerate()
            .any(|(i, t)| !(preserve_method_name && i == 1) && t.end - t.start > token_cap)
    {
        return fail(Reason::Limit, line);
    }
    let mut long_tokens = row.iter().enumerate()
        .filter(|(i, t)| !(preserve_method_name && *i == 1) && t.end - t.start > 128)
        .count();
    if row.len() < 4
        || !row[1].identifier
        || start(&row[1].text).is_some()
        || end(&row[1].text).is_some()
        || export(&row[1].text)
        || row[2].text != "("
    {
        return Ok(false);
    }
    let mut i = 3;
    let mut names = std::collections::BTreeSet::new();
    if row[i].text != ")" {
        loop {
            if row
                .get(i)
                .is_some_and(|t| matches!(t.text.as_str(), "val" | "знач"))
            {
                i += 1;
            }
            let Some(t) = row.get(i) else {
                return Ok(false);
            };
            if !t.identifier
                || reserved_name(&t.text)
                || start(&t.text).is_some()
                || end(&t.text).is_some()
                || export(&t.text)
                || asynchronous(&t.text)
                || matches!(t.text.as_str(), "val" | "знач")
                || !names.insert(t.text.as_str())
            {
                return Ok(false);
            }
            // The sole exception is a grammar-confirmed formal identifier
            // with an explicit scalar default in an extended ordinary header.
            if t.end - t.start > 128 {
                if !long_ordinary || !literals || row.get(i + 1).is_none_or(|t| t.text != "=") {
                    return fail(Reason::Limit, line);
                }
                long_tokens -= 1;
            }
            if names.len() > 64 {
                return fail(Reason::Limit, line);
            }
            i += 1;
            if literals && row.get(i).is_some_and(|t| t.text == "=") {
                let Some(next) = header_literal_end(row, i + 1, header_forms) else {
                    return Ok(false);
                };
                i = next;
            }
            match row.get(i).map(|t| t.text.as_str()) {
                Some(",") => i += 1,
                Some(")") => break,
                _ => return Ok(false),
            }
        }
    }
    i += 1;
    if row.get(i).is_some_and(|t| export(&t.text)) {
        i += 1;
    }
    if literals && row.get(i).is_some_and(|t| t.text == ";") {
        i += 1;
    }
    if long_tokens != 0 {
        return fail(Reason::Limit, line);
    }
    Ok(i == row.len())
}
fn analyze_mode(
    raw: &[u8],
    context: bool,
    parameters: bool,
    conditional: bool,
    strings: bool,
    literals: bool,
    wrapped: bool,
    refined: bool,
    annotated_vars: bool,
    dates_members: bool,
    async_casing: bool,
    mobile_vars: bool,
    method_regions: bool,
    header_forms: bool,
    source_limits: bool,
    long_headers: bool,
    lexical_tail: bool,
    anomaly_view: anomaly::View,
) -> Result<Analysis, Refusal> {
    if raw.len() > if source_limits { LARGE_MODULE_BYTES } else { MAX_BYTES } {
        return fail(Reason::Limit, 1);
    }
    let token_cap = if lexical_tail && raw.len() > LEXICAL_TAIL_LARGE_TOKEN_THRESHOLD_BYTES {
        LEXICAL_TAIL_LARGE_MAX_TOKENS
    } else {
        MAX_TOKENS
    };
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
    let mut opaque: Option<usize> = None;
    let mut branches: Vec<condition::Frame> = vec![];
    let mut blocks = 0usize;
    // Physical line, supports annotated variable, exact bare &НаКлиенте.
    let mut pending_annotation: Option<(usize, bool, bool)> = None;
    let mut current: Option<Boundary> = None;
    let mut found = vec![];
    let mut stack = vec![];
    let mut regions = vec![];
    // A method owns at most one extra region on the existing shared stack.
    // Module-level regions beneath it remain outside that method's scope.
    let mut owned_region: Option<usize> = None;
    let mut count = 0;
    let mut offset = 0;
    let mut last_line = 1;
    let mut physical_lines = text.split_inclusive('\n').enumerate();
    while let Some((i, physical)) = physical_lines.next() {
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
        let resumed = opaque.is_some();
        if resumed {
            let trimmed = body.trim_start_matches([' ', '\t']);
            if trimmed.starts_with("//") {
                continue;
            }
            if body.is_empty()
                && anomaly_view == anomaly::View::EmptyOpaquePhysicalLine(base)
            {
                continue;
            }
            let Some(fragment) = trimmed.strip_prefix('|') else {
                return fail(Reason::MultilineString, line);
            };
            let mut chars = fragment.char_indices().peekable();
            let mut close = None;
            while let Some((i, c)) = chars.next() {
                if c == '"' {
                    if chars.peek().is_some_and(|(_, c)| *c == '"') {
                        chars.next();
                    } else {
                        close = Some(i + 1);
                        break;
                    }
                }
            }
            let Some(close) = close else {
                continue;
            };
            let consumed = body.len() - fragment.len() + close;
            body = &body[consumed..];
            base += consumed;
            opaque = None;
        }
        if source_limits && !resumed && body.trim_start_matches([' ', '\t']).starts_with("//") {
            if let Some((at, _, true)) = pending_annotation {
                if line - at > MAX_CLIENT_ANNOTATION_COMMENT_LINES {
                    return fail(Reason::Limit, line);
                }
                // Only such lines preserve the state; any actual line of
                // code still passes the old next-declaration/scope checks.
                continue;
            }
        }
        if conditional && !resumed {
            if let Some((kind, n)) = condition::directive(body, line, refined, async_casing, mobile_vars)? {
                if !stack.is_empty() || pending_annotation.is_some() {
                    return fail(Reason::DirectiveUnsupported, line);
                }
                charge_tokens(&mut count, n, token_cap, line)?;
                let method = current.as_ref().map(|m: &Boundary| m.start_byte);
                if kind == "if" {
                    blocks += 1;
                    if blocks > 4096 || branches.len() >= 8 {
                        return fail(Reason::Limit, line);
                    }
                    branches.push(condition::Frame {
                        branch: Branch {
                            block_line: line,
                            arm: 0,
                            kind,
                            arm_line: line,
                            directive_start_byte: base,
                            directive_end_byte: base + body.len(),
                        },
                        method,
                        regions: regions.clone(),
                        seen_else: false,
                    });
                } else {
                    let Some(frame) = branches.last_mut() else {
                        return fail(Reason::DirectiveUnsupported, line);
                    };
                    if frame.method != method || frame.regions != regions {
                        return fail(Reason::DirectiveUnsupported, line);
                    }
                    if kind == "end" {
                        branches.pop();
                    } else {
                        if frame.seen_else {
                            return fail(Reason::DirectiveUnsupported, line);
                        }
                        frame.seen_else = kind == "else";
                        frame.branch.arm += 1;
                        if frame.branch.arm >= 64 {
                            return fail(Reason::Limit, line);
                        }
                        frame.branch.kind = kind;
                        frame.branch.arm_line = line;
                        frame.branch.directive_start_byte = base;
                        frame.branch.directive_end_byte = base + body.len();
                    }
                }
                continue;
            }
        }
        if context && !resumed && body.trim_start_matches([' ', '\t']).starts_with('&') {
            if current.is_some()
                || !stack.is_empty()
                || pending_annotation.is_some()
                || !(["&НаКлиенте", "&НаСервере", "&НаСервереБезКонтекста"]
                    .contains(&body.trim_matches([' ', '\t']))
                    || (literals && body.trim_matches([' ', '\t']) == "&НаКлиентеНаСервереБезКонтекста"))
            {
                return fail(Reason::AnnotationUnsupported, line);
            }
            charge_tokens(&mut count, 2, token_cap, line)?;
            pending_annotation = Some((
                line,
                annotated_vars && matches!(
                    body.trim_matches([' ', '\t']),
                    "&НаКлиенте" | "&НаСервере"
                ),
                body.trim_matches([' ', '\t']) == "&НаКлиенте",
            ));
            continue;
        }
        if pending_annotation.is_some() && body.trim_start().starts_with('#') {
            return fail(Reason::AnnotationUnsupported, line);
        }
        if let Some(open) = if resumed { None } else { region(body, line)? } {
            let in_method = current.is_some();
            if in_method
                && (!method_regions
                    || body.contains("//")
                    || if open {
                        owned_region.is_some()
                    } else {
                        owned_region.is_none() || regions.last().copied() != owned_region
                    })
            {
                return fail(Reason::RegionInsideMethod, line);
            }
            if !stack.is_empty() {
                return fail(Reason::Delimiters, line);
            }
            charge_tokens(&mut count, if open { 2 } else { 1 }, token_cap, line)?;
            if open {
                if regions.len() >= MAX_REGION_DEPTH {
                    return fail(Reason::Limit, line);
                }
                regions.push(line);
                if in_method {
                    owned_region = Some(line);
                }
            } else if branches.iter().any(|b| regions.len() <= b.regions.len()) {
                return fail(Reason::DirectiveUnsupported, line);
            } else if regions.pop().is_none() {
                return fail(Reason::RegionUnmatchedEnd, line);
            } else if in_method {
                owned_region = None;
            }
            continue;
        }
        let mut row = tokens(
            body, base, line, &mut count, token_cap, strings, dates_members,
            source_limits && current.is_some(), &mut opaque,
        )?;
        if resumed {
            row.insert(
                0,
                Token {
                    text: "@string".into(),
                    start: base - 1,
                    end: base,
                    identifier: false,
                },
            );
        }
        if pending_annotation.is_some_and(|(_, variables, _)| variables)
            && row.first().is_some_and(|t| t.text == "перем")
        {
            if current.is_some() || !stack.is_empty() || opaque.is_some() {
                return fail(Reason::AnnotationUnsupported, line);
            }
            if mobile_vars && row.last().is_some_and(|t| t.text == ",") {
                if variable_fragment(&row[1..]) != Some(false)
                    || !row.last().is_some_and(|t| {
                        t.end == base + body.trim_end_matches([' ', '\t']).len()
                    })
                {
                    return fail(Reason::AnnotationUnsupported, line);
                }
                for used in 1.. {
                    if used >= MAX_ANNOTATED_VAR_LINES {
                        return fail(Reason::Limit, last_line);
                    }
                    let Some((i, physical)) = physical_lines.next() else {
                        return fail(Reason::AnnotationUnsupported, last_line);
                    };
                    let next_line = i + 1;
                    if next_line > MAX_LINES {
                        return fail(Reason::Limit, next_line);
                    }
                    let body = physical.strip_suffix('\n').unwrap_or(physical);
                    let body = body.strip_suffix('\r').unwrap_or(body);
                    let end = offset + body.trim_end_matches([' ', '\t']).len();
                    if end - row[0].start > MAX_ANNOTATED_VAR_LIST_BYTES {
                        return fail(Reason::Limit, next_line);
                    }
                    let next = tokens(body, offset, next_line, &mut count, token_cap, false, false, false, &mut opaque)?;
                    let Some(last) = next.last() else {
                        return fail(Reason::AnnotationUnsupported, next_line);
                    };
                    let Some(complete) = variable_fragment(&next).filter(|_| last.end == end) else {
                        return fail(Reason::AnnotationUnsupported, next_line);
                    };
                    if row.len() + next.len() > 1 + 2 * MAX_ANNOTATED_VAR_NAMES {
                        return fail(Reason::Limit, next_line);
                    }
                    row.extend(next);
                    offset += physical.len();
                    last_line = next_line;
                    if complete {
                        break;
                    }
                }
            }
            if !annotated_module_variables(&row, line)? {
                return fail(Reason::AnnotationUnsupported, line);
            }
            // The annotation belongs to this variable declaration, never to a
            // following method. Module variables emit no method facts.
            pending_annotation = None;
            continue;
        }
        // Save the modifier byte before handing the unmodified declaration
        // grammar a view without the prefix. Do not consume another line.
        let modifier_byte = if async_casing
            && row.first().is_some_and(|t| t.text == "асинх")
            && row.get(1).is_some_and(|t| start(&t.text).is_some())
        {
            if pending_annotation.is_some_and(|(at, _, client)| !client || at + 1 != line) {
                return fail(Reason::AnnotationUnsupported, line);
            }
            Some(row.remove(0).start)
        } else {
            None
        };
        if pending_annotation.is_some() && (row.is_empty() || start(&row[0].text).is_none()) {
            return fail(Reason::AnnotationUnsupported, line);
        }
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
            let mut header_end_line = line;
            let wrapped_header = refined
                && (modifier_byte.is_none() || header_forms)
                && opaque.is_none()
                && row.len() >= 3
                && row[2].text == "("
                && row.last().is_some_and(|t| {
                    (row.len() == 3 || t.text == ",")
                        && header_line_end(body, base, t, header_forms).is_some()
                });
            let valid_header = if modifier_byte.is_some() && !wrapped_header {
                opaque.is_none()
                    && row.last().is_some_and(|t| {
                        t.text != ";" && t.end == base + body.trim_end_matches([' ', '\t']).len()
                    })
                    && parameter_header(&row, line, conditional, literals, modifier_byte, header_forms, false)?
            } else if wrapped_header {
                let line_cap = if long_headers && modifier_byte.is_none() {
                    MAX_LONG_ORDINARY_HEADER_LINES
                } else {
                    MAX_REFINED_HEADER_LINES
                };
                loop {
                    if header_end_line - line + 1 >= line_cap {
                        return fail(Reason::Limit, header_end_line);
                    }
                    let Some((_, continuation)) = physical_lines.next() else {
                        return fail(Reason::UnsupportedHeader, header_end_line);
                    };
                    let next_line = header_end_line + 1;
                    if next_line > MAX_LINES {
                        return fail(Reason::Limit, next_line);
                    }
                    let next_body = continuation.strip_suffix('\n').unwrap_or(continuation);
                    let next_body = next_body.strip_suffix('\r').unwrap_or(next_body);
                    let next = tokens(next_body, offset, next_line, &mut count, token_cap, false, dates_members, false, &mut opaque)?;
                    let Some(last) = next.last() else {
                        return fail(Reason::UnsupportedHeader, next_line);
                    };
                    let Some(end) = header_line_end(next_body, offset, last, header_forms) else {
                        return fail(Reason::UnsupportedHeader, next_line);
                    };
                    if row.len() + next.len() + usize::from(modifier_byte.is_some()) > 200
                        || end - modifier_byte.unwrap_or(row[0].start)
                            - (row[1].end - row[1].start) > 8192
                    {
                        return fail(Reason::Limit, next_line);
                    }
                    let incomplete = last.text == ",";
                    if incomplete
                        && (next.len() < 2 || next.iter().any(|t| t.text == ")" || t.text == ";"))
                    {
                        return fail(Reason::UnsupportedHeader, next_line);
                    }
                    row.extend(next);
                    header_end_line = next_line;
                    last_line = next_line;
                    offset += continuation.len();
                    if !incomplete {
                        break (modifier_byte.is_none() || row.last().is_some_and(|t| t.text != ";"))
                            && parameter_header(&row, line, conditional, true, modifier_byte, header_forms,
                                line_cap > MAX_REFINED_HEADER_LINES
                                    && header_end_line - line + 1 > MAX_REFINED_HEADER_LINES)?;
                    }
                }
            } else if wrapped
                && opaque.is_none()
                && row.len() >= 5
                && row[2].text == "("
                && row.last().is_some_and(|t| t.text == ",")
                && body.trim_end_matches([' ', '\t']).ends_with(',')
                && !body.contains("//")
            {
                let Some((_, continuation)) = physical_lines.next() else {
                    return fail(Reason::UnsupportedHeader, line);
                };
                let next_line = line + 1;
                if next_line > MAX_LINES {
                    return fail(Reason::Limit, next_line);
                }
                let next_body = continuation.strip_suffix('\n').unwrap_or(continuation);
                let next_body = next_body.strip_suffix('\r').unwrap_or(next_body);
                if !next_body.trim_end_matches([' ', '\t']).ends_with(')')
                    || next_body.contains("//")
                {
                    return fail(Reason::UnsupportedHeader, next_line);
                }
                // Parse this exact line only, never dispatching directives, branches,
                // annotations, methods or regions from the continuation.
                let next = tokens(next_body, offset, next_line, &mut count, token_cap, false, dates_members, false, &mut opaque)?;
                if next.len() < 2 {
                    return fail(Reason::UnsupportedHeader, next_line);
                }
                row.extend(next);
                header_end_line = next_line;
                last_line = next_line;
                offset += continuation.len();
                !row[3..].iter().any(|t| matches!(t.text.as_str(), "val" | "знач"))
                    && parameter_header(&row, line, conditional, false, None, header_forms, false)?
            } else if parameters {
                (!literals || opaque.is_none())
                    && parameter_header(&row, line, conditional, literals, None, header_forms, false)?
            } else {
                (row.len() == 4 || row.len() == 5 && export(&row[4].text))
                    && row[1].identifier
                    && start(&row[1].text).is_none()
                    && end(&row[1].text).is_none()
                    && !export(&row[1].text)
                    && row[2].text == "("
                    && row[3].text == ")"
            };
            if !valid_header {
                return fail(Reason::UnsupportedHeader, line);
            }
            if reserved_name(&row[1].text) {
                return fail(Reason::ReservedName, line);
            }
            pending_annotation = None;
            current = Some(Boundary {
                branch_path: branches.iter().map(|b| b.branch.clone()).collect(),
                name: row[1].text.clone(),
                kind,
                start_line: line,
                start_byte: modifier_byte.unwrap_or(row[0].start),
                header_start_byte: modifier_byte.unwrap_or(row[0].start),
                header_end_byte: row.last().unwrap().end,
                header_end_line,
                end_line: 0,
                end_byte: 0,
                occurrences: 0,
                duplicate: false,
            });
            continue;
        }
        if let Some(kind) = end(&row[0].text) {
            if let Some(open_line) = owned_region {
                return fail(Reason::RegionMissingEnd, open_line);
            }
            if current
                .as_ref()
                .is_some_and(|m| branches.iter().any(|b| b.method == Some(m.start_byte)))
            {
                return fail(Reason::DirectiveUnsupported, line);
            }
            let terminal_separator = row.len() == 2
                && row[1].text == ";"
                && anomaly_view == anomaly::View::TerminalSeparator(row[1].start);
            if (row.len() != 1 && !terminal_separator)
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
        if row.iter().enumerate().any(|(i, t)| {
            end(&t.text).is_some()
                || (start(&t.text).is_some()
                    && !(dates_members && is_dotted_procedure_member(&row, i, lexical_tail && current.is_some())))
        }) {
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
    if let Some(line) = opaque {
        return fail(Reason::MultilineString, line);
    }
    if !branches.is_empty() {
        return fail(Reason::DirectiveUnsupported, last_line);
    }
    if let Some((line, _, _)) = pending_annotation {
        return fail(Reason::AnnotationUnsupported, line);
    }
    if let Some(b) = current {
        return fail(Reason::MissingEnd, b.start_line);
    }
    if !stack.is_empty() {
        return fail(Reason::Delimiters, last_line);
    }
    if let Some(line) = regions.first() {
        return fail(Reason::RegionMissingEnd, *line);
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
        completeness: if literals {
            "scalar defaults, opaque pipe strings and physical conditional declarations; no value or branch evaluation; absence unknown"
        } else if strings {
            "opaque pipe strings and physical conditional declarations; no string value or active branch evaluation; absence unknown"
        } else if conditional {
            "physical conditional declarations; no active branch selection; absence unknown; no body/call/provenance validation"
        } else if parameters {
            "lexical bounded singleline parameter subset without defaults; absence unknown; no body/call/provenance validation"
        } else {
            "lexical zero-argument subset; absence unknown; no body/call/provenance validation"
        },
    })
}
