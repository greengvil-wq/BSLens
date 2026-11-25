//! Общий контракт S3/S4. Вызывающий отвечает за доверенную приватную копию и транзакцию.
use super::{Error, Result};
use rusqlite::{types::ValueRef, Connection};
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = include_str!("../../spikes/linux-v5/schema.sql");
pub const PROFILE: &str = "linux-v5-spike-xml221-bsl-r09";
pub const SCOPE: &str = "xml221-r09-all-xml-bsl";
pub const CORE: &str = env!("CARGO_PKG_VERSION");
pub const ADAPTER: &str = "xml221-r09";
pub const LEXICAL: &str = "r09-regions";

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn frame(h: &mut Sha256, tag: u8, bytes: &[u8]) {
    h.update([tag]);
    h.update((bytes.len() as u64).to_be_bytes());
    h.update(bytes);
}
fn table(h: &mut Sha256, db: &Connection, name: &str, sql: &str) -> Result<()> {
    frame(h, b'T', name.as_bytes());
    let mut query = db.prepare(sql)?;
    let count = query.column_count();
    h.update((count as u64).to_be_bytes());
    for name in query.column_names() {
        frame(h, b'F', name.as_bytes());
    }
    let mut rows = query.query([])?;
    while let Some(row) = rows.next()? {
        h.update(b"R");
        for i in 0..count {
            match row.get_ref(i)? {
                ValueRef::Null => h.update(b"N"),
                ValueRef::Integer(n) => {
                    h.update(b"I");
                    h.update(n.to_be_bytes());
                }
                ValueRef::Text(t) => {
                    std::str::from_utf8(t).map_err(|_| Error::Contract("canonical_utf8"))?;
                    frame(h, b'S', t);
                }
                _ => return Err(Error::Contract("canonical_type")),
            }
        }
    }
    h.update(b"E");
    Ok(())
}

