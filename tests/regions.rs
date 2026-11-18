#![cfg(feature = "experimental-lexical")]
use bslens_core::lexical::{analyze, Reason, MAX_REGION_DEPTH};
#[test]
fn mixed_nested_regions_keep_original_offsets_and_duplicates() {
    let raw="\u{feff}\t#оБлАсТь _Публичное2 // section\r\n#rEgIoN Inner\r\n  Процедура Тест() Экспорт // header\r\n// #If ignored comment\r\nКонецПроцедуры\r\n#EnDrEgIoN // done\r\nFunction Тест()\r\nReturn \"#Region string\";\r\nEndFunction\r\n#КонецОбласти\r\n";
    let facts = analyze(raw.as_bytes()).unwrap().boundaries;
    assert_eq!(facts.len(), 2);
    let first = &facts[0];
    assert_eq!((first.start_line, first.end_line), (3, 5));
    assert_eq!(first.start_byte, raw.find("Процедура").unwrap());
    assert_eq!(
        first.end_byte,
        raw.find("КонецПроцедуры").unwrap() + "КонецПроцедуры".len()
    );
    assert!(first.duplicate);
    assert_eq!(first.occurrences, 2);
    assert_eq!(
        &raw[first.start_byte..first.end_byte],
        "Процедура Тест() Экспорт // header\r\n// #If ignored comment\r\nКонецПроцедуры"
    );
    assert_eq!((facts[1].start_line, facts[1].end_line), (7, 9));
}
#[test]
fn limits_and_unbalanced_regions_refuse_all_candidates() {
    let exact = format!(
        "{}Procedure A()\nEndProcedure\n{}",
        "#Region R\n".repeat(MAX_REGION_DEPTH),
        "#EndRegion\n".repeat(MAX_REGION_DEPTH)
    );
    assert_eq!(analyze(exact.as_bytes()).unwrap().boundaries.len(), 1);
    let over = format!("#Region R\n{exact}");
    assert_eq!(analyze(over.as_bytes()).unwrap_err().reason, Reason::Limit);
    for (source, reason) in [
        (
            "Procedure A()\nEndProcedure\n#EndRegion",
            Reason::RegionUnmatchedEnd,
        ),
        (
            "#Region R\nProcedure A()\nEndProcedure",
            Reason::RegionMissingEnd,
        ),
        (
            "Procedure A()\n#Region R\n#EndRegion\nEndProcedure",
            Reason::RegionInsideMethod,
        ),
        (
            "#Region R\nProcedure A()\n#EndRegion\nEndProcedure",
            Reason::RegionInsideMethod,
        ),
        ("Call(\n#Region R\n);\n#EndRegion", Reason::Delimiters),
    ] {
        assert_eq!(analyze(source.as_bytes()).unwrap_err().reason, reason);
    }
}
#[test]
fn malformed_headers_and_unsupported_directives_stay_closed() {
    for source in [
        "#Region",
        "#Region A B",
        "#Region 1A",
        "#Region A-B",
        "#Region \"A\"",
        "#Region A;",
        "#EndRegion A",
        "#Region A &AtClient",
        "#Region A #If Server",
        "#Region A\u{a0}",
    ] {
        assert_eq!(
            analyze(source.as_bytes()).unwrap_err().reason,
            Reason::RegionHeader,
            "{source}"
        );
    }
    for source in [
        "# Region A",
        "#If Server Then\n#Region R\n#EndRegion\n#EndIf",
        "#Region R\n#Else\n#EndRegion",
        "#Region R\n#Unknown\n#EndRegion",
        "x = 1; #Region R",
    ] {
        assert_eq!(
            analyze(source.as_bytes()).unwrap_err().reason,
            Reason::DirectiveUnsupported,
            "{source}"
        );
    }
    assert_eq!(
        analyze(b"#Region R\n&AtClient\nProcedure X()\nEndProcedure\n#EndRegion")
            .unwrap_err()
            .reason,
        Reason::AnnotationUnsupported
    );
    assert_eq!(
        analyze(b"#Region R\nProcedure X(P)\nEndProcedure\n#EndRegion")
            .unwrap_err()
            .reason,
        Reason::UnsupportedHeader
    );
    for source in [
        "#Region R // hidden\0\n#EndRegion",
        "// hidden\u{7f}\n#Region R\n#EndRegion",
        "#Region R\r#EndRegion",
    ] {
        assert_eq!(
            analyze(source.as_bytes()).unwrap_err().reason,
            Reason::InvalidControl
        );
    }
    assert!(analyze(b"// #Region Fake\nProcedure X()\ntext=\"#EndRegion\";\nEndProcedure").is_ok());
}
