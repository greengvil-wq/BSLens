use super::*;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub block_line: usize,
    pub arm: usize,
    pub kind: &'static str,
    pub arm_line: usize,
    pub directive_start_byte: usize,
    pub directive_end_byte: usize,
}
pub(super) struct Frame {
    pub branch: Branch,
    pub method: Option<usize>,
    pub regions: Vec<usize>,
    pub seen_else: bool,
}
// Only explicitly versioned preprocessor atoms; never evaluate branches.
fn expression(text: &str, line: usize, refined: bool, casefold: bool, mobile: bool) -> Result<usize, Refusal> {
    if text.len() > 4096 {
        return fail(Reason::Limit, line);
    }
    let mut ts = Vec::new();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if matches!(c, ' ' | '\t' | '(' | ')') {
            if start < i {
                ts.push(&text[start..i]);
            }
            if matches!(c, '(' | ')') {
                ts.push(&text[i..i + 1]);
            }
            start = i + c.len_utf8();
        }
    }
    if start < text.len() {
        ts.push(&text[start..]);
    }
    if ts.len() > 256 {
        return fail(Reason::Limit, line);
    }
    if casefold {
        // Canonicalize only the closed list this parser already admits.
        // Raw offsets, original directive text, and grammar remain unchanged.
        for t in &mut ts {
            *t = match t.to_lowercase().as_str() {
                "не" => "Не",
                "и" => "И",
                "или" => "Или",
                "сервер" => "Сервер",
                "толстыйклиентобычноеприложение" => "ТолстыйКлиентОбычноеПриложение",
                "внешнеесоединение" => "ВнешнееСоединение",
                "вебклиент" => "ВебКлиент",
                "мобильныйклиент" => "МобильныйКлиент",
                "тонкийклиент" => "ТонкийКлиент",
                "толстыйклиентуправляемоеприложение" => "ТолстыйКлиентУправляемоеПриложение",
                "клиент" => "Клиент",
                "мобильныйавтономныйсервер" if mobile => "МобильныйАвтономныйСервер",
                "мобильноеприложениеклиент" if mobile => "МобильноеПриложениеКлиент",
                "мобильноеприложениесервер" if mobile => "МобильноеПриложениеСервер",
                _ => *t,
            };
        }
    }
    fn unary(ts: &[&str], i: &mut usize, depth: usize, refined: bool, mobile: bool) -> bool {
        if depth > 16 {
            return false;
        }
        match ts.get(*i).copied() {
            Some("Не") => {
                *i += 1;
                unary(ts, i, depth + 1, refined, mobile)
            }
            Some("(") => {
                *i += 1;
                if !or(ts, i, depth + 1, refined, mobile) || ts.get(*i) != Some(&")") {
                    return false;
                }
                *i += 1;
                true
            }
            Some("Сервер" | "ТолстыйКлиентОбычноеПриложение" | "ВнешнееСоединение") =>
            {
                *i += 1;
                true
            }
            Some(
                "ВебКлиент"
                | "МобильныйКлиент"
                | "ТонкийКлиент"
                | "ТолстыйКлиентУправляемоеПриложение"
                | "Клиент",
            ) if refined => {
                *i += 1;
                true
            }
            Some("МобильныйАвтономныйСервер" | "МобильноеПриложениеКлиент" | "МобильноеПриложениеСервер")
                if mobile =>
            {
                *i += 1;
                true
            }
            _ => false,
        }
    }
    fn and(ts: &[&str], i: &mut usize, d: usize, refined: bool, mobile: bool) -> bool {
        if !unary(ts, i, d, refined, mobile) {
            return false;
        }
        while ts.get(*i) == Some(&"И") {
            *i += 1;
            if !unary(ts, i, d, refined, mobile) {
                return false;
            }
        }
        true
    }
    fn or(ts: &[&str], i: &mut usize, d: usize, refined: bool, mobile: bool) -> bool {
        if !and(ts, i, d, refined, mobile) {
            return false;
        }
        while ts.get(*i) == Some(&"Или") {
            *i += 1;
            if !and(ts, i, d, refined, mobile) {
                return false;
            }
        }
        true
    }
    let mut i = 0;
    if !or(&ts, &mut i, 0, refined, mobile) || i != ts.len() {
        return fail(Reason::DirectiveUnsupported, line);
    }
    Ok(ts.len())
}
// Returns None only for another directive (the existing region guard handles it).
pub(super) fn directive(body: &str, line: usize, refined: bool, casefold: bool, mobile: bool) -> Result<Option<(&'static str, usize)>, Refusal> {
    let body = body.trim_start_matches([' ', '\t']);
    let body = body
        .split_once("//")
        .map_or(body, |(a, _)| a)
        .trim_end_matches([' ', '\t']);
    let (head, tail) = body.split_once([' ', '\t']).unwrap_or((body, ""));
    let kind = match head {
        "#Если" => "if",
        "#ИначеЕсли" => "elseif",
        "#Иначе" => "else",
        "#КонецЕсли" => "end",
        _ => return Ok(None),
    };
    let tail = tail.trim_matches([' ', '\t']);
    let tokens = if matches!(kind, "if" | "elseif") {
        let Some(expr) = tail.strip_suffix("Тогда") else {
            return fail(Reason::DirectiveUnsupported, line);
        };
        if !expr.ends_with([' ', '\t']) {
            return fail(Reason::DirectiveUnsupported, line);
        }
        expression(expr.trim_end_matches([' ', '\t']), line, refined, casefold, mobile)? + 2
    } else {
        if !tail.is_empty() {
            return fail(Reason::DirectiveUnsupported, line);
        }
        1
    };
    Ok(Some((kind, tokens)))
}
