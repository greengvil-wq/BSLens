//! Opt-in fallback over one exactly bounded raw-source anomaly.
//! Alternate views are local, offline and never stored as original source.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceAnomaly {
    pub code: &'static str,
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum View {
    Strict,
    TerminalSeparator(usize),
    EmptyOpaquePhysicalLine(usize),
}

fn parse(raw: &[u8], view: View, tail: bool) -> Result<Analysis, Refusal> {
    analyze_mode(
        raw, true, true, true, true, true, true, true, true, true, true, true, true, true, true,
        tail, tail, view,
    )
}

fn line_starts(raw: &[u8]) -> Option<Vec<usize>> {
    let mut out = vec![0];
    for (i, b) in raw.iter().enumerate() {
        if *b == b'\n' {
            if out.len() > MAX_LINES {
                return None;
            }
            out.push(i + 1);
        }
    }
    let physical_count = out.len() - usize::from(out.last() == Some(&raw.len()));
    (physical_count <= MAX_LINES).then_some(out)
}

fn line_content<'a>(raw: &'a [u8], starts: &[usize], line: usize) -> Option<(usize, &'a [u8])> {
    let start = *starts.get(line.checked_sub(1)?)?;
    let end = starts.get(line).copied().unwrap_or(raw.len());
    let row = raw.get(start..end)?;
    Some((
        start,
        row.strip_suffix(b"\r\n")
            .or_else(|| row.strip_suffix(b"\n"))
            .unwrap_or(row),
    ))
}

fn remap(
    mut view: Analysis,
    view_lines: &[usize],
    raw_lines: &[usize],
    inserted: Option<usize>,
) -> Option<Analysis> {
    let raw_byte = |pos| match inserted {
        Some(at) if pos > at => pos - 1,
        _ => pos,
    };
    let raw_line = |pos| raw_lines.partition_point(|v| *v <= pos);
    for m in &mut view.boundaries {
        m.start_byte = raw_byte(m.start_byte);
        m.end_byte = raw_byte(m.end_byte);
        m.header_start_byte = raw_byte(m.header_start_byte);
        m.header_end_byte = raw_byte(m.header_end_byte);
        m.start_line = raw_line(m.start_byte);
        m.end_line = raw_line(m.end_byte.checked_sub(1)?);
        m.header_end_line = raw_line(m.header_end_byte.checked_sub(1)?);
        for b in &mut m.branch_path {
            b.block_line = raw_line(raw_byte(*view_lines.get(b.block_line.checked_sub(1)?)?));
            b.arm_line = raw_line(raw_byte(*view_lines.get(b.arm_line.checked_sub(1)?)?));
            b.directive_start_byte = raw_byte(b.directive_start_byte);
            b.directive_end_byte = raw_byte(b.directive_end_byte);
        }
    }
    Some(view)
}

fn lone_cr_is_bounded(row: &[u8], at: usize, terminated_lf: bool) -> bool {
    let Ok(pre) = std::str::from_utf8(&row[..at]) else {
        return false;
    };
    let Ok(post) = std::str::from_utf8(&row[at + 1..]) else {
        return false;
    };
    let pre_trimmed = pre.trim_start_matches([' ', '\t']);
    if pre_trimmed.starts_with("//") {
        return post.starts_with("//");
    }
    // The supported ordinary fragments do not begin an opaque continuation
    // or put the exceptional byte in a quoted string or an inline comment.
    !pre_trimmed.starts_with(['#', '&', '|'])
        && !pre.contains('"')
        && !pre.contains("//")
        && post.strip_prefix('\t').is_some_and(|t| {
            let after = t.trim_start_matches([' ', '\t']);
            if after.is_empty() {
                // An observed export shape: a terminated statement followed
                // only by TAB/hspace before a real physical line ending.
                terminated_lf && pre.ends_with(';')
            } else {
                !after.starts_with(['#', '&', '|'])
            }
        })
        && pre
            .chars()
            .next_back()
            .is_some_and(|c| letter(c) || c.is_ascii_digit() || c == ';')
}

/// Returns the entire inventory or the original strict refusal. The sole
/// annotation, when present, qualifies structure only, not compilation.
pub fn analyze_source_anomalies_v1(
    raw: &[u8],
) -> Result<(Analysis, Option<SourceAnomaly>), Refusal> {
    analyze_agreement(raw, false)
}

