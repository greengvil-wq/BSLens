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

#[test]
fn context_v1_exact_allowlist_adjacency_boundaries_and_legacy() {
    use bslens_core::lexical::analyze_context_v1;
    for annotation in ["НаКлиенте", "НаСервере", "НаСервереБезКонтекста"]
    {
        let raw = format!("\u{feff}\t&{annotation} \r\nПроцедура Тест()\r\nКонецПроцедуры");
        assert_eq!(
            analyze(raw.as_bytes()).unwrap_err().reason,
            Reason::AnnotationUnsupported
        );
        let parsed = analyze_context_v1(raw.as_bytes()).unwrap();
        assert_eq!(parsed.boundaries.len(), 1);
        let b = &parsed.boundaries[0];
        assert_eq!(b.start_line, 2);
        assert_eq!(b.end_line, 3);
        assert_eq!(
            &raw.as_bytes()[b.start_byte..b.end_byte],
            "Процедура Тест()\r\nКонецПроцедуры".as_bytes()
        );
    }
    let method = "Процедура P()\nКонецПроцедуры";
    for annotation in [
        "НаКлиенте()",
        "AtClient",
        "НаClient",
        "наклиенте",
        "НаСервереБезКонтекста(1)",
        "НаКлиенте // comment",
        "НаКлиенте;",
        "НаКлиенте &НаСервере",
    ] {
        assert!(
            analyze_context_v1(format!("&{annotation}\n{method}").as_bytes()).is_err(),
            "{annotation}"
        );
    }
    for bad in [
        "&НаКлиенте",
        "&НаКлиенте\n",
        "&НаКлиенте\n\nПроцедура P()\nКонецПроцедуры",
        "&НаКлиенте\n// comment\nПроцедура P()\nКонецПроцедуры",
        "&НаКлиенте\n&НаКлиенте\nПроцедура P()\nКонецПроцедуры",
        "&НаКлиенте\n&НаСервере\nПроцедура P()\nКонецПроцедуры",
        "&НаКлиенте\n#Область R\nПроцедура P()\nКонецПроцедуры\n#КонецОбласти",
        "#Область R\n&НаКлиенте\n#КонецОбласти",
        "&НаКлиенте\n#Если Клиент Тогда",
        "&НаКлиенте\nX=1;",
        "X=(&НаКлиенте);",
        "Процедура P()\n&НаКлиенте\nКонецПроцедуры",
        "&НаКлиенте Процедура P()\nКонецПроцедуры",
    ] {
        assert!(analyze_context_v1(bad.as_bytes()).is_err(), "{bad}");
    }
    assert_eq!(
        analyze_context_v1(b"#If Client Then").unwrap_err().reason,
        Reason::DirectiveUnsupported
    );
    assert_eq!(
        analyze_context_v1("&НаКлиенте\nПроцедура P(Arg)\nКонецПроцедуры".as_bytes())
            .unwrap_err()
            .reason,
        Reason::UnsupportedHeader
    );
    let plain = "Процедура P()\nX=\"&НаКлиенте\"; // &НаСервере\nКонецПроцедуры";
    assert_eq!(
        analyze(plain.as_bytes()),
        analyze_context_v1(plain.as_bytes())
    );
    let region = format!("#Область R\n&НаКлиенте\n{method}\n#КонецОбласти");
    assert!(analyze_context_v1(region.as_bytes()).is_ok());
}

