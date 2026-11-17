#![cfg(feature = "experimental-lexical")]
use bslens_core::lexical::{analyze, Reason, MAX_BYTES, MAX_LINES, MAX_TOKENS};
#[test]
fn external_python_authored_vectors_exact_coordinates() {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/lexical/oracle.json")).unwrap();
    for vector in vectors.as_array().unwrap() {
        let hex = vector["raw_hex"].as_str().unwrap();
        let raw: Vec<_> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let result = analyze(&raw).unwrap();
        let boundaries:Vec<_>=result.boundaries.iter().map(|b|serde_json::json!({"name":b.name,"kind":b.kind,"start_line":b.start_line,"start_byte":b.start_byte,"header_start_byte":b.header_start_byte,"header_end_byte":b.header_end_byte,"header_end_line":b.header_end_line,"end_line":b.end_line,"end_byte":b.end_byte,"occurrences":b.occurrences,"duplicate":b.duplicate})).collect();
        assert_eq!(
            serde_json::json!(boundaries),
            vector["expected"],
            "{}",
            vector["name"]
        );
        for b in result.boundaries {
            assert!(std::str::from_utf8(&raw[b.start_byte..b.end_byte]).is_ok());
            assert_eq!(
                raw[..b.start_byte].iter().filter(|c| **c == b'\n').count() + 1,
                b.start_line
            );
            assert_eq!(
                raw[..b.end_byte].iter().filter(|c| **c == b'\n').count() + 1,
                b.end_line
            );
        }
    }
}
#[test]
fn unsupported_whole_module_no_partial_boundaries() {
    let good = "Procedure Good()\nEndProcedure\n";
    for (body, reason) in [
        (
            "#If Server Then\nProcedure X()\nEndProcedure\n#EndIf",
            Reason::DirectiveUnsupported,
        ),
        (
            "Async Procedure X()\nEndProcedure",
            Reason::AsyncUnsupported,
        ),
        ("Асинх Функция Х()\nКонецФункции", Reason::AsyncUnsupported),
        (
            "&AtServer\nProcedure X()\nEndProcedure",
            Reason::AnnotationUnsupported,
        ),
        ("Procedure X(a)\nEndProcedure", Reason::UnsupportedHeader),
        ("Procedure X(\n)\nEndProcedure", Reason::UnsupportedHeader),
        (
            "Procedure X()\nProcedure Y()\nEndProcedure",
            Reason::AmbiguousBoundary,
        ),
        ("x = 1; Procedure X()", Reason::AmbiguousBoundary),
        ("Procedure X()\nEndFunction", Reason::UnmatchedEnd),
        ("Procedure X()\n", Reason::MissingEnd),
        ("Procedure X()\nx = (1];\nEndProcedure", Reason::Delimiters),
        (
            "Procedure X()\nx = \"text\n|continued\";\nEndProcedure",
            Reason::MultilineString,
        ),
        ("x = '20200101';", Reason::UnsupportedCharacter),
        ("Procedure X()\nEndProcedure;", Reason::UnmatchedEnd),
    ] {
        assert_eq!(
            analyze(format!("{good}{body}").as_bytes())
                .unwrap_err()
                .reason,
            reason,
            "{body}"
        );
    }
}
#[test]
fn limits_invalid_utf8_and_normalized_occurrences() {
    assert_eq!(analyze(&[0xff]).unwrap_err().reason, Reason::Utf8);
    assert_eq!(
        analyze(&vec![b' '; MAX_BYTES + 1]).unwrap_err().reason,
        Reason::Limit
    );
    assert_eq!(
        analyze(&vec![b'\n'; MAX_LINES + 1]).unwrap_err().reason,
        Reason::Limit
    );
    assert_eq!(
        analyze("+ ".repeat(MAX_TOKENS + 1).as_bytes())
            .unwrap_err()
            .reason,
        Reason::Limit
    );
    assert_eq!(
        analyze("(".repeat(129).as_bytes()).unwrap_err().reason,
        Reason::Limit
    );
    let b = analyze("Процедура ЁЖ()\nКонецПроцедуры\nПроцедура ёж()\nКонецПроцедуры".as_bytes())
        .unwrap()
        .boundaries;
    assert_eq!(b.len(), 2);
    assert!(b
        .iter()
        .all(|b| b.name == "ёж" && b.occurrences == 2 && b.duplicate));
    assert_ne!(b[0].start_byte, b[1].start_byte);
    assert!(analyze(b"").unwrap().boundaries.is_empty());
    assert!(bslens_core::ValidatedSnapshot::open(std::path::Path::new("Cargo.toml"), "x").is_err());
}

#[test]
fn external_python_negative_vectors_are_never_promoted() {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/lexical/refusals.json")).unwrap();
    for v in vectors.as_array().unwrap() {
        assert!(v["oracle"]["boundaries"].as_array().unwrap().is_empty());
        assert!(!v["oracle"]["excluded"].as_array().unwrap().is_empty());
        assert!(
            analyze(v["source"].as_str().unwrap().as_bytes()).is_err(),
            "{}",
            v["name"]
        );
    }
}

#[test]
fn global_controls_are_not_hidden_by_comments_or_literals() {
    for control in [
        '\0', '\u{000b}', '\u{001f}', '\u{007f}', '\u{0085}', '\u{009f}',
    ] {
        for body in [
            format!("// hidden {control}"),
            format!("x=\"hidden {control}\";"),
        ] {
            let source = format!("Procedure X()\n{body}\nEndProcedure");
            let refusal = analyze(source.as_bytes()).unwrap_err();
            assert_eq!(refusal.reason, Reason::InvalidControl);
            assert_eq!(refusal.line, 2);
        }
    }
    for source in [
        "// lone\r",
        "// lone\rtext",
        "Procedure X()\rEndProcedure",
        "Procedure X()\nEndProcedure\r",
        "// double\r\r\n",
    ] {
        assert_eq!(
            analyze(source.as_bytes()).unwrap_err().reason,
            Reason::InvalidControl
        );
    }
    let raw = "\u{feff}\tПроцедура Ёж()\r\n\t// safe\r\nКонецПроцедуры\r\n".as_bytes();
    let b = analyze(raw).unwrap().boundaries.remove(0);
    assert_eq!(b.start_byte, 4);
    assert_eq!(b.start_line, 1);
    assert_eq!(b.end_line, 3);
    assert_eq!(&raw[b.end_byte..], b"\r\n");
}
#[test]
fn explicit_reserved_name_subset_fails_closed() {
    for name in [
        "If",
        "ЕСЛИ",
        "Return",
        "Возврат",
        "True",
        "Неопределено",
        "While",
        "И",
    ] {
        let source = format!("Procedure {name}()\nEndProcedure");
        assert_eq!(
            analyze(source.as_bytes()).unwrap_err().reason,
            Reason::ReservedName
        );
    }
    assert!(analyze(b"Procedure IfValue()\nEndProcedure").is_ok());
    assert!(analyze("Процедура ЕслиЗначение()\nКонецПроцедуры".as_bytes()).is_ok());
}