/// Same mapped agreement over the bounded lexical-tail profile.
pub fn analyze_navigator_agreement_v1(
    raw: &[u8],
) -> Result<(Analysis, Option<SourceAnomaly>), Refusal> {
    analyze_agreement(raw, true)
}

fn analyze_agreement(raw: &[u8], tail: bool) -> Result<(Analysis, Option<SourceAnomaly>), Refusal> {
    let strict = match parse(raw, View::Strict, tail) {
        Ok(clean) => return Ok((clean, None)),
        Err(refusal) => refusal,
    };
    if !matches!(
        strict.reason,
        Reason::InvalidControl | Reason::UnmatchedEnd | Reason::MultilineString
    ) {
        return Err(strict);
    }
    // The strict analyzer has already bounded raw bytes and verified UTF-8.
    let Some(raw_lines) = line_starts(raw) else {
        return Err(strict);
    };
    let attempt = || -> Option<(Analysis, Option<SourceAnomaly>)> {
        let (base, row) = line_content(raw, &raw_lines, strict.line)?;
        let (first, second, anomaly) = match strict.reason {
            Reason::InvalidControl => {
                let mut cr = raw
                    .iter()
                    .enumerate()
                    .filter(|(i, b)| **b == b'\r' && raw.get(*i + 1) != Some(&b'\n'));
                let (at, _) = cr.next()?;
                if cr.next().is_some()
                    || at < base
                    || at - base >= row.len()
                    || !lone_cr_is_bounded(row, at - base, raw_lines.get(strict.line).is_some())
                {
                    return None;
                }
                let mut view = raw.to_vec();
                view[at] = b' ';
                let first = parse(&view, View::Strict, tail).ok()?;
                view[at] = b'\n';
                let second = remap(
                    parse(&view, View::Strict, tail).ok()?,
                    &line_starts(&view)?,
                    &raw_lines,
                    None,
                )?;
                (
                    first,
                    second,
                    SourceAnomaly {
                        code: "lone_cr_structural_agreement",
                        line: strict.line,
                        start: at,
                        end: at + 1,
                    },
                )
            }
            Reason::UnmatchedEnd => {
                let text = std::str::from_utf8(row).ok()?;
                let trimmed = text.trim_matches([' ', '\t']);
                if trimmed.to_lowercase() != "конецпроцедуры;" {
                    return None;
                }
                let at = base + text.trim_end_matches([' ', '\t']).len() - 1;
                let first = parse(raw, View::TerminalSeparator(at), tail).ok()?;
                let mut view = raw.to_vec();
                view[at] = b' ';
                let second = parse(&view, View::Strict, tail).ok()?;
                (
                    first,
                    second,
                    SourceAnomaly {
                        code: "method_terminal_semicolon",
                        line: strict.line,
                        start: at,
                        end: at + 1,
                    },
                )
            }
            Reason::MultilineString => {
                let (next, following) = line_content(raw, &raw_lines, strict.line + 1)?;
                if !row.is_empty()
                    || !std::str::from_utf8(following)
                        .ok()?
                        .trim_start_matches([' ', '\t'])
                        .starts_with('|')
                    || next <= base
                {
                    return None;
                }
                let first = parse(raw, View::EmptyOpaquePhysicalLine(base), tail).ok()?;
                let mut view = Vec::with_capacity(raw.len() + 1);
                view.extend_from_slice(&raw[..base]);
                view.push(b'|');
                view.extend_from_slice(&raw[base..]);
                let second = remap(
                    parse(&view, View::Strict, tail).ok()?,
                    &line_starts(&view)?,
                    &raw_lines,
                    Some(base),
                )?;
                (
                    first,
                    second,
                    SourceAnomaly {
                        code: "empty_opaque_physical_line",
                        line: strict.line,
                        start: base,
                        end: next,
                    },
                )
            }
            _ => return None,
        };
        if first != second {
            return None;
        }
        Some((first, Some(anomaly)))
    };
    attempt().ok_or(strict)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agrees_on_full_inventory_without_changing_lf_ranges() {
        let begin = "\u{feff}#Если Сервер Тогда\r\nПроцедура P()\r\n";
        let end = "\r\nКонецПроцедуры\r\n#КонецЕсли";
        for (body, needle, code) in [
            ("X=A\r\t+1;", "\r", "lone_cr_structural_agreement"),
            ("X=1;\r\t \t", "\r", "lone_cr_structural_agreement"),
            (" // before\r// after", "\r", "lone_cr_structural_agreement"),
            (
                "X=\"open\r\n\r\n |close\";",
                "\r\n",
                "empty_opaque_physical_line",
            ),
        ] {
            let source = format!("{begin}{body}{end}");
            assert!(parse(source.as_bytes(), View::Strict, false).is_err());
            let (inventory, diagnostic) = analyze_source_anomalies_v1(source.as_bytes()).unwrap();
            let diagnostic = diagnostic.unwrap();
            assert_eq!(diagnostic.code, code);
            assert_eq!(
                diagnostic.line,
                if code == "empty_opaque_physical_line" {
                    4
                } else {
                    3
                }
            );
            assert_eq!(
                &source.as_bytes()[diagnostic.start..diagnostic.end],
                needle.as_bytes()
            );
            assert_eq!(inventory.boundaries.len(), 1);
            let item = &inventory.boundaries[0];
            assert_eq!(item.start_line, 2);
            assert_eq!(
                item.end_line,
                if code == "empty_opaque_physical_line" {
                    6
                } else {
                    4
                }
            );
            assert_eq!(item.branch_path[0].block_line, 1);
            assert_eq!(
                &source.as_bytes()[item.end_byte - "КонецПроцедуры".len()..item.end_byte],
                "КонецПроцедуры".as_bytes()
            );
        }
        let raw = "Процедура P()\nКонецПроцедуры;\nПроцедура Q()\nКонецПроцедуры";
        let (result, diagnostic) = analyze_source_anomalies_v1(raw.as_bytes()).unwrap();
        let diagnostic = diagnostic.unwrap();
        assert_eq!(diagnostic.code, "method_terminal_semicolon");
        let p = result.boundaries.iter().find(|m| m.name == "p").unwrap();
        assert_eq!(p.end_byte, diagnostic.start);
        assert_eq!(raw.as_bytes()[diagnostic.start], b';');
        assert_eq!(result.boundaries.len(), 2);
        let clean = "Процедура P()\nКонецПроцедуры";
        assert_eq!(
            analyze_source_anomalies_v1(clean.as_bytes()).unwrap(),
            (parse(clean.as_bytes(), View::Strict, false).unwrap(), None)
        );
    }

    #[test]
    fn refuses_divergence_more_than_one_site_and_unsupported_raw_shapes() {
        for bad in [
            "Процедура P()\nX=1;\r\tКонецПроцедуры",
            "Процедура P()\nX=A\r\t \nКонецПроцедуры",
            "Процедура P()\nX=1;\r \t \nКонецПроцедуры",
            "Процедура P()\nX=1;\r\t \t",
            "Процедура P()\nX=1;\r\t#Если Сервер Тогда\n#КонецЕсли\nКонецПроцедуры",
            "#Область A\r\tX=1;\n#КонецОбласти\nПроцедура P()\nКонецПроцедуры",
            "Процедура P()\nX=1;\r\t  &НаКлиенте\nКонецПроцедуры",
            "Процедура P()\nX=1;\r\t|marker\nКонецПроцедуры",
            "Процедура P()\n// a\r// b\r// c\nКонецПроцедуры",
            "Процедура P()\nX=\"a\rb\";\nКонецПроцедуры",
            "Процедура P()\nX=1;\r\tX=2;\nКонецПроцедуры;",
            "Процедура P()\nКонецПроцедуры;;",
            "Процедура P()\nКонецПроцедуры; X=1;",
            "Процедура P()\nКонецФункции;",
            "Процедура P()\nX=\"a\n \n|b\";\nКонецПроцедуры",
            "Процедура P()\nX=\"a\n\n\n|b\";\nКонецПроцедуры",
            "Процедура P()\nX=\"a\n\n#Если Сервер Тогда\n|b\";\nКонецПроцедуры",
            "Процедура P()\n// bad \u{7}\nКонецПроцедуры",
        ] {
            assert!(
                analyze_source_anomalies_v1(bad.as_bytes()).is_err(),
                "{bad}"
            );
        }
        assert!(analyze_source_anomalies_v1(&[0xff]).is_err());
        assert!(lone_cr_is_bounded(b"X=1;\r\t ", 4, true));
        assert!(!lone_cr_is_bounded(b"X=1;\r\t ", 4, false));
        assert!(!lone_cr_is_bounded(b"X=1;\r \t", 4, true));
        assert!(!lone_cr_is_bounded(b"X=A\r\t ", 3, true));
    }
}