#[test]
fn parameter_v1_singleline_bounded_names_context_and_legacy() {
    use bslens_core::lexical::{analyze_context_v1, analyze_parameters_v1};
    for header in [
        "Процедура P(A)",
        "Функция P(Знач A, B, Val Третий, Четвертый) Экспорт",
        "Procedure P(Val A, B) Export",
    ] {
        let end = if header.starts_with("Функция") {
            "КонецФункции"
        } else if header.starts_with("Procedure") {
            "EndProcedure"
        } else {
            "КонецПроцедуры"
        };
        let raw=format!("\u{feff}&НаСервере\r\n{header} // comma,) ignored\r\nX=\"comma, ) // &НаКлиенте\";\r\n{end}");
        let a = analyze_parameters_v1(raw.as_bytes()).unwrap();
        assert_eq!(a.boundaries.len(), 1);
        let b = &a.boundaries[0];
        assert_eq!(b.start_line, 2);
        assert_eq!(b.end_line, 4);
        assert_eq!(
            &raw.as_bytes()[b.header_start_byte..b.header_end_byte],
            header.as_bytes()
        );
        assert_eq!(
            &raw.as_bytes()[b.start_byte..b.end_byte],
            raw.split_once('\n').unwrap().1.as_bytes()
        );
        assert!(analyze_context_v1(raw.as_bytes()).is_err());
        assert!(analyze(raw.as_bytes()).is_err());
    }
    for h in [
        "P(A,a)",
        "P(Тест,тест)",
        "P(A,)",
        "P(,A)",
        "P(Val)",
        "P(Val Знач A)",
        "P(A B)",
        "P(A=1)",
        "P(A=\"x,)\")",
        "P(A=Undefined)",
        "P(A=Fn())",
        "P(A+1)",
        "P(A[0])",
        "P(If)",
        "P(Export)",
        "P(Procedure)",
        "P(A) Export Export",
        "P(A);",
        "P(A) Procedure Other()",
        "P(A,\nB)",
        "P(//)\nA)",
    ] {
        let raw = format!("Процедура {h}\nКонецПроцедуры");
        assert!(analyze_parameters_v1(raw.as_bytes()).is_err(), "{h}");
    }
    for raw in [
        "&НаКлиенте\n&НаСервере\nПроцедура P(A)\nКонецПроцедуры",
        "&НаКлиенте\n\nПроцедура P(A)\nКонецПроцедуры",
        "#If Client Then\nProcedure P(A)\nEndProcedure",
        "&НаКлиенте\n#Область R\nПроцедура P(A)\nКонецПроцедуры\n#КонецОбласти",
    ] {
        assert!(analyze_parameters_v1(raw.as_bytes()).is_err());
    }
    for n in [64, 65] {
        let params = (0..n)
            .map(|i| format!("Val P{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let raw = format!("Procedure P({params})\nEndProcedure");
        if n == 64 {
            assert!(analyze_parameters_v1(raw.as_bytes()).is_ok());
        } else {
            assert_eq!(
                analyze_parameters_v1(raw.as_bytes()).unwrap_err().reason,
                Reason::Limit
            );
        }
    }
    for n in [128, 129] {
        let raw = format!("Procedure P({})\nEndProcedure", "A".repeat(n));
        if n == 128 {
            assert!(analyze_parameters_v1(raw.as_bytes()).is_ok());
        } else {
            assert_eq!(
                analyze_parameters_v1(raw.as_bytes()).unwrap_err().reason,
                Reason::Limit
            );
        }
    }
    let large = format!("Procedure P(A{} )\nEndProcedure", " ".repeat(8192));
    assert_eq!(
        analyze_parameters_v1(large.as_bytes()).unwrap_err().reason,
        Reason::Limit
    );
    let plain = "&НаКлиенте\nПроцедура P()\nКонецПроцедуры";
    assert_eq!(
        analyze_context_v1(plain.as_bytes()).unwrap().boundaries,
        analyze_parameters_v1(plain.as_bytes()).unwrap().boundaries
    );
}

#[test]
fn conditional_v1_physical_arms_state_restore_and_bounds() {
    use bslens_core::lexical::{analyze_conditionals_v1 as parse, analyze_parameters_v1};
    let raw="\u{feff}#Область Outer\r\n#Если Не (Сервер И ВнешнееСоединение) Или ТолстыйКлиентОбычноеПриложение Тогда // ) ignored\r\n&НаКлиенте\r\nПроцедура P(A)\r\n#Если Сервер Тогда\r\nX=(1);\r\n#Иначе\r\nX=\"#Если ,) //\";\r\n#КонецЕсли\r\nКонецПроцедуры\r\n#ИначеЕсли Сервер Тогда\r\n#Если ВнешнееСоединение Тогда\r\nПроцедура P(A)\r\nКонецПроцедуры\r\n#КонецЕсли\r\n#Иначе\r\n#Область Inner\r\nФункция F()\r\nКонецФункции\r\n#КонецОбласти\r\n#КонецЕсли\r\n#КонецОбласти";
    let a = parse(raw.as_bytes()).unwrap();
    assert_eq!(a.boundaries.len(), 3);
    assert!(analyze_parameters_v1(raw.as_bytes()).is_err());
    let b = &a.boundaries[0];
    assert_eq!(b.branch_path.len(), 1);
    assert_eq!(b.branch_path[0].block_line, 2);
    assert_eq!(b.branch_path[0].arm, 0);
    assert_eq!(b.start_line, 4);
    assert_eq!(b.end_line, 10);
    let second = &a.boundaries[1];
    assert_eq!(second.branch_path.len(), 2);
    assert_eq!(second.branch_path[0].arm, 1);
    assert_eq!(second.branch_path[0].kind, "elseif");
    assert_eq!(second.branch_path[1].block_line, 12);
    assert_eq!(a.boundaries[2].branch_path[0].arm, 2);
    assert_eq!(a.boundaries[2].branch_path[0].kind, "else");
    for b in &a.boundaries {
        for p in &b.branch_path {
            let exact = &raw.as_bytes()[p.directive_start_byte..p.directive_end_byte];
            assert!(std::str::from_utf8(exact).unwrap().starts_with('#'));
        }
    }
    for bad in [
        "#Если Клиент Тогда\n#КонецЕсли", "#Если Сервер И Тогда\n#КонецЕсли", "#Если Сервер ВнешнееСоединение Тогда\n#КонецЕсли", "#Если (Сервер Тогда\n#КонецЕсли", "#Если \"Сервер\" Тогда\n#КонецЕсли", "#If Server Then\n#EndIf", "#если Сервер Тогда\n#КонецЕсли", "#Если Сервер Тогда extra\n#КонецЕсли",
        "#Иначе", "#КонецЕсли", "#Если Сервер Тогда", "#Если Сервер Тогда\n#Иначе\n#Иначе\n#КонецЕсли", "#Если Сервер Тогда\n#Иначе\n#ИначеЕсли Сервер Тогда\n#КонецЕсли",
        "X=(\n#Если Сервер Тогда\n);\n#КонецЕсли", "#Если Сервер Тогда\nX=(\n#Иначе\n);\n#КонецЕсли", "#Если Сервер Тогда\n&НаКлиенте\n#КонецЕсли", "&НаКлиенте\n#Если Сервер Тогда\n#КонецЕсли",
        "#Если Сервер Тогда\nПроцедура P()\n#Иначе\nКонецПроцедуры\n#КонецЕсли",
        "Процедура P()\n#Если Сервер Тогда\nКонецПроцедуры\nПроцедура P()\n#КонецЕсли\nКонецПроцедуры",
        "Процедура P()\n#Если Сервер Тогда\nПроцедура Q()\nКонецПроцедуры\n#КонецЕсли\nКонецПроцедуры",
        "#Область R\n#Если Сервер Тогда\n#КонецОбласти\n#Область R\n#КонецЕсли\n#КонецОбласти",
        "#Если Сервер Тогда\n#Область R\n#КонецЕсли\n#КонецОбласти",
        "Процедура P()\nX=1; #Если Сервер Тогда\nКонецПроцедуры",
    ] {assert!(parse(bad.as_bytes()).is_err(),"{bad}");}
    for n in [8, 9] {
        let raw = format!(
            "{}{}",
            "#Если Сервер Тогда\n".repeat(n),
            "#КонецЕсли\n".repeat(n)
        );
        assert_eq!(parse(raw.as_bytes()).is_ok(), n == 8);
    }
    for n in [4096, 4097] {
        let raw = "#Если Сервер Тогда\n#КонецЕсли\n".repeat(n);
        assert_eq!(parse(raw.as_bytes()).is_ok(), n == 4096);
    }
    for n in [63, 64] {
        let raw = format!(
            "#Если Сервер Тогда\n{}#КонецЕсли",
            "#ИначеЕсли Сервер Тогда\n".repeat(n)
        );
        assert_eq!(parse(raw.as_bytes()).is_ok(), n == 63);
    }
    for expr in [
        format!("{}Сервер", "Не ".repeat(17)),
        format!("Сервер{}", " ".repeat(4096)) + "И Сервер",
        (0..130).map(|_| "Сервер").collect::<Vec<_>>().join(" Или "),
    ] {
        assert!(parse(format!("#Если {expr} Тогда\n#КонецЕсли").as_bytes()).is_err());
    }
    let plain = "Процедура P(A)\nКонецПроцедуры";
    assert_eq!(
        parse(plain.as_bytes()).unwrap().boundaries,
        analyze_parameters_v1(plain.as_bytes()).unwrap().boundaries
    );
}

#[test]
fn conditional_successor_preserves_long_legacy_method_names() {
    use bslens_core::lexical::{
        analyze_conditionals_v1, analyze_context_v1, analyze_parameters_v1,
    };
    for size in [76, 5000] {
        let name = "Я".repeat(size);
        let raw = format!("\u{feff}&НаСервере\r\nПроцедура {name}()\r\nКонецПроцедуры");
        assert!(analyze_context_v1(raw.as_bytes()).is_ok());
        assert_eq!(
            analyze_parameters_v1(raw.as_bytes()).unwrap_err().reason,
            Reason::Limit
        );
        let a = analyze_conditionals_v1(raw.as_bytes()).unwrap();
        let b = &a.boundaries[0];
        assert_eq!(
            &raw.as_bytes()[b.start_byte..b.end_byte],
            format!("Процедура {name}()\r\nКонецПроцедуры").as_bytes()
        );
    }
    let raw = format!("Процедура P({})\nКонецПроцедуры", "Я".repeat(65));
    assert_eq!(
        analyze_conditionals_v1(raw.as_bytes()).unwrap_err().reason,
        Reason::Limit
    );
}

#[test]
fn pipe_strings_v1_opaque_fragments_resume_and_limits() {
    use bslens_core::lexical::{analyze_conditionals_v1, analyze_pipe_strings_v1 as parse};
    let raw="\u{feff}#Если Сервер Тогда\r\n&НаСервере\r\nПроцедура P(A)\r\nX=(\"first\r\n \t|Процедура Fake() #Если Unknown Тогда ([ &НаКлиенте\r\n\t// ignored comment \" #КонецЕсли\r\n |КонецПроцедуры\r\n |quotes \"\"inside\"\" and // literal\r\n |last\"); X=\"next\r\n |end\";\r\nКонецПроцедуры\r\n#КонецЕсли";
    let a = parse(raw.as_bytes()).unwrap();
    assert_eq!(a.boundaries.len(), 1);
    let b = &a.boundaries[0];
    assert_eq!(b.name, "p");
    assert_eq!(b.start_line, 3);
    assert_eq!(b.end_line, 11);
    assert_eq!(b.branch_path[0].block_line, 1);
    assert_eq!(
        &raw.as_bytes()[b.start_byte..b.end_byte],
        raw.split("\r\n")
            .skip(2)
            .take(9)
            .collect::<Vec<_>>()
            .join("\r\n")
            .as_bytes()
    );
    assert_eq!(
        analyze_conditionals_v1(raw.as_bytes()).unwrap_err().reason,
        Reason::MultilineString
    );
    for bad in [
        "Процедура P()\nX=\"open\nmissing pipe\";\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n\n|end\";\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n\u{a0}|end\";\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n// quote \" does not close",
        "Процедура P()\nX=\"open\n|КонецПроцедуры",
        "Процедура P()\nX=\"open\n|end\"\"",
        "Процедура P()\nX=\"open\n|end\");\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n|end\"; #Если Сервер Тогда\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n|end\"; &НаКлиенте\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n|end\"; Процедура Fake()\nКонецПроцедуры",
        "Процедура P(A=\"open\n|end\")\nКонецПроцедуры",
        "Процедура P(A,\nB)\nКонецПроцедуры",
        "Процедура P()\n|not a continuation\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n#КонецЕсли\n|end\";\nКонецПроцедуры",
    ] {
        assert!(parse(bad.as_bytes()).is_err(), "{bad}");
    }
    let plain = "&НаСервере\nПроцедура P(A)\nX=\"escaped \"\"quote\"\" //\";\nКонецПроцедуры";
    assert_eq!(
        parse(plain.as_bytes()).unwrap().boundaries,
        analyze_conditionals_v1(plain.as_bytes())
            .unwrap()
            .boundaries
    );
    for n in [65532, 65533] {
        let raw = format!(
            "Процедура P()\nX=\"open\n{}|end\";\nКонецПроцедуры",
            "|x\n".repeat(n)
        );
        if n == 65532 {
            assert!(parse(raw.as_bytes()).is_ok());
        } else {
            assert_eq!(parse(raw.as_bytes()).unwrap_err().reason, Reason::Limit);
        }
    }
    let too_big = format!("X=\"{}\n|end\";", "x".repeat(1024 * 1024));
    assert_eq!(parse(too_big.as_bytes()).unwrap_err().reason, Reason::Limit);
}

#[test]
fn literal_defaults_v1_is_single_line_and_does_not_evaluate() {
    use bslens_core::lexical::{analyze_literal_defaults_v1 as parse, analyze_pipe_strings_v1};
    let span = "Процедура P(Знач A=-12.50, B=Ложь, C=undefined, D=\"x,)// \"\"quote\"\"\");";
    let end = "КонецПроцедуры";
    let src = format!("\u{feff}&НаКлиентеНаСервереБезКонтекста\r\n{span} // ignored\r\n{end}");
    let item = parse(src.as_bytes()).unwrap().boundaries.remove(0);
    assert_eq!(item.start_line, 2);
    assert_eq!(item.header_end_line, 2);
    assert_eq!(item.end_line, 3);
    assert_eq!(&src.as_bytes()[item.header_start_byte..item.header_end_byte], span.as_bytes());
    assert_eq!(&src.as_bytes()[item.end_byte - end.len()..item.end_byte], end.as_bytes());
    assert!(analyze_pipe_strings_v1(src.as_bytes()).is_err());

    for (tail, ok) in [
        ("(A=+12);", true),
        ("(A=\"\");", true),
        ("(A=true) Экспорт; // separator", true),
        ("(A=12345678901234567890123456789012)", true),
        ("(A=123456789012345678901234567890123)", false),
        ("(A=.1)", false),
        ("(A=1.)", false),
        ("(A=1 2)", false),
        ("(A=- 1)", false),
        ("(A=Строка(1))", false),
        ("(A=(1))", false),
        ("(A=1+2)", false),
        ("(A=Null)", false),
        ("(A=\"x\" \"y\")", false),
        ("(A=\"open\n|close\")", false),
        ("(A=1); Экспорт", false),
        ("(A=1);;", false),
        ("(A=1); X=2", false),
    ] {
        let module = format!("Процедура Test{tail}\nКонецПроцедуры");
        assert_eq!(parse(module.as_bytes()).is_ok(), ok, "{tail}");
    }
}

#[test]
fn wrapped_headers_v1_consumes_exactly_one_adjacent_plain_parameter_line() {
    use bslens_core::lexical::{analyze_literal_defaults_v1, analyze_wrapped_headers_v1 as parse};
    let src = "\u{feff}#Если Сервер Тогда\r\n&НаКлиенте\r\nПроцедура P(A, B, C, \t \r\n \tГ, Д) \r\nКонецПроцедуры\r\n#КонецЕсли";
    let item = parse(src.as_bytes()).unwrap().boundaries.remove(0);
    assert_eq!(item.start_line, 3);
    assert_eq!(item.header_end_line, 4);
    assert_eq!(item.end_line, 5);
    assert_eq!(
        &src.as_bytes()[item.header_start_byte..item.header_end_byte],
        "Процедура P(A, B, C, \t \r\n \tГ, Д)".as_bytes()
    );
    assert_eq!(item.branch_path[0].block_line, 1);
    assert!(analyze_literal_defaults_v1(src.as_bytes()).is_err());
    for bad in [
        "P(A\n B)",
        "P(A,\n\n B)",
        "P(A,\n B,\n C)",
        "P(A, // end\n B)",
        "P(A,\n B) // end",
        "P(A,\n #Иначе)",
        "P(A,\n Процедура Other())",
        "P(A,\n КонецПроцедуры)",
        "P(A,\n (B))",
        "P(A,\n B))",
        "P(A,\n B) Экспорт",
        "P(A,\n B);",
        "P(A,\n A)",
        "P(A,\n Знач B)",
        "P(Val A,\n B)",
        "P(A=0,\n B)",
        "P(A,\n B=0)",
        "P(A,\n )",
        "P(A,\n B",
        "P(A,",
    ] {
        let src = format!("Процедура Before()\nКонецПроцедуры\nПроцедура {bad}\nКонецПроцедуры");
        assert!(parse(src.as_bytes()).is_err(), "{bad}");
    }
}

#[test]
fn corpus_refinements_v1_admits_only_explicit_header_and_branch_grammar() {
    use bslens_core::lexical::{analyze_corpus_refinements_v1 as parse, analyze_wrapped_headers_v1};
    let header = "Функция P(\r\n  Знач A=2,\r\n  B=\"x\") Экспорт ;";
    let tail = "КонецФункции";
    let src = format!("\u{feff}#Если ВебКлиент И Не МобильныйКлиент Тогда\r\n{header}\r\n{tail}\r\n#КонецЕсли");
    let item = parse(src.as_bytes()).unwrap().boundaries.remove(0);
    assert_eq!(item.start_line, 2);
    assert_eq!(item.header_end_line, 4);
    assert_eq!(item.end_line, 5);
    assert_eq!(&src.as_bytes()[item.header_start_byte..item.header_end_byte], header.as_bytes());
    assert_eq!(&src.as_bytes()[item.end_byte-tail.len()..item.end_byte], tail.as_bytes());
    assert_eq!(item.branch_path[0].block_line, 1);
    assert!(analyze_wrapped_headers_v1(src.as_bytes()).is_err());

    for atom in ["ВебКлиент", "МобильныйКлиент", "ТонкийКлиент", "ТолстыйКлиентУправляемоеПриложение", "Клиент"] {
        let src = format!("#Если Не ({atom} Или Сервер) Тогда\nПроцедура P()\nКонецПроцедуры\n#КонецЕсли");
        assert!(parse(src.as_bytes()).is_ok(), "{atom}");
        assert!(analyze_wrapped_headers_v1(src.as_bytes()).is_err(), "{atom}");
    }
    let unknown = "#Если ПохожийКлиент Тогда\nПроцедура P()\nКонецПроцедуры\n#КонецЕсли";
    assert!(parse(unknown.as_bytes()).is_err());

    for bad in [
        "P(\n\n B)",
        "P(\n // blank\n B)",
        "P(\n #Если ВебКлиент Тогда\n B)",
        "P(\n &НаКлиенте\n B)",
        "P(\n #Область Name\n B)",
        "P(\n Процедура Другой())",
        "P(\n A=Call())",
        "P(\n A=1+2)",
        "P(A,\n A)",
        "P(A,\n B))",
        "P(A,\n B, // not allowed\n C)",
        "P(A,\n B\n C)",
        "P(A,\n B,",
    ] {
        let src = format!("Процедура Before()\nКонецПроцедуры\nПроцедура {bad}\nКонецПроцедуры");
        assert!(parse(src.as_bytes()).is_err(), "{bad}");
    }
    let quoted = "Процедура URL(A=\"https://example.invalid/\"\"quoted\"\"/tail\",\n B=\"//not a comment\")\nКонецПроцедуры";
    let quoted_fact = parse(quoted.as_bytes()).unwrap().boundaries.remove(0);
    assert_eq!(quoted_fact.header_end_line, 2);
    assert!(quoted[..quoted_fact.header_end_byte].ends_with("B=\"//not a comment\")"));
    for comment in [
        "Процедура P(A=\"https://example.invalid\", // actual comment,\n B)\nКонецПроцедуры",
        "Процедура P(\n A=\"https://example.invalid\") // actual comment\nКонецПроцедуры",
    ] {
        assert!(parse(comment.as_bytes()).is_err());
    }

    let middle = (1..=14).map(|i| format!(" A{i},\n")).collect::<String>();
    let exactly = format!("Процедура P(\n{middle} Z)\nКонецПроцедуры");
    let bound = parse(exactly.as_bytes()).unwrap().boundaries.remove(0);
    assert_eq!(bound.header_end_line, 16);
    assert_eq!(bound.end_line, 17);
    let excess = format!("Процедура P(\n{middle} Extra,\n Z)\nКонецПроцедуры");
    assert_eq!(
        parse(excess.as_bytes()).unwrap_err().reason,
        bslens_core::lexical::Reason::Limit
    );
}

#[test]
fn annotated_variables_v1_consume_the_annotation_without_emitting_methods() {
    use bslens_core::lexical::{analyze_annotated_vars_v1 as parse, analyze_corpus_refinements_v1};
    let method = "Процедура Valid()\r\nКонецПроцедуры";
    let src = format!("\u{feff}#Если Сервер Тогда\r\n&НаКлиенте\r\nПерем A, B; // ok\r\n&НаСервере\r\nПерем C Экспорт;\r\n{method}\r\n#КонецЕсли");
    let analysis = parse(src.as_bytes()).unwrap();
    assert_eq!(analysis.boundaries.len(), 1);
    let item = &analysis.boundaries[0];
    assert_eq!(item.start_line, 6);
    assert_eq!(&src.as_bytes()[item.start_byte..item.end_byte], method.as_bytes());
    assert_eq!(item.branch_path[0].block_line, 1);
    assert!(analyze_corpus_refinements_v1(src.as_bytes()).is_err());

    let annotated_only = "&НаКлиенте\nПерем A;";
    assert!(parse(annotated_only.as_bytes()).unwrap().boundaries.is_empty());
    for bad in [
        "&НаКлиенте\nПерем A=1;",
        "&НаКлиенте\nПерем A, B Экспорт;",
        "&НаКлиенте\nПерем A, a;",
        "&НаКлиенте\nПерем A,;",
        "&НаКлиенте\nПерем A",
        "&НаКлиенте\nПерем A; Extra;",
        "&НаКлиенте\nПерем A,\n B;",
        "&НаКлиенте\n\nПерем A;",
        "&НаКлиенте\n// comment\nПерем A;",
        "&НаСервереБезКонтекста\nПерем A;",
        "&НаКлиенте\nПерем Если;",
        "&НаКлиенте\nПерем A; Процедура Fake()",
        "Процедура P()\n&НаКлиенте\nПерем A;\nКонецПроцедуры",
    ] {
        let src = format!("Процедура Before()\nКонецПроцедуры\n{bad}\nПроцедура After()\nКонецПроцедуры");
        assert!(parse(src.as_bytes()).is_err(), "{bad}");
    }
    let names = (1..=65).map(|i| format!("A{i}")).collect::<Vec<_>>().join(",");
    assert_eq!(
        parse(format!("&НаКлиенте\nПерем {names};").as_bytes()).unwrap_err().reason,
        bslens_core::lexical::Reason::Limit
    );
}

#[test]
fn compact_dates_and_dotted_procedure_leave_boundaries_exact() {
    use bslens_core::lexical::{analyze_annotated_vars_v1 as old, analyze_dates_members_v1 as parse, Reason};
    let header = "Процедура P(\r\n A='20240229',\r\n B='202402291201') Экспорт";
    let method = format!("{header}\r\nX=Receiver.Процедура;\r\nY=Container[0].Процедура;\r\nZ='20240229120102';\r\nT=\"'foo' X..Процедура\"; // bad ' date\r\nКонецПроцедуры");
    let src = format!("\u{feff}#Если Сервер Тогда\r\n{method}\r\n#КонецЕсли");
    let result = parse(src.as_bytes()).unwrap();
    assert_eq!(result.boundaries.len(), 1);
    let m = &result.boundaries[0];
    assert_eq!((m.start_line, m.header_end_line, m.end_line), (2, 4, 9));
    assert_eq!(m.branch_path[0].block_line, 1);
    assert_eq!(&src.as_bytes()[m.header_start_byte..m.header_end_byte], header.as_bytes());
    assert_eq!(&src.as_bytes()[m.start_byte..m.end_byte], method.as_bytes());
    assert_eq!(old(src.as_bytes()).unwrap_err().reason, Reason::UnsupportedCharacter);
    assert_eq!(old("X=Receiver.Процедура;".as_bytes()).unwrap_err().reason, Reason::AmbiguousBoundary);
    assert!(parse(b"X=\"20240229\"; // 'foo'").is_ok());
    // This pass checks the spelling, not the calendar or runtime value.
    assert!(parse(b"X='99999999';").is_ok());

    for bad in [
        "X='';", "X='2024022910';", "X='202402291201022';", "X='２０２４０２２９';",
        "X='2024-02-29';", "X='2024\n0229';", "X='20240229", "X='20240229''20240229';",
        "X=\"text\"'20240101';", "X='20240101'\"text\";",
        "X=A'20240229';", "X='20240229'A;", "X='20240229'2;",
    ] {
        assert_eq!(parse(bad.as_bytes()).unwrap_err().reason, Reason::UnsupportedCharacter, "{bad}");
    }
    for bad in [
        ".Процедура;", "Obj..Процедура;", "Obj .Процедура;", "Obj. Процедура;",
        "Obj.\nПроцедура;", "Obj.Процедура P()", "Obj.КонецПроцедуры;",
        "Obj.Функция;", "Obj.Процедура..Other;", "Obj..Inner.Процедура;", "Если.Процедура;",
        "X=Obj.Процедура; Процедура Fake()", "Процедура Outer()\nПроцедура P()",
        "Процедура Bad(Процедура)\nКонецПроцедуры",
    ] {
        assert!(parse(bad.as_bytes()).is_err(), "{bad}");
    }
}

#[test]
fn async_headers_and_closed_casefold_keep_exact_ranges() {
    use bslens_core::lexical::{analyze_async_casing_v1 as parse, analyze_dates_members_v1 as old, Reason};
    let header = "Асинх Функция P(Знач A='20240229') Экспорт";
    let method = format!("{header}\r\nВозврат Ждать Call(A);\r\nКонецФункции");
    let directive = "#Если нЕ (вЕбКлИеНт иЛи сЕрВеР) Тогда";
    let prefix = format!("\u{feff}{directive}\r\n&НаКлиенте\r\n  ");
    let source = format!("{prefix}{method}\r\n#КонецЕсли");
    let result = parse(source.as_bytes()).unwrap();
    assert_eq!(result.boundaries.len(), 1);
    let item = &result.boundaries[0];
    assert_eq!(item.kind, "function");
    assert_eq!((item.start_line, item.header_end_line, item.end_line), (3, 3, 5));
    assert_eq!(item.start_byte, prefix.len());
    assert_eq!(item.header_start_byte, prefix.len());
    assert_eq!(&source.as_bytes()[item.start_byte..item.end_byte], method.as_bytes());
    assert_eq!(&source.as_bytes()[item.header_start_byte..item.header_end_byte], header.as_bytes());
    let b = &item.branch_path[0];
    assert_eq!((b.block_line, b.arm), (1, 0));
    assert_eq!(&source.as_bytes()[b.directive_start_byte..b.directive_end_byte], directive.as_bytes());
    assert_eq!(old(source.as_bytes()).unwrap_err().reason, Reason::DirectiveUnsupported);
    assert_eq!(old(method.as_bytes()).unwrap_err().reason, Reason::AsyncUnsupported);
    assert!(parse(method.as_bytes()).is_ok());

    let hidden = "X=\"Асинх Функция Fake()\"; // Асинх Процедура Fake()";
    assert!(parse(hidden.as_bytes()).unwrap().boundaries.is_empty());
    for bad in [
        "&НаСервере\nАсинх Процедура P()\nКонецПроцедуры",
        "&НаСервереБезКонтекста\nАсинх Процедура P()\nКонецПроцедуры",
        "&НаКлиенте\n// gap\nАсинх Процедура P()\nКонецПроцедуры",
        "Асинх\nПроцедура P()\nКонецПроцедуры",
        "Асинх Процедура P(A,\n B)\nКонецПроцедуры",
        "Асинх Асинх Процедура P()\nКонецПроцедуры",
        "Асинх Процедура Асинх()\nКонецПроцедуры",
        "Асинх Процедура P(A=Call())\nКонецПроцедуры",
        "Асинх Процедура P();\nКонецПроцедуры",
        "Асинх Процедура P() // unsupported declaration tail\nКонецПроцедуры",
        "Асинх Процедура P()\nКонецФункции",
        "X=1; Асинх Процедура P()\nКонецПроцедуры",
        "Процедура Outer()\nАсинх Процедура P()\nКонецПроцедуры\nКонецПроцедуры",
        "#если не Сервер Тогда\nПроцедура P()\nКонецПроцедуры\n#КонецЕсли",
        "#Если не Сервер тогда\nПроцедура P()\nКонецПроцедуры\n#КонецЕсли",
        "#Если не МобильноеПриложениеКлиент Тогда\nПроцедура P()\nКонецПроцедуры\n#КонецЕсли",
        "#Если нe Сервер Тогда\nПроцедура P()\nКонецПроцедуры\n#КонецЕсли",
        "#Если сервер иЛи Тогда\nПроцедура P()\nКонецПроцедуры\n#КонецЕсли",
    ] {
        assert!(parse(bad.as_bytes()).is_err(), "{bad}");
    }
}

#[test]
fn async_header_budget_includes_modifier_and_prefix_gap() {
    use bslens_core::lexical::{analyze_async_casing_v1 as parse, analyze_dates_members_v1 as old, Reason};
    let parameters = (0..64).map(|i| {
        if i < 34 { format!("P{i}=0") } else { format!("P{i}") }
    }).collect::<Vec<_>>();
    // 4 declaration tokens + 64 names + 63 commas + 68 default tokens
    // = 199; the Async modifier makes it exactly 200.
    let accepted = format!("Асинх Процедура F({})\nКонецПроцедуры", parameters.join(","));
    assert_eq!(parse(accepted.as_bytes()).unwrap().boundaries.len(), 1);
    let mut one_more = parameters;
    one_more[63] = format!("Знач {}", one_more[63]);
    let plain = format!("Процедура F({})\nКонецПроцедуры", one_more.join(","));
    assert_eq!(old(plain.as_bytes()).unwrap().boundaries.len(), 1);
    let too_many = format!("Асинх {plain}");
    assert_eq!(parse(too_many.as_bytes()).unwrap_err().reason, Reason::Limit);

    let suffix = "Процедура F()";
    // Budget excludes the one-byte method name but includes the async token
    // and every byte between it and the closing delimiter.
    let exact_gap = 8192 + "F".len() - "Асинх".len() - suffix.len();
    let source = |n| format!("Асинх{}{suffix}\nКонецПроцедуры", " ".repeat(n));
    let exact = source(exact_gap);
    let one = parse(exact.as_bytes()).unwrap().boundaries.remove(0);
    assert_eq!(one.header_end_byte - one.header_start_byte, 8193);
    let over = source(exact_gap + 1);
    assert_eq!(parse(over.as_bytes()).unwrap_err().reason, Reason::Limit);
}

#[test]
fn mobile_atoms_and_wrapped_variables_are_bounded_and_lexical() {
    use bslens_core::lexical::{analyze_async_casing_v1 as old, analyze_mobile_vars_v1 as parse, Reason};
    let method = "Процедура After()\r\nКонецПроцедуры";
    let directive = "#ИначеЕсли нЕ моБильныйАвтономныйСервер Или МобильноеПриложениеКлиент Тогда";
    let source = format!("\u{feff}#Если МобильноеПриложениеСервер Тогда\r\n{directive}\r\n&НаКлиенте\r\nПерем A, B,\r\n C, D,\r\n E, F;\r\n{method}\r\n#КонецЕсли");
    let result = parse(source.as_bytes()).unwrap();
    assert_eq!(result.boundaries.len(), 1);
    let item = &result.boundaries[0];
    assert_eq!((item.start_line, item.header_end_line, item.end_line), (7, 7, 8));
    assert_eq!(&source.as_bytes()[item.start_byte..item.end_byte], method.as_bytes());
    let b = &item.branch_path[0];
    assert_eq!((b.block_line, b.arm_line, b.arm), (1, 2, 1));
    assert_eq!(&source.as_bytes()[b.directive_start_byte..b.directive_end_byte], directive.as_bytes());
    assert_eq!(old(source.as_bytes()).unwrap_err().reason, Reason::DirectiveUnsupported);

    let list_only = "&НаСервере\nПерем A, B, C,\n D, E,\n F, G,\n H,\n I;";
    assert!(parse(list_only.as_bytes()).unwrap().boundaries.is_empty());
    assert_eq!(old(list_only.as_bytes()).unwrap_err().reason, Reason::AnnotationUnsupported);
    let limit = "&НаКлиенте\nПерем A,\n B,\n C,\n D,\n E,\n F;";
    assert_eq!(parse(limit.as_bytes()).unwrap_err().reason, Reason::Limit);
    let wrapped_head = "Перем A,\n";
    let wrapped_tail = "B;";
    let exact_gap = 8192 - wrapped_head.len() - wrapped_tail.len();
    let with_gap = |n| format!("&НаКлиенте\n{wrapped_head}{}{wrapped_tail}", " ".repeat(n));
    assert!(parse(with_gap(exact_gap).as_bytes()).is_ok());
    assert_eq!(parse(with_gap(exact_gap + 1).as_bytes()).unwrap_err().reason, Reason::Limit);
    let names = (1..64).map(|i| format!("X{i}")).collect::<Vec<_>>().join(",");
    assert!(parse(format!("&НаКлиенте\nПерем {names},\n X64;").as_bytes()).is_ok());
    assert_eq!(
        parse(format!("&НаКлиенте\nПерем {names},\n X64,X65;").as_bytes()).unwrap_err().reason,
        Reason::Limit,
    );
    for invalid in [
        "&НаКлиенте",
        "&НаКлиенте\nПерем A,",
        "&НаКлиенте\nПерем A,\n B,",
        "&НаКлиенте\nПерем A,\n\n B;",
        "&НаКлиенте\nПерем A, // no\n B;",
        "&НаКлиенте\nПерем A,\n B, // no\n C;",
        "&НаКлиенте\nПерем A,\n B; // no",
        "&НаКлиенте\nПерем A,\n ,B;",
        "&НаКлиенте\nПерем A,\n a;",
        "&НаКлиенте\nПерем A,\n B Экспорт;",
        "&НаКлиенте\nПерем A,\n B=0;",
        "&НаКлиенте\nПерем A,\n Перем B;",
        "&НаКлиенте\nПерем A,\n Процедура P()\nКонецПроцедуры",
        "&НаКлиенте\nПерем A,\n #Если МобильноеПриложениеКлиент Тогда\n B;\n#КонецЕсли",
        "#Если Сервер Тогда\n&НаКлиенте\nПерем A,\n#Иначе\n B;\n#КонецЕсли",
        "#Если МобильноеПриложениеФон Тогда\nПроцедура P()\nКонецПроцедуры\n#КонецЕсли",
        "#Если МобильныйАвтономныйСервер И Тогда\nПроцедура P()\nКонецПроцедуры\n#КонецЕсли",
    ] {
        assert!(parse(invalid.as_bytes()).is_err(), "{invalid}");
    }
}

#[test]
fn method_regions_are_owned_and_do_not_cross_methods_or_branches() {
    use bslens_core::lexical::{analyze_method_regions_v1 as parse, analyze_mobile_vars_v1 as old, Reason};
    let first = "Процедура P()\r\n#Область Local\r\nX=1;\r\n#КонецОбласти\r\nКонецПроцедуры";
    let second = "Процедура Q()\r\nКонецПроцедуры";
    let source = format!("\u{feff}#Область Outer\r\n#Если Сервер Тогда\r\n{first}\r\n{second}\r\n#КонецЕсли\r\n#КонецОбласти");
    let mut result = parse(source.as_bytes()).unwrap();
    result.boundaries.sort_by_key(|b| b.start_line);
    assert_eq!(result.boundaries.len(), 2);
    for (b, expected, start, end) in [(&result.boundaries[0], first, 3, 7), (&result.boundaries[1], second, 8, 9)] {
        assert_eq!((b.start_line, b.end_line), (start, end));
        assert_eq!(&source.as_bytes()[b.start_byte..b.end_byte], expected.as_bytes());
        assert_eq!(b.branch_path.len(), 1);
        assert_eq!((b.branch_path[0].block_line, b.branch_path[0].arm), (2, 0));
    }
    assert_eq!(old(source.as_bytes()).unwrap_err().reason, Reason::RegionInsideMethod);
    let outer_only = format!("#Область Outer\r\n{second}\r\n#КонецОбласти");
    let prev = old(outer_only.as_bytes()).unwrap().boundaries.remove(0);
    let next = parse(outer_only.as_bytes()).unwrap().boundaries.remove(0);
    assert_eq!(prev, next);

    let two_sequential = "Процедура P()\n#Область X\n#КонецОбласти\n#Область Y\n#КонецОбласти\nКонецПроцедуры";
    assert_eq!(parse(two_sequential.as_bytes()).unwrap().boundaries.len(), 1);
    let outer_comments = "Процедура P()\nКонецПроцедуры";
    let outer_comments = format!("#Область Outer // still accepted\n{outer_comments}\n#КонецОбласти // still accepted");
    assert_eq!(old(outer_comments.as_bytes()).unwrap(), parse(outer_comments.as_bytes()).unwrap());
    let bounded_arm = "Процедура P()\n#Если Сервер Тогда\n#Область Local\n#КонецОбласти\n#Иначе\nX=0;\n#КонецЕсли\nКонецПроцедуры";
    assert_eq!(parse(bounded_arm.as_bytes()).unwrap().boundaries.len(), 1);
    for invalid in [
        "Процедура P()\n#Область A\n#Область B\n#КонецОбласти\n#КонецОбласти\nКонецПроцедуры",
        "Процедура P()\n#Область A // no\n#КонецОбласти\nКонецПроцедуры",
        "Процедура P()\n#Область A\n#КонецОбласти // no\nКонецПроцедуры",
        "Процедура P()\n#Область A\nКонецПроцедуры\nПроцедура Q()\n#КонецОбласти\nКонецПроцедуры",
        "Процедура P()\n#Область A\nКонецПроцедуры\n#КонецОбласти",
        "#Область Outer\nПроцедура P()\n#КонецОбласти\nКонецПроцедуры",
        "Процедура P()\n#КонецОбласти\nКонецПроцедуры",
        "Процедура P()\n#Если Сервер Тогда\n#Область A\n#Иначе\n#КонецОбласти\n#КонецЕсли\nКонецПроцедуры",
        "Процедура P()\n#Если Сервер Тогда\n#Область A\n#КонецЕсли\n#КонецОбласти\nКонецПроцедуры",
        "Процедура P()\n#Область A\n#Если Сервер Тогда\n#КонецОбласти\n#КонецЕсли\nКонецПроцедуры",
        "Процедура P()\nX=(1+\n#Область A\n2);\n#КонецОбласти\nКонецПроцедуры",
    ] {
        assert!(parse(invalid.as_bytes()).is_err(), "{invalid}");
    }
}

#[test]
fn header_forms_are_bounded_and_keep_exact_async_prefix_and_end() {
    use bslens_core::lexical::{analyze_header_forms_v1 as parse, analyze_method_regions_v1 as old, Reason};
    let header = "Асинх Функция P(A=Null, // first\r\n B=\"https://example.invalid/\"\"q\"\"\", // middle\r\n C) Экспорт";
    let method = format!("{header} // last\r\nВозврат A;\r\nКонецФункции");
    let prefix = "\u{feff}#Если Сервер Тогда\r\n&НаКлиенте\r\n";
    let source = format!("{prefix}{method}\r\n#КонецЕсли");
    let mut result = parse(source.as_bytes()).unwrap();
    let item = result.boundaries.remove(0);
    assert!(result.boundaries.is_empty());
    assert_eq!(item.start_byte, prefix.len());
    assert_eq!((item.start_line, item.header_end_line, item.end_line), (3, 5, 7));
    assert_eq!(item.branch_path[0].block_line, 1);
    assert_eq!(&source.as_bytes()[item.header_start_byte..item.header_end_byte], header.as_bytes());
    assert_eq!(&source.as_bytes()[item.start_byte..item.end_byte], method.as_bytes());
    assert!(old(source.as_bytes()).is_err());
    let plain_null = "Процедура P(A=nUlL)\nКонецПроцедуры";
    assert!(parse(plain_null.as_bytes()).is_ok());
    assert!(old(plain_null.as_bytes()).is_err());
    let comment_only_extension = "Процедура P(A=0, // comment\n B=\"//literal\") // second\nКонецПроцедуры";
    assert!(parse(comment_only_extension.as_bytes()).is_ok());
    assert!(old(comment_only_extension.as_bytes()).is_err());

    for bad in [
        "Процедура P(A=NullValue)\nКонецПроцедуры",
        "Процедура P(A=Null+0)\nКонецПроцедуры",
        "Процедура P(A=-Null)\nКонецПроцедуры",
        "Асинх Процедура P(A=0, // c\n // no tokens\n B)\nКонецПроцедуры",
        "Асинх Процедура P(A=0, // c\n #Если Сервер Тогда\n B)\nКонецПроцедуры\n#КонецЕсли",
        "Асинх Процедура P(A, // c\n Процедура Fake()\nКонецПроцедуры",
        "Асинх Процедура P(A, // c\n КонецПроцедуры\n B)\nКонецПроцедуры",
        "Асинх Процедура P(A, // c\n B) ; // forbidden\nКонецПроцедуры",
        "Асинх Процедура P(A=\"unfinished //,\n B)\nКонецПроцедуры",
        "Асинх Процедура P(A=\"//\",\n B) #Иначе // not inside a comment\nКонецПроцедуры",
        "Асинх Процедура P(A=Null) // still single-line\nКонецПроцедуры",
    ] {
        assert!(parse(bad.as_bytes()).is_err(), "{bad}");
    }

    let continued = (1..=14).map(|i| format!(" A{i},\n")).collect::<String>();
    let make_lines = |extra| format!("Асинх Процедура P(\n{continued}{extra} Z)\nКонецПроцедуры");
    assert_eq!(parse(make_lines("").as_bytes()).unwrap().boundaries[0].header_end_line, 16);
    assert_eq!(parse(make_lines(" Extra,\n").as_bytes()).unwrap_err().reason, Reason::Limit);

    let head = "Асинх Процедура P(A,\n B) //";
    let tail_limit = 8192 + "P".len() - head.len();
    let with_comment = |n| format!("{head}{}\nКонецПроцедуры", "x".repeat(n));
    assert!(parse(with_comment(tail_limit).as_bytes()).is_ok());
    assert_eq!(parse(with_comment(tail_limit + 1).as_bytes()).unwrap_err().reason, Reason::Limit);
}

#[test]
fn source_limits_keep_comments_dates_and_other_budgets_exact() {
    use bslens_core::lexical::{analyze_source_limits_v1 as parse, analyze_header_forms_v1 as old, Reason};
    let method = "Процедура After()\r\nX='2024-02-29';\r\nКонецПроцедуры";
    let comments = " \t// #Если Сервер Тогда, fake declaration\r\n".repeat(16);
    let prefix = format!("\u{feff}&НаКлиенте\r\n{comments}");
    let source = format!("{prefix}{method}");
    let only = parse(source.as_bytes()).unwrap().boundaries.remove(0);
    assert_eq!((only.start_line, only.end_line), (18, 20));
    assert_eq!(only.start_byte, prefix.len());
    assert_eq!(&source.as_bytes()[only.start_byte..only.end_byte], method.as_bytes());
    assert!(old(source.as_bytes()).is_err());
    let after_var = format!("&НаКлиенте\r\n// Перем may follow\r\nПерем A;\r\n{method}");
    let var_then_method = parse(after_var.as_bytes()).unwrap();
    assert_eq!(var_then_method.boundaries.len(), 1);
    assert_eq!(var_then_method.boundaries[0].start_line, 4);
    assert_eq!(parse(format!("{prefix}// 17th\r\n{method}").as_bytes()).unwrap_err().reason, Reason::Limit);
    let with_body = |row| format!("Процедура P()\n{row}\nКонецПроцедуры");
    assert_eq!(old(with_body("X='2024-02-29';").as_bytes()).unwrap_err().reason, Reason::UnsupportedCharacter);
    assert!(parse(with_body("X='2024-02-29';").as_bytes()).is_ok());
    assert!(parse(with_body("X='9999-99-99';").as_bytes()).is_ok()); // shape, not calendar validity
    assert_eq!(parse(b"X='2024-02-29';").unwrap_err().reason, Reason::UnsupportedCharacter);
    // Existing compact date behavior remains unchanged in module code.
    assert_eq!(old(b"X='20240229';").unwrap(), parse(b"X='20240229';").unwrap());
    for invalid_body in [
        "X='2024--02-29';", "X='2024-2-29';", "X='2024/02/29';", "X='2024-02-2900';",
        "X='2024-02-29'Name;", "X=\"prefix\"'2024-02-29';", "X='2024-02-29'\"suffix\";",
    ] {
        assert_eq!(parse(with_body(invalid_body).as_bytes()).unwrap_err().reason, Reason::UnsupportedCharacter, "{invalid_body}");
    }
    for invalid in [
        "&НаКлиенте\n// only EOF",
        "&НаКлиенте\n// ok\n\nПроцедура P()\nКонецПроцедуры",
        "&НаКлиенте\n// ok\nX=1;\nПроцедура P()\nКонецПроцедуры",
        "&НаКлиенте\n// ok\n&НаКлиенте\nПроцедура P()\nКонецПроцедуры",
        "&НаКлиенте\n// ok\n#Область X\nПроцедура P()\nКонецПроцедуры\n#КонецОбласти",
        "&НаСервере\n// not observed\nПроцедура P()\nКонецПроцедуры",
        "&НаКлиенте\n// async adjacency unchanged\nАсинх Процедура P()\nКонецПроцедуры",
        "Процедура P(A='2024-02-29')\nКонецПроцедуры",
        "Процедура P(A,\n B='2024-02-29')\nКонецПроцедуры",
    ] {
        assert!(parse(invalid.as_bytes()).is_err(), "{invalid}");
    }

    let small_method = "Процедура After()\nКонецПроцедуры";
    let make = |n: usize| format!("//{}\n{small_method}", "x".repeat(n - 3 - small_method.len()));
    let exact = make(4*1024*1024);
    assert_eq!(exact.len(),4*1024*1024);
    let item = parse(exact.as_bytes()).unwrap().boundaries.remove(0);
    assert_eq!((item.start_line, item.end_line), (2, 3));
    assert_eq!(&exact.as_bytes()[item.start_byte..item.end_byte], small_method.as_bytes());
    assert_eq!(parse(make(4*1024*1024+1).as_bytes()).unwrap_err().reason, Reason::Limit);
    assert!(old(make(1024*1024).as_bytes()).is_ok());
    assert_eq!(old(make(1024*1024+1).as_bytes()).unwrap_err().reason, Reason::Limit);
    let over_lines = "\n".repeat(65537);
    let over_tokens = "+;".repeat(50001);
    assert_eq!(parse(over_lines.as_bytes()).unwrap_err().reason, Reason::Limit);
    assert_eq!(parse(over_tokens.as_bytes()).unwrap_err().reason, Reason::Limit);
}

#[test]
fn long_ordinary_headers_keep_line_and_formal_exception_bounded() {
    use bslens_core::lexical::{analyze_long_headers_v1 as parse, analyze_source_limits_v1 as old, Reason};
    let render = |lines: usize, head: &str, last: &str| {
        let middle = (1..=lines-2).map(|n| format!(" A{n},\r\n")).collect::<String>();
        format!("{head}(\r\n{middle} {last}) Экспорт\r\nКонецПроцедуры")
    };
    let short = render(16, "Процедура P", "Z");
    assert_eq!(parse(short.as_bytes()).unwrap(), old(short.as_bytes()).unwrap());
    for count in [17, 20] {
        let src = render(count, "Процедура P", "Z");
        let b = parse(src.as_bytes()).unwrap().boundaries.remove(0);
        assert_eq!((b.start_line, b.header_end_line, b.end_line), (1,count,count+1));
        assert_eq!(&src.as_bytes()[b.start_byte..b.end_byte], src.as_bytes());
        assert_eq!(old(src.as_bytes()).unwrap_err().reason, Reason::Limit);
    }
    let wide = "Я".repeat(65);
    assert_eq!(wide.len(),130);
    let accept = render(20, "Процедура P", &format!("{wide}=Null"));
    assert!(parse(accept.as_bytes()).is_ok());
    let default_too_long = format!("Z=\"{}\"", "x".repeat(128));
    for src in [
        render(21, "Процедура P", "Z"),
        render(17, "Асинх Процедура P", "Z"),
        render(16, "Процедура P", &format!("{wide}=0")),
        render(20, "Процедура P", &wide),
        render(20, "Процедура P", &format!("{wide}a=0")),
        render(20, "Процедура P", &default_too_long),
    ] {
        assert_eq!(parse(src.as_bytes()).unwrap_err().reason, Reason::Limit);
    }
    for src in [
        render(20, "Процедура P", &format!("{wide}=Call()")),
        render(20, "Процедура P", "Z=#Если Сервер Тогда"),
        render(20, "Процедура P", "Z=0; Процедура Q()"),
    ] {
        assert!(parse(src.as_bytes()).is_err());
    }
    // The pre-existing method-name exception is not the new formal-name rule.
    let wide_method = format!("Процедура {}()\nКонецПроцедуры", "Я".repeat(76));
    assert_eq!(parse(wide_method.as_bytes()).unwrap(), old(wide_method.as_bytes()).unwrap());
    let mut line = render(20, "Процедура P", "Z");
    let end = line.find(" Экспорт").unwrap() + " Экспорт".len();
    line.insert_str(end, &format!(" //{}", "x".repeat(8200)));
    assert_eq!(parse(line.as_bytes()).unwrap_err().reason, Reason::Limit);
}

#[test]
fn lexical_tail_selectors_are_narrow_and_do_not_create_methods() {
    use bslens_core::lexical::{analyze_lexical_tail_v1 as parse, analyze_long_headers_v1 as old, Reason};
    let render = |body| format!("Процедура P()\r\n{body}\r\nКонецПроцедуры");
    let src = render("A.B.Процедура.Field = V;");
    assert_eq!(old(src.as_bytes()).unwrap_err().reason, Reason::AmbiguousBoundary);
    let analysis = parse(src.as_bytes()).unwrap();
    assert_eq!(analysis.boundaries.len(), 1);
    let first = &analysis.boundaries[0];
    assert_eq!((first.name.as_str(), first.start_line, first.end_line), ("p",1,3));
    assert_eq!(&src.as_bytes()[first.start_byte..first.end_byte], src.as_bytes());
    let quoted = render(r#"A.B.Процедура.Field = "Процедура ""Байт""; //";"#);
    assert_eq!(old(quoted.as_bytes()).unwrap_err().reason, Reason::AmbiguousBoundary);
    let analysis = parse(quoted.as_bytes()).unwrap();
    assert_eq!(analysis.boundaries.len(), 1);
    let first = &analysis.boundaries[0];
    assert_eq!((first.name.as_str(), first.start_line, first.end_line), ("p",1,3));
    assert_eq!(&quoted.as_bytes()[first.start_byte..first.end_byte], quoted.as_bytes());
    for bad in [
        r#"A.B.Процедура.Field = "a" + "b";"#,
        r#"A.B.Процедура.Field = "a";;"#,
        r#"A.B.Процедура.Field = "a; "#,
        "A.B.Процедура.Field = '20240229';",
        "A.B.Процедура.Field = 1;",
        "A.B.Процедура .Field = V;", "A.B.Процедура. Field = V;",
        "A.B.Процедура.Field.SubField = V;", "A.B.Процедура. = V;",
        "A.B.Процедура.Field = ;", "A.B.Процедура.Field = V",
        "A.B.Процедура.КонецПроцедуры = V;", "A..Процедура.Field = V;",
        "A.B.Функция.Field = V;", "A.B.Процедура.Field = V; Процедура Q()",
    ] {
        let s = render(bad);
        assert_eq!(parse(s.as_bytes()).unwrap_err().reason, Reason::AmbiguousBoundary, "{bad}");
    }
    let outside = "A.B.Процедура.Field = V;\nПроцедура P()\nКонецПроцедуры";
    assert_eq!(parse(outside.as_bytes()).unwrap_err().reason, Reason::AmbiguousBoundary);
    let old_witness = render("A.B.Процедура = V;");
    assert_eq!(parse(old_witness.as_bytes()).unwrap(), old(old_witness.as_bytes()).unwrap());
}

#[test]
fn lexical_tail_token_budget_is_explicit_and_size_gated() {
    use bslens_core::lexical::{
        analyze_lexical_tail_v1 as parse, analyze_long_headers_v1 as old,
        LEXICAL_TAIL_LARGE_TOKEN_THRESHOLD_BYTES as threshold, Reason,
    };
    let source = |total_bytes: usize, tokens: usize| {
        let header = "Процедура P()\n";
        let close = "КонецПроцедуры";
        let fuel = ";".repeat(tokens - 5);
        let body = fuel.as_bytes().chunks(1000).map(|c| {
            std::str::from_utf8(c).unwrap()
        }).collect::<Vec<_>>().join("\n");
        let body = format!("{header}{body}\n{close}");
        format!("//{}\n{body}", "x".repeat(total_bytes - 3 - body.len()))
    };
    let small_exact = source(threshold, 100000);
    assert_eq!(old(small_exact.as_bytes()).unwrap(), parse(small_exact.as_bytes()).unwrap());
    let small_over = source(threshold, 100001);
    assert_eq!(parse(small_over.as_bytes()).unwrap_err().reason, Reason::Limit);
    let larger = source(threshold + 1, 200000);
    let b = parse(larger.as_bytes()).unwrap().boundaries.remove(0);
    assert_eq!(b.start_line, 2);
    assert_eq!(b.end_byte, larger.len());
    assert_eq!(old(larger.as_bytes()).unwrap_err().reason, Reason::Limit);
    let larger_over = source(threshold + 1, 200001);
    assert_eq!(parse(larger_over.as_bytes()).unwrap_err().reason, Reason::Limit);
}