/// Хеш логических фактов; ID заменяются стабильными XML/BSL-ключами.
/// Перед вызовом: validate_schema_profile и одна read-транзакция на стороне S4.
pub fn facts_sha256(db: &Connection) -> Result<String> {
    if db
        .prepare("PRAGMA foreign_key_check")?
        .query([])?
        .next()?
        .is_some()
    {
        return Err(Error::Contract("foreign_key"));
    }
    let mut h = Sha256::new();
    let inline:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='inline_declarations')",[],|r|r.get(0))?;
    let companions: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='companions')",
        [],
        |r| r.get(0),
    )?;
    let journals: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='inline_module_owners')",
        [],
        |r| r.get(0),
    )?;
    let diagnostics: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='xml_diagnostics')",
        [],
        |r| r.get(0),
    )?;
    let source_anomalies: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='bsl_source_anomalies')",
        [],
        |r| r.get(0),
    )?;
    if source_anomalies {
        h.update(b"BSLens-v5/facts/source-anomalies1\0");
    }
    let conditional: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='method_branches')",
        [],
        |r| r.get(0),
    )?;
    if conditional {
        h.update(b"BSLens-v5/facts/conditional1\0");
    }
    if diagnostics {
        h.update(b"BSLens-v5/facts/diagnostics1\0");
        let summary = super::diagnostics_writer::summarize(
            db,
            super::diagnostics_writer::MAX_BYTES,
            2_000_000,
        )?;
        frame(&mut h, b'D', summary.to_string().as_bytes());
    } else if journals {
        h.update(b"BSLens-v5/facts/journal1\0");
    } else if companions {
        h.update(b"BSLens-v5/facts/companion1\0");
    } else if inline {
        h.update(b"BSLens-v5/facts/inline1\0");
    } else {
        h.update(b"BSLens-v5/facts/1\0");
    }
    table(&mut h, db, "objects", "SELECT o.xml_path AS xml_path,p.xml_path AS parent_xml_path,o.kind AS kind,o.name AS name,o.source_kind AS source_kind FROM objects o LEFT JOIN objects p ON p.object_id=o.parent_id ORDER BY o.xml_path COLLATE BINARY")?;
    table(&mut h, db, "modules", "SELECT m.path AS path,m.source_kind AS source_kind,o.xml_path AS owner_xml_path,m.parse_status AS parse_status,m.reason_code AS reason_code FROM modules m LEFT JOIN objects o ON o.object_id=m.owner_id ORDER BY m.path COLLATE BINARY")?;
    table(&mut h, db, "methods", "SELECT module_path,parse_status,kind,name,exported,start_line,end_line,byte_start,byte_end FROM methods ORDER BY module_path COLLATE BINARY,start_line")?;
    table(&mut h, db, "source_lines", "SELECT path,source_kind,line_number,byte_start,byte_end FROM source_lines ORDER BY path COLLATE BINARY,line_number")?;
    if inline {
        table(&mut h,db,"inline_declarations","SELECT d.declaration_key,o.xml_path AS owner_xml_path,d.parent_key,d.kind,d.name,d.uuid,d.xml_path,d.byte_start,d.byte_end FROM inline_declarations d JOIN objects o ON o.object_id=d.owner_id ORDER BY d.declaration_key COLLATE BINARY")?;
    }
    if companions {
        table(&mut h,db,"companions","SELECT o.xml_path,c.reference_path,c.byte_start,c.byte_end FROM companions c JOIN objects o ON o.object_id=c.object_id ORDER BY o.xml_path COLLATE BINARY")?;
        table(&mut h,db,"companion_payloads","SELECT p.path,o.xml_path,p.role FROM companion_payloads p JOIN objects o ON o.object_id=p.object_id ORDER BY p.path COLLATE BINARY")?;
    }
    if journals {
        table(&mut h,db,"inline_module_owners","SELECT m.module_path,m.declaration_key,o.xml_path FROM inline_module_owners m JOIN objects o ON o.object_id=m.owner_id ORDER BY m.module_path COLLATE BINARY")?;
    }
    if conditional {
        table(&mut h,db,"method_branches","SELECT m.module_path,m.start_line,b.branch_path FROM method_branches b JOIN methods m ON m.method_id=b.method_id ORDER BY m.module_path COLLATE BINARY,m.start_line")?;
    }
    if source_anomalies {
        table(&mut h, db, "bsl_source_anomalies",
            "SELECT module_path,code,physical_line,byte_start,byte_end FROM bsl_source_anomalies ORDER BY module_path COLLATE BINARY")?;
    }
    Ok(hex(&h.finalize()))
}

