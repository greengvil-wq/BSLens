//! Building blocks for opt-in method hash facts.
//!
//! This module does not participate in existing snapshot profiles.
use super::{Boundary, Reason, Refusal};

/// Validated byte ranges for the original header and body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodFragments {
    pub header_start: usize,
    pub header_end: usize,
    pub body_start: usize,
    pub body_end: usize,
}

/// Locate fragments in the original source, rejecting inconsistent boundaries.
/// The current accepted header grammar requires the declaration to end the
/// physical line; trailing horizontal whitespace and a comment are excluded.
pub fn method_fragments(raw: &[u8], boundary: &Boundary) -> Result<MethodFragments, Refusal> {
    let invalid = || Refusal {
        reason: Reason::AmbiguousBoundary,
        line: boundary.header_end_line,
    };
    if boundary.start_byte != boundary.header_start_byte
        || boundary.header_start_byte > boundary.header_end_byte
        || boundary.header_end_byte > boundary.end_byte
        || boundary.end_byte > raw.len()
    {
        return Err(invalid());
    }
    let remaining = raw.get(boundary.header_end_byte..).ok_or_else(invalid)?;
    let line_end = remaining
        .iter()
        .position(|b| *b == b'\n')
        .ok_or_else(invalid)?;
    let gap = &remaining[..line_end];
    let gap = gap.strip_suffix(b"\r").unwrap_or(gap);
    let whitespace_end = gap
        .iter()
        .position(|b| !matches!(b, b' ' | b'\t'))
        .unwrap_or(gap.len());
    if whitespace_end != gap.len() && !gap[whitespace_end..].starts_with(b"//") {
        return Err(invalid());
    }
    let body_start = boundary.header_end_byte + line_end + 1;
    if body_start > boundary.end_byte {
        return Err(invalid());
    }
    Ok(MethodFragments {
        header_start: boundary.header_start_byte,
        header_end: boundary.header_end_byte,
        body_start,
        body_end: boundary.end_byte,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexical::analyze_navigator_agreement_v1;

    #[test]
    fn fragments_preserve_bom_crlf_and_blank_body_line() {
        let raw = "\u{feff}Процедура P() // comment\r\n\r\nКонецПроцедуры\r\n".as_bytes();
        let (analysis, _) = analyze_navigator_agreement_v1(raw).unwrap();
        let f = method_fragments(raw, &analysis.boundaries[0]).unwrap();
        assert_eq!(
            &raw[f.header_start..f.header_end],
            "Процедура P()".as_bytes()
        );
        assert_eq!(
            &raw[f.body_start..f.body_end],
            "\r\nКонецПроцедуры".as_bytes()
        );
        assert_eq!(f.header_start, 3);
    }

    #[test]
    fn invalid_gap_and_missing_lf_are_refused() {
        let raw = b"Procedure P()\nEndProcedure";
        let (analysis, _) = analyze_navigator_agreement_v1(raw).unwrap();
        let mut b = analysis.boundaries[0].clone();
        b.header_end_byte -= 1;
        assert_eq!(
            method_fragments(raw, &b).unwrap_err().reason,
            Reason::AmbiguousBoundary
        );
        b.header_end_byte = raw.len();
        b.end_byte = raw.len();
        assert_eq!(
            method_fragments(raw, &b).unwrap_err().reason,
            Reason::AmbiguousBoundary
        );
    }

    #[test]
    fn mismatched_header_start_is_refused() {
        let raw = b"Procedure P()\nEndProcedure";
        let (analysis, _) = analyze_navigator_agreement_v1(raw).unwrap();
        let mut b = analysis.boundaries[0].clone();
        b.header_start_byte += 1;
        assert_eq!(
            method_fragments(raw, &b).unwrap_err().reason,
            Reason::AmbiguousBoundary
        );
    }
}