/// Входы сборки, без даты, фактов и физического SQLite-файла.
pub fn snapshot_id(db: &Connection) -> Result<String> {
    metadata_bounds(db)?;
    let raw: String = db.query_row(
        "SELECT build_parameters FROM metadata WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    let json: serde_json::Value = serde_json::from_str(&raw)?;
    if !json.is_object() || serde_json::to_string(&json)? != raw {
        return Err(Error::Contract("parameters_not_normalized"));
    }
    let mut h = Sha256::new();
    h.update(b"BSLens-v5/inputs/1\0");
    table(&mut h, db, "inputs", "SELECT source_id,commit_oid,tree_oid,schema_version,core_version,adapter_version,xml_version,lexical_version,profile,scope_id,build_parameters FROM metadata WHERE singleton=1")?;
    Ok(format!("bslens-v5-{}", hex(&h.finalize())))
}

// Только доверенный эталон собирается в Vec; недоверенный каталог — никогда.
fn check_catalog(db: &Connection, reference: &Connection) -> Result<()> {
    let expected = reference
        .prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")?
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let count: i64 = db.query_row(
        "SELECT count(*) FROM (SELECT 1 FROM sqlite_schema LIMIT ?1)",
        [expected.len() as i64 + 1],
        |r| r.get(0),
    )?;
    if count != expected.len() as i64 {
        return Err(Error::Contract("schema_count"));
    }
    let maxima = [
        expected.iter().map(|r| r.0.len()).max().unwrap_or(0),
        expected.iter().map(|r| r.1.len()).max().unwrap_or(0),
        expected.iter().map(|r| r.2.len()).max().unwrap_or(0),
        expected
            .iter()
            .map(|r| r.3.as_ref().map_or(0, String::len))
            .max()
            .unwrap_or(0),
    ];
    // Проверка байтовых длин до сортировки/выдачи текста. CAST AS BLOB учитывает NUL.
    for (i, column) in ["type", "name", "tbl_name", "sql"].iter().enumerate() {
        let invalid:i64=db.query_row(&format!(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE (typeof({column})!='text' AND NOT (?2 AND {column} IS NULL)) OR length(CAST({column} AS BLOB))>?1 LIMIT 1)"),
            rusqlite::params![maxima[i] as i64,i==3],|r|r.get(0))?;
        if invalid != 0 {
            return Err(Error::Contract("schema_field_budget"));
        }
    }
    let mut query =
        db.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")?;
    let mut rows = query.query([])?;
    for (kind, name, table, sql) in &expected {
        let row = rows.next()?.ok_or(Error::Contract("schema"))?;
        for (i, value) in [
            Some(kind.as_str()),
            Some(name.as_str()),
            Some(table.as_str()),
            sql.as_deref(),
        ]
        .iter()
        .enumerate()
        {
            let matches = match (row.get_ref(i)?, value) {
                (ValueRef::Null, None) => true,
                (ValueRef::Text(actual), Some(expected)) => actual == expected.as_bytes(),
                _ => false,
            };
            if !matches {
                return Err(Error::Contract("schema"));
            }
        }
    }
    if rows.next()?.is_some() {
        return Err(Error::Contract("schema"));
    }
    Ok(())
}

// Бюджеты чтения текущего экспериментального контракта, не лимиты DDL/корпуса.
// Только скалярные результаты SQL до копирования String или разбора JSON.
fn metadata_bounds(db: &Connection) -> Result<()> {
    let count: i64 = db.query_row(
        "SELECT count(*) FROM (SELECT 1 FROM metadata LIMIT 2)",
        [],
        |r| r.get(0),
    )?;
    if count != 1 {
        return Err(Error::Contract("metadata_count"));
    }
    for (field, maximum, nullable) in [
        ("snapshot_id", 74, false),
        ("built_at_utc", 20, false),
        ("facts_sha256", 64, true),
        ("source_id", 1024, false),
        ("commit_oid", 64, false),
        ("tree_oid", 64, false),
        ("core_version", 128, false),
        ("adapter_version", 128, false),
        ("xml_version", 16, false),
        ("lexical_version", 128, false),
        ("profile", 64, false),
        ("scope_id", 1024, false),
        ("relations", 32, false),
        ("build_parameters", 16 * 1024, false),
        ("coverage_draft", 1024 * 1024, false),
        ("limits_draft", 4096, false),
        ("input_manifest", 16 * 1024 * 1024, false),
    ] {
        let invalid:i64=db.query_row(&format!(
            "SELECT EXISTS(SELECT 1 FROM metadata WHERE (typeof({field})!='text' AND NOT (?2 AND {field} IS NULL)) OR length(CAST({field} AS BLOB))>?1 LIMIT 1)"),
            rusqlite::params![maximum,nullable],|r|r.get(0))?;
        if invalid != 0 {
            return Err(Error::Contract("metadata_field_budget"));
        }
    }
    Ok(())
}
/// Точное определение схемы S2, user_version, единственная metadata и известный профиль.
/// Не открывает путь, не пересчитывает BSL и не создаёт ValidatedSnapshot.
pub fn validate_schema_profile(db: &Connection) -> Result<()> {
    if db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? != 5 {
        return Err(Error::Contract("user_version"));
    }
    let mut matched = None;
    let mut rejection = Error::Contract("schema_count");
    for (profile, ddl) in [
        (PROFILE, SCHEMA.to_owned()),
        (
            super::string_writer::PROFILE,
            super::string_writer::schema(),
        ),
        (
            super::literal_writer::PROFILE,
            super::literal_writer::schema(),
        ),
        (
            super::wrapped_writer::PROFILE,
            super::wrapped_writer::schema(),
        ),
        (
            super::refined_writer::PROFILE,
            super::refined_writer::schema(),
        ),
        (
            super::annotated_vars_writer::PROFILE,
            super::annotated_vars_writer::schema(),
        ),
        (
            super::dates_members_writer::PROFILE,
            super::dates_members_writer::schema(),
        ),
        (
            super::async_casing_writer::PROFILE,
            super::async_casing_writer::schema(),
        ),
        (
            super::mobile_vars_writer::PROFILE,
            super::mobile_vars_writer::schema(),
        ),
        (
            super::method_regions_writer::PROFILE,
            super::method_regions_writer::schema(),
        ),
        (
            super::header_forms_writer::PROFILE,
            super::header_forms_writer::schema(),
        ),
        (
            super::source_limits_writer::PROFILE,
            super::source_limits_writer::schema(),
        ),
        (
            super::long_headers_writer::PROFILE,
            super::long_headers_writer::schema(),
        ),
        (
            super::lexical_tail_writer::PROFILE,
            super::lexical_tail_writer::schema(),
        ),
        (
            super::source_anomalies_writer::PROFILE,
            super::source_anomalies_writer::schema(),
        ),
        (
            super::navigator_agreement_writer::PROFILE,
            super::navigator_agreement_writer::schema(),
        ),
        (
            super::navigator_owners_writer::PROFILE,
            super::navigator_owners_writer::schema(),
        ),
        (
            super::conditional_writer::PROFILE,
            super::conditional_writer::schema(),
        ),
        (
            super::parameter_writer::PROFILE,
            super::parameter_writer::schema(),
        ),
        (
            super::context_writer::PROFILE,
            super::context_writer::schema(),
        ),
        (
            super::sidecar_writer::PROFILE,
            super::sidecar_writer::schema(),
        ),
        (
            super::optional_form_writer::PROFILE,
            super::optional_form_writer::schema(),
        ),
        (
            super::diagnostics_writer::PROFILE,
            super::diagnostics_writer::schema(),
        ),
        (
            super::corpus_config_writer::PROFILE,
            super::corpus_config_writer::schema(),
        ),
        (
            super::corpus_help_writer::PROFILE,
            super::corpus_help_writer::schema(),
        ),
        (
            super::corpus_bin_writer::PROFILE,
            super::corpus_bin_writer::schema(),
        ),
        (super::chart_writer::PROFILE, super::chart_writer::schema()),
        (
            super::corpus_writer::PROFILE,
            super::corpus_writer::schema(),
        ),
        (
            super::workflow_writer::PROFILE,
            super::workflow_writer::schema(),
        ),
        (
            super::report_writer::PROFILE,
            super::report_writer::schema(),
        ),
        (
            super::register_writer::PROFILE,
            super::register_writer::schema(),
        ),
        (
            super::service_writer::PROFILE,
            super::service_writer::schema(),
        ),
        (
            super::journal_writer::PROFILE,
            super::journal_writer::schema(),
        ),
        (super::leaf_writer::PROFILE, super::leaf_writer::schema()),
        (
            super::shared_writer::PROFILE,
            super::shared_writer::schema(),
        ),
        (super::root_writer::PROFILE, super::root_writer::schema()),
        (
            super::common_writer::PROFILE,
            super::common_writer::schema(),
        ),
        (
            super::companion_writer::PROFILE,
            super::companion_writer::schema(),
        ),
        (
            super::ingest_writer::PROFILE,
            super::ingest_writer::schema(),
        ),
        (
            super::streaming_writer::PROFILE,
            super::streaming_writer::schema(),
        ),
        (
            super::inline_writer::PROFILE,
            super::inline_writer::schema(),
        ),
    ] {
        let reference = Connection::open_in_memory()?;
        reference.execute_batch(&ddl)?;
        match check_catalog(db, &reference) {
            Ok(()) => {
                matched = Some(profile);
                break;
            }
            Err(e) => {
                if matches!(rejection, Error::Contract("schema_count")) {
                    rejection = e;
                }
            }
        }
    }
    let expected_profile = matched.ok_or(rejection)?;
    metadata_bounds(db)?;
    let row: (i64, i64, String, String) = db.query_row(
        "SELECT count(*),schema_version,profile,relations FROM metadata",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    if row.0 != 1 || row.1 != 5 || row.2 != expected_profile || row.3 != "not_analyzed" {
        return Err(Error::Contract("profile"));
    }
    if row.2 == super::annotated_vars_writer::PROFILE {
        super::annotated_vars_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::dates_members_writer::PROFILE {
        super::dates_members_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::async_casing_writer::PROFILE {
        super::async_casing_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::mobile_vars_writer::PROFILE {
        super::mobile_vars_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::method_regions_writer::PROFILE {
        super::method_regions_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::header_forms_writer::PROFILE {
        super::header_forms_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::source_limits_writer::PROFILE {
        super::source_limits_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::long_headers_writer::PROFILE {
        super::long_headers_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::lexical_tail_writer::PROFILE {
        super::lexical_tail_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::source_anomalies_writer::PROFILE {
        super::source_anomalies_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::navigator_agreement_writer::PROFILE {
        super::navigator_agreement_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::navigator_owners_writer::PROFILE {
        super::navigator_owners_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::refined_writer::PROFILE {
        super::refined_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::wrapped_writer::PROFILE {
        super::wrapped_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::literal_writer::PROFILE {
        super::literal_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::string_writer::PROFILE {
        super::string_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::conditional_writer::PROFILE {
        super::conditional_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::parameter_writer::PROFILE {
        super::parameter_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::context_writer::PROFILE {
        super::context_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::sidecar_writer::PROFILE {
        super::sidecar_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::optional_form_writer::PROFILE {
        super::optional_form_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::diagnostics_writer::PROFILE {
        super::diagnostics_writer::validate_metadata(db)?;
        super::diagnostics_writer::validate_saved(db)?;
    }
    if row.2 == super::corpus_config_writer::PROFILE {
        super::corpus_config_writer::validate_metadata(db)?;
    }
    if row.2 == super::corpus_help_writer::PROFILE {
        super::corpus_help_writer::validate_metadata(db)?;
    }
    if row.2 == super::corpus_bin_writer::PROFILE {
        super::corpus_bin_writer::validate_metadata(db)?;
    }
    if row.2 == super::corpus_writer::PROFILE {
        super::corpus_writer::validate_metadata(db)?;
    }
    if row.2 == super::workflow_writer::PROFILE {
        super::workflow_writer::validate_metadata(db)?;
    }
    if row.2 == super::report_writer::PROFILE {
        super::report_writer::validate_metadata(db)?;
    }
    if row.2 == super::register_writer::PROFILE {
        super::register_writer::validate_metadata(db)?;
    }
    if row.2 == super::chart_writer::PROFILE {
        super::chart_writer::validate_metadata(db)?;
    }
    if row.2 == super::service_writer::PROFILE {
        super::service_writer::validate_metadata(db)?;
    }
    if row.2 == super::journal_writer::PROFILE {
        super::journal_writer::validate_metadata(db)?;
    }
    if row.2 == super::leaf_writer::PROFILE {
        super::leaf_writer::validate_metadata(db)?;
    }
    if row.2 == super::shared_writer::PROFILE {
        super::shared_writer::validate_metadata(db)?;
    }
    if row.2 == super::root_writer::PROFILE {
        super::root_writer::validate_metadata(db)?;
    }
    if row.2 == super::common_writer::PROFILE {
        super::common_writer::validate_metadata(db)?;
    }
    if row.2 == super::companion_writer::PROFILE {
        super::companion_writer::validate_metadata(db)?;
    }
    if row.2 == super::inline_writer::PROFILE {
        super::inline_writer::validate_metadata(db)?;
    }
    if row.2 == super::streaming_writer::PROFILE {
        super::streaming_writer::validate_metadata(db)?;
    }
    if row.2 == super::ingest_writer::PROFILE {
        super::ingest_writer::validate_metadata(db)?;
    }
    Ok(())
}

/// Совместимость известного профиля. Другой непустой scope_id сам по себе не отказ.
/// Покрытие остаётся временным JSON и должно быть показано потребителю S4.
pub fn compatible_scope(db: &Connection) -> Result<()> {
    validate_schema_profile(db)?;
    let (core,adapter,xml,lexical,scope,coverage):(String,String,String,String,String,String)=db.query_row(
        "SELECT core_version,adapter_version,xml_version,lexical_version,scope_id,coverage_draft FROM metadata",[],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;
    let coverage: serde_json::Value = serde_json::from_str(&coverage)?;
    if core != CORE
        || adapter != ADAPTER
        || xml != "2.21"
        || lexical
            != if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::navigator_owners_writer::PROFILE
            {
                super::navigator_agreement_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::navigator_agreement_writer::PROFILE
            {
                super::navigator_agreement_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::source_anomalies_writer::PROFILE
            {
                super::source_anomalies_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::lexical_tail_writer::PROFILE
            {
                super::lexical_tail_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::long_headers_writer::PROFILE
            {
                super::long_headers_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::source_limits_writer::PROFILE
            {
                super::source_limits_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::header_forms_writer::PROFILE
            {
                super::header_forms_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::method_regions_writer::PROFILE
            {
                super::method_regions_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::mobile_vars_writer::PROFILE
            {
                super::mobile_vars_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::async_casing_writer::PROFILE
            {
                super::async_casing_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::dates_members_writer::PROFILE
            {
                super::dates_members_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::annotated_vars_writer::PROFILE
            {
                super::annotated_vars_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::refined_writer::PROFILE
            {
                super::refined_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::wrapped_writer::PROFILE
            {
                super::wrapped_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::literal_writer::PROFILE
            {
                super::literal_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::string_writer::PROFILE
            {
                super::string_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::conditional_writer::PROFILE
            {
                super::conditional_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::parameter_writer::PROFILE
            {
                super::parameter_writer::LEXICAL
            } else if db.query_row("SELECT profile FROM metadata", [], |r| {
                r.get::<_, String>(0)
            })? == super::context_writer::PROFILE
            {
                super::context_writer::LEXICAL
            } else {
                LEXICAL
            }
        || scope.is_empty()
        || coverage.get("temporary") != Some(&serde_json::Value::Bool(true))
    {
        return Err(Error::Contract("incompatible_profile"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn digest(sql: &str) -> String {
        let db = Connection::open_in_memory().unwrap();
        let mut h = Sha256::new();
        table(&mut h, &db, "test", sql).unwrap();
        hex(&h.finalize())
    }
    #[test]
    fn typed_frames_and_row_boundaries_do_not_alias() {
        assert_ne!(digest("SELECT NULL AS value"), digest("SELECT '' AS value"));
        assert_ne!(digest("SELECT 1 AS value"), digest("SELECT '1' AS value"));
        assert_ne!(
            digest("SELECT 'ab' AS a,'c' AS b"),
            digest("SELECT 'a' AS a,'bc' AS b")
        );
        assert_ne!(
            digest("SELECT 'a' AS value UNION ALL SELECT 'b'"),
            digest("SELECT 'ab' AS value")
        );
        assert_ne!(digest("SELECT 'x' AS a"), digest("SELECT 'x' AS b"));
    }
}
