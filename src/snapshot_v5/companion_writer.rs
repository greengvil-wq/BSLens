//! Явный профиль: bounded Git ingest и пофайловый producer из сохранённых sources.
use super::{contract, publish, streaming_facts, BuiltSnapshot, Error, Limits, Result};
use crate::git_manifest::ingest;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

pub const PROFILE: &str = "linux-v5-companion1-xml221-r09";

/// Отдельный DDL профиля и таблицы inline; исходный legacy DDL неизменён.
pub(crate) fn schema() -> String {
    format!(
        "{}\n{}",
        contract::SCHEMA
            .replace(contract::PROFILE, PROFILE)
            .replace("('xml','bsl')", "('xml','bsl','payload')"),
        format!("{}\n{}", super::inline_writer::INLINE_DDL, COMPANION_DDL)
    )
}

/// Нормализованные эффективные бюджеты нового producer; legacy-профиль не меняется.
fn parameters(
    stream: streaming_facts::Limits,
    git: ingest::Limits,
    common: bool,
    roots: bool,
    shared: bool,
    leaves: bool,
    journals: bool,
    services: bool,
    charts: bool,
    registers: bool,
    reports: bool,
    workflows: bool,
    corpus: bool,
    global_opaque: &[String],
    bin_bytes: Option<u64>,
    root_help: bool,
    configuration: bool,
    diagnostics: Option<u64>,
    optional_form: bool,
    root_sidecars: bool,
    context: bool,
    headers: bool,
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
    source_anomalies: bool,
    command_owners: bool,
) -> Result<Value> {
    if git.deadline != Duration::from_millis(git.deadline.as_millis() as u64) {
        return Err(Error::Contract("ingest_deadline_milliseconds"));
    }
    git.validate()
        .map_err(|_| Error::Contract("ingest_limits"))?;
    stream.validate()?;
    if stream.time != Duration::from_millis(stream.time.as_millis() as u64) {
        return Err(Error::Contract("stream_deadline_milliseconds"));
    }
    let limits = stream.sources;
    let mut value = json!({"limits":limits.json(),"selection":"xml-bsl-and-companion-files",
        "git_guard":"bounded-ingest-v1","git_limits":{
            "entries":git.entries,"trees":git.trees,"reads":git.reads,
            "object_bytes":git.object_bytes,"total_bytes":git.total_bytes,
            "deadline_ms":git.deadline.as_millis() as u64},
        "git_fixed":{"depth":64,"path_bytes":1024,"layout_entries":100000,"layout_depth":64,
            "excluded":"inventory-only-unread"},
        "facts_guard":"sqlite-companion-facts-v1","inline_guard":"confirmed-inline-v1","companions":"descriptor-structure-v1-payload-opaque","xml_guard":"r09","lexical_guard":"r09",
        "facts_limits":{"facts":stream.facts,"coverage_bytes":stream.coverage_bytes,"deadline_ms":stream.time.as_millis() as u64},
        "xml_limits":{"bytes":crate::xml221::MAX_XML_BYTES,"depth":crate::xml221::MAX_DEPTH,"events":crate::xml221::MAX_EVENTS,"nodes":crate::xml221::MAX_NODES},
        "lexical_limits":{"bytes":crate::lexical::MAX_BYTES,"lines":crate::lexical::MAX_LINES,"tokens":crate::lexical::MAX_TOKENS,"region_depth":crate::lexical::MAX_REGION_DEPTH},
        "coverage":"temporary-r09","relations":"not_analyzed"});
    if let Some(bytes) = bin_bytes {
        super::corpus_bin_writer::Policy::new(bytes, global_opaque)?;
        value["opaque_bin"] = json!({"bytes":bytes,"grammar":"owned-template-xdto-global-bin-v1"});
    }
    if context {
        value["lexical_guard"] = json!(super::context_writer::LEXICAL);
        value["context_annotations"] = json!({"allowlist":["НаКлиенте","НаСервере","НаСервереБезКонтекста"],"adjacency":"next-physical-line","case":"exact","method_range":"declaration-through-end"});
    }
    if headers {
        value["lexical_guard"] = json!(super::parameter_writer::LEXICAL);
        value["parameter_headers"] = json!({"grammar":"identifier-val-no-default-v1","max_parameters":64,"max_header_bytes":8192,"max_tokens":200,"max_token_bytes":128,"multiline":false,"defaults":false});
    }
    if conditional {
        value["lexical_guard"] = json!(super::conditional_writer::LEXICAL);
        value["conditional_guard"] = json!({"grammar":"ru-physical-boolean-v1","atoms":["Сервер","ТолстыйКлиентОбычноеПриложение","ВнешнееСоединение"],"case":"exact","max_blocks":4096,"max_depth":8,"max_arms":64,"expression_bytes":4096,"expression_tokens":256,"expression_depth":16,"evaluation":"none","branch_evidence":"physical-declaration-path-v1","method_name_budget":"legacy_source_bound","header_bytes_exclude_method_name":true});
    }
    if strings {
        value["lexical_guard"] = json!(super::string_writer::LEXICAL);
        value["string_guard"] = json!({"grammar":"pipe-continuation-opaque-v1","indent":"space-tab","comments":"comment-only-lines","quotes":"doubled","evaluation":"none","bounds":"existing-module-bytes-lines-tokens","multiline_headers":false,"defaults":false});
    }
    if literals {
        value["lexical_guard"] = json!(super::literal_writer::LEXICAL);
        value["context_annotations"]["allowlist"] = json!(["НаКлиенте","НаСервере","НаСервереБезКонтекста","НаКлиентеНаСервереБезКонтекста"]);
        value["parameter_headers"]["grammar"] = json!("identifier-val-scalar-default-v1");
        value["parameter_headers"]["defaults"] = json!(true);
        value["string_guard"]["defaults"] = json!("single-line-only");
        value["literal_defaults"] = json!({"grammar":"undefined-boolean-signed-decimal-string-v1","decimal_digits":32,"evaluation":"none"});
    }
    if wrapped {
        value["lexical_guard"] = json!(super::wrapped_writer::LEXICAL);
        value["parameter_headers"]["multiline"] = json!("two-physical-lines-with-plain-parameters-only");
        value["wrapped_headers"] = json!({"grammar":"adjacent-after-comma-identifiers-v1","max_physical_lines":2,"defaults":false,"markers":false});
    }
    if refined {
        value["lexical_guard"] = json!(super::refined_writer::LEXICAL);
        value["conditional_guard"]["atoms"] = json!(["Сервер","ТолстыйКлиентОбычноеПриложение","ВнешнееСоединение","ВебКлиент","МобильныйКлиент","ТонкийКлиент","ТолстыйКлиентУправляемоеПриложение","Клиент"]);
        value["parameter_headers"]["multiline"] = json!("bounded-comma-separated-with-existing-scalar-defaults");
        value["wrapped_headers"] = json!({"grammar":"adjacent-comma-separated-scalar-v2","max_physical_lines":crate::lexical::MAX_REFINED_HEADER_LINES,"comments":false,"directives":false,"evaluation":"none"});
    }
    if annotated_vars {
        value["lexical_guard"] = json!(super::annotated_vars_writer::LEXICAL);
        value["annotated_module_variables"] = json!({"grammar":"bare-client-or-server-adjacent-ru-module-var-v1","max_names":crate::lexical::MAX_ANNOTATED_VAR_NAMES,"export":"single-name-only","evaluation":"none","facts":"none"});
    }
    if dates_members {
        value["lexical_guard"] = json!(super::dates_members_writer::LEXICAL);
        value["date_member_grammar"] = json!({"date":"compact-8-12-14-ascii","member":"adjacent-ru-procedure","evaluation":"none"});
    }
    if async_casing {
        value["lexical_guard"] = json!(super::async_casing_writer::LEXICAL);
        value["async_condition_grammar"] = json!({"header":"single-line-ru-async","context":"bare-client-or-absent","condition":"known-ru-atoms-operators-case-insensitive","evaluation":"none"});
    }
    if mobile_vars {
        value["lexical_guard"] = json!(super::mobile_vars_writer::LEXICAL);
        value["mobile_variable_grammar"] = json!({"condition":"three-ru-mobile-atoms","variable":"bounded-adjacent-ru-name-list","evaluation":"none","facts":"none"});
    }
    if method_regions {
        value["lexical_guard"] = json!(super::method_regions_writer::LEXICAL);
        value["method_region_grammar"] = json!({"max_method_owned_depth":1,"branch_crossing":false,"facts":"none"});
    }
    if header_forms {
        value["lexical_guard"] = json!(super::header_forms_writer::LEXICAL);
        value["extended_header_grammar"] = json!({"null":"scalar-only","async_wrapped":true,"token_proven_line_comments":true,"evaluation":"none"});
    }
    if source_limits {
        value["lexical_guard"] = json!(super::source_limits_writer::LEXICAL);
        value["bounded_source_grammar"] = json!({"module_bytes":4*1024*1024,"client_comment_lines":16,"date":"exact-iso-day-digits-only","evaluation":"none"});
    }
    if long_headers {
        value["lexical_guard"] = json!(super::long_headers_writer::LEXICAL);
        value["long_ordinary_header"] = json!({
            "max_physical_lines":crate::lexical::MAX_LONG_ORDINARY_HEADER_LINES,
            "formal_identifier_bytes":crate::lexical::MAX_LONG_FORMAL_BYTES,
            "exception":"formal-with-scalar-default-in-17-to-20-line-ordinary-header"
        });
    }
    if lexical_tail {
        value["lexical_guard"] = json!(super::lexical_tail_writer::LEXICAL);
        value["lexical_tail_grammar"] = json!({
            "member":"adjacent-ru-procedure-one-nonreserved-selector-atomic-id-or-closed-string-in-method",
            "large_token_threshold_bytes":crate::lexical::LEXICAL_TAIL_LARGE_TOKEN_THRESHOLD_BYTES,
            "large_token_cap":crate::lexical::LEXICAL_TAIL_LARGE_MAX_TOKENS
        });
    }
    if source_anomalies {
        value["lexical_guard"] = json!(if lexical_tail {
            super::navigator_agreement_writer::LEXICAL
        } else {
            super::source_anomalies_writer::LEXICAL
        });
        value["source_anomaly_policy"] = json!({"algorithm":"full-mapped-inventory-agreement-v1","max_sites_per_module":1,"coordinates":"raw-lf","compiler_validation":false});
    }
    if command_owners {
        value["command_owner_policy"] = json!("exact-inline-catalog-document-informationregister-v1");
    }
    if root_sidecars {
        value["root_sidecars_guard"] = json!("catalog-document-register-root-xml-opaque-v1");
    }
    if optional_form {
        value["optional_form_guard"] = json!("accumulation-characteristic-module-0-or-1-layout-v1");
    }
    if let Some(bytes) = diagnostics {
        super::diagnostics_writer::check_limit(bytes)?;
        value["diagnostics"] = json!({"format":"xml-diagnostics-json-v1","bytes":bytes,"row_bytes":super::diagnostics_writer::ROW_BYTES,"domain":super::diagnostics_writer::DOMAIN});
    }
    if configuration {
        value["configuration_xml"] = json!({"path":"Configuration.xml","kind":"Configuration","nodes":crate::xml221::MAX_CONFIGURATION_NODES,"guard":"root-dom16384-v1"});
    }
    if root_help {
        value["root_help_guard"] = json!("dataprocessor-root-help-png-pair-v1");
    }
    if common {
        value["common_module_guard"] = json!("xml221-required-module-v1");
    }
    if roots {
        value["root_locator_guard"] = json!("descriptor-only-allowlist-v1");
    }
    if shared {
        value["shared_guard"] = json!("common-command-form-template-known-files-v1");
        value["shared_template_header_bytes"] = json!(65536);
        value["shared_template_header_events"] = json!(128);
    }
    if leaves {
        value["leaf_guard"] = json!("five-leaf-roots-known-files-v1");
    }
    if journals {
        value["journal_guard"] = json!("enum-journal-inline-command-v1");
    }
    if services {
        value["service_guard"] = json!("service-inline-xml221-v1");
    }
    if corpus {
        value["corpus_guard"] = json!("root42-subsystem3-empty-filter-global-v1");
        let normalized =
            super::corpus_writer::parse_global_opaque(&serde_json::to_string(global_opaque)?)?;
        value["global_scope"] =
            json!({"policy":"root-or-ext-xml-opaque-allowlist-v1","opaque":normalized});
    }
    if workflows {
        value["workflow_guard"] = json!("exchange-process-task-v1");
    }
    if reports {
        value["report_guard"] = json!("processor-report-assets-v1");
    }
    if registers {
        value["register_guard"] = json!("three-registers-recalculation-v1");
    }
    if charts {
        value["chart_guard"] = json!("three-charts-inline-form-png-v1");
    }
    Ok(value)
}

/// Проверяется reader даже без Git. Сохранённые бюджеты не становятся его политикой.
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    validate_metadata_mode(
        db, false, false, false, false, false, false, false, false, false, false, false, false,
        false, false, false, false, false, false, false, false, false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    )
}
pub(super) fn validate_metadata_mode(
    db: &Connection,
    common: bool,
    roots: bool,
    shared: bool,
    leaves: bool,
    journals: bool,
    services: bool,
    charts: bool,
    registers: bool,
    reports: bool,
    workflows: bool,
    corpus: bool,
    bin_profile: bool,
    root_help: bool,
    configuration: bool,
    diagnostics: bool,
    optional_form: bool,
    root_sidecars: bool,
    context: bool,
    headers: bool,
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
    source_anomalies: bool,
    command_owners: bool,
) -> Result<()> {
    let (raw, stored): (String, String) = db.query_row(
        "SELECT build_parameters,limits_draft FROM metadata WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let p: Value = serde_json::from_str(&raw)?;
    let number = |v: &Value, k: &str| v[k].as_u64().ok_or(Error::Contract("ingest_parameters"));
    let l = &p["limits"];
    let limits = Limits {
        source_count: number(l, "source_count")?,
        source_bytes: number(l, "source_bytes")?,
        total_source_bytes: number(l, "total_source_bytes")?,
        sqlite_bytes: number(l, "sqlite_bytes")?,
    };
    let g = &p["git_limits"];
    let usize_value = |k| -> Result<usize> {
        usize::try_from(number(g, k)?).map_err(|_| Error::Contract("ingest_parameters"))
    };
    let git = ingest::Limits {
        entries: usize_value("entries")?,
        trees: usize_value("trees")?,
        reads: usize_value("reads")?,
        object_bytes: usize_value("object_bytes")?,
        total_bytes: usize_value("total_bytes")?,
        deadline: Duration::from_millis(number(g, "deadline_ms")?),
    };
    let stream = streaming_facts::Limits {
        sources: limits,
        facts: number(&p["facts_limits"], "facts")?,
        coverage_bytes: number(&p["facts_limits"], "coverage_bytes")?,
        time: Duration::from_millis(number(&p["facts_limits"], "deadline_ms")?),
    };
    let global_opaque = if corpus {
        super::corpus_writer::parse_global_opaque(&p["global_scope"]["opaque"].to_string())?
    } else {
        vec![]
    };
    if corpus {
        super::corpus_writer::validate_global_sources(db, &global_opaque)?;
    }
    let bin_policy = if bin_profile {
        Some(super::corpus_bin_writer::stored_policy(db)?)
    } else {
        None
    };
    if p != parameters(
        stream,
        git,
        common,
        roots,
        shared,
        leaves,
        journals,
        services,
        charts,
        registers,
        reports,
        workflows,
        corpus,
        &global_opaque,
        bin_policy.as_ref().map(|b| b.bytes),
        root_help,
        configuration,
        if diagnostics {
            bin_policy.as_ref().and_then(|p| p.diagnostics)
        } else {
            None
        },
        optional_form,
        root_sidecars,
        context,
        headers,
        conditional,
        strings,
        literals,
        wrapped,
        refined,
        annotated_vars,
        dates_members,
        async_casing,
        mobile_vars,
        method_regions,
        header_forms,
        source_limits,
        long_headers,
        lexical_tail,
        source_anomalies,
        command_owners,
    )? || raw != p.to_string()
        || stored != limits.json().to_string()
    {
        return Err(Error::Contract("ingest_parameters"));
    }
    let (count, total, largest): (u64,u64,u64) = db.query_row(
        "SELECT count(*),coalesce(sum(length(content)),0),coalesce(max(length(content)),0) FROM sources",[],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    Limits::check("source_count", limits.source_count, count)?;
    let _ = largest;
    super::corpus_bin_writer::check_sources(db, limits.source_bytes, bin_policy.as_ref())?;
    Limits::check("total_source_bytes", limits.total_source_bytes, total)?;
    let mut facts = 0u64;
    for table in [
        "objects",
        "modules",
        "source_lines",
        "methods",
        "inline_declarations",
        "companions",
        "companion_payloads",
    ] {
        facts += db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
            r.get::<_, u64>(0)
        })?;
    }
    if journals {
        facts += db.query_row("SELECT count(*) FROM inline_module_owners", [], |r| {
            r.get::<_, u64>(0)
        })?;
    }
    if diagnostics {
        let n: u64 = db.query_row(
            "SELECT length(CAST(coverage_draft AS BLOB)) FROM metadata",
            [],
            |r| r.get(0),
        )?;
        Limits::check("coverage_summary_bytes", 8192, n)?;
    }
    let raw: String = db.query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))?;
    Limits::check("coverage_bytes", stream.coverage_bytes, raw.len() as u64)?;
    let coverage: Value = serde_json::from_str(&raw)?;
    facts += if diagnostics {
        db.query_row("SELECT count(*) FROM xml_diagnostics", [], |r| {
            r.get::<_, u64>(0)
        })?
    } else {
        coverage["xml_unverified"]
            .as_array()
            .ok_or(Error::Contract("stream_coverage"))?
            .len() as u64
    };
    Limits::check("facts", stream.facts, facts)?;
    Ok(())
}

/// Только явный режим; публикация после Receipt, фактов и полного contract check.
pub fn build(
    repo: &Path,
    commit: &str,
    expected_tree: &str,
    source_id: &str,
    output: &Path,
    stream: streaming_facts::Limits,
    git: ingest::Limits,
) -> Result<BuiltSnapshot> {
    build_mode(
        repo,
        commit,
        expected_tree,
        source_id,
        output,
        stream,
        git,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        &[],
        None,
        false,
        false,
        None,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    )
}
pub(super) fn build_mode(
    repo: &Path,
    commit: &str,
    expected_tree: &str,
    source_id: &str,
    output: &Path,
    stream: streaming_facts::Limits,
    git: ingest::Limits,
    common: bool,
    roots: bool,
    shared: bool,
    leaves: bool,
    journals: bool,
    services: bool,
    charts: bool,
    registers: bool,
    reports: bool,
    workflows: bool,
    corpus: bool,
    global_opaque: &[String],
    bin_bytes: Option<u64>,
    root_help: bool,
    configuration: bool,
    diagnostics: Option<u64>,
    optional_form: bool,
    root_sidecars: bool,
    context: bool,
    headers: bool,
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
    source_anomalies: bool,
    command_owners: bool,
) -> Result<BuiltSnapshot> {
    if source_id.is_empty() {
        return Err(Error::Contract("source_id"));
    }
    let parameters = parameters(
        stream,
        git,
        common,
        roots,
        shared,
        leaves,
        journals,
        services,
        charts,
        registers,
        reports,
        workflows,
        corpus,
        global_opaque,
        bin_bytes,
        root_help,
        configuration,
        diagnostics,
        optional_form,
        root_sidecars,
        context,
        headers,
        conditional,
        strings,
        literals,
        wrapped,
        refined,
        annotated_vars,
        dates_members,
        async_casing,
        mobile_vars,
        method_regions,
        header_forms,
        source_limits,
        long_headers,
        lexical_tail,
        source_anomalies,
        command_owners,
    )?;
    let mut bin_policy = bin_bytes
        .map(|n| super::corpus_bin_writer::Policy::new(n, global_opaque))
        .transpose()?;
    if let Some(p) = &mut bin_policy {
        p.root_help = root_help;
        p.configuration = configuration;
        p.diagnostics = diagnostics;
        p.optional_form = optional_form;
        p.root_sidecars = root_sidecars;
        p.context = context;
        p.headers = headers;
        p.conditional = conditional;
        p.strings = strings;
        p.literals = literals;
        p.wrapped = wrapped;
        p.refined = refined;
        p.annotated_vars = annotated_vars;
        p.dates_members = dates_members;
        p.async_casing = async_casing;
        p.mobile_vars = mobile_vars;
        p.method_regions = method_regions;
        p.header_forms = header_forms;
        p.source_limits = source_limits;
        p.long_headers = long_headers;
        p.lexical_tail = lexical_tail;
        p.source_anomalies = source_anomalies;
        p.command_owners = command_owners;
    }
    let limits = stream.sources;
    let dir = publish::directory(output)?;
    let temp = publish::Temporary::new(&dir)?;
    let mut db = Connection::open(&temp.path)?;
    db.pragma_update(None, "page_size", 4096)?;
    let max_pages = limits.sqlite_bytes / 4096;
    if max_pages == 0 {
        return Err(Error::Limit {
            name: "sqlite_bytes",
            maximum: limits.sqlite_bytes,
            actual: 4096,
        });
    }
    db.pragma_update(None, "max_page_count", max_pages)?;
    let result = (|| -> Result<(String, String)> {
        db.execute_batch(&if command_owners {
            super::navigator_owners_writer::schema()
        } else if source_anomalies && lexical_tail {
            super::navigator_agreement_writer::schema()
        } else if source_anomalies {
            super::source_anomalies_writer::schema()
        } else if lexical_tail {
            super::lexical_tail_writer::schema()
        } else if long_headers {
            super::long_headers_writer::schema()
        } else if source_limits {
            super::source_limits_writer::schema()
        } else if header_forms {
            super::header_forms_writer::schema()
        } else if method_regions {
            super::method_regions_writer::schema()
        } else if mobile_vars {
            super::mobile_vars_writer::schema()
        } else if async_casing {
            super::async_casing_writer::schema()
        } else if dates_members {
            super::dates_members_writer::schema()
        } else if annotated_vars {
            super::annotated_vars_writer::schema()
        } else if refined {
            super::refined_writer::schema()
        } else if wrapped {
            super::wrapped_writer::schema()
        } else if literals {
            super::literal_writer::schema()
        } else if strings {
            super::string_writer::schema()
        } else if conditional {
            super::conditional_writer::schema()
        } else if headers {
            super::parameter_writer::schema()
        } else if context {
            super::context_writer::schema()
        } else if root_sidecars {
            super::sidecar_writer::schema()
        } else if optional_form {
            super::optional_form_writer::schema()
        } else if diagnostics.is_some() {
            super::diagnostics_writer::schema()
        } else if configuration {
            super::corpus_config_writer::schema()
        } else if root_help {
            super::corpus_help_writer::schema()
        } else if bin_bytes.is_some() {
            super::corpus_bin_writer::schema()
        } else if corpus {
            super::corpus_writer::schema()
        } else if workflows {
            super::workflow_writer::schema()
        } else if reports {
            super::report_writer::schema()
        } else if registers {
            super::register_writer::schema()
        } else if charts {
            super::chart_writer::schema()
        } else if services {
            super::service_writer::schema()
        } else if journals {
            super::journal_writer::schema()
        } else if leaves {
            super::leaf_writer::schema()
        } else if shared {
            super::shared_writer::schema()
        } else if roots {
            super::root_writer::schema()
        } else if common {
            super::common_writer::schema()
        } else {
            schema()
        })?;
        let tx = db.transaction()?;
        let (mut count, mut total) = (0u64, 0u64);
        let mut consumer_error = None;
        let receipt = ingest::ingest_selected_capped(
            repo,
            commit,
            expected_tree,
            git,
            |e| {
                selected(e)
                    || (shared && super::shared_writer::is_path(&e.path))
                    || (leaves && super::leaf_writer::is_path(&e.path))
                    || (journals && super::journal_writer::is_path(&e.path))
                    || (services && super::service_writer::is_path(&e.path))
                    || (charts && super::chart_writer::is_path(&e.path))
                    || (registers && super::register_writer::is_path(&e.path))
                    || (reports && super::report_writer::is_path(&e.path))
                    || (workflows && super::workflow_writer::is_path(&e.path))
                    || (corpus
                        && (super::corpus_writer::selected_extra(&e.path)
                            || global_opaque.contains(&e.path)))
            },
            |entry| {
                bin_policy
                    .as_ref()
                    .filter(|b| b.eligible(&entry.path))
                    .map(|b| b.bytes as usize)
            },
            |entry, sha, bytes| {
                let result = (|| -> Result<()> {
                    count += 1;
                    total = total
                        .checked_add(bytes.len() as u64)
                        .ok_or(Error::Contract("source_size_overflow"))?;
                    Limits::check("source_count", limits.source_count, count)?;
                    Limits::check(
                        "source_bytes",
                        bin_policy.as_ref().map_or(limits.source_bytes, |b| {
                            b.cap(
                                &entry.path,
                                super::corpus_writer::source_kind(&entry.path),
                                limits.source_bytes,
                            )
                        }),
                        bytes.len() as u64,
                    )?;
                    Limits::check("total_source_bytes", limits.total_source_bytes, total)?;
                    tx.execute(
                        "INSERT INTO sources VALUES (?1,?2,?3,?4,?5,?6)",
                        params![
                            entry.path,
                            if root_sidecars {
                                super::sidecar_writer::source_kind(&entry.path)
                            } else if corpus {
                                super::corpus_writer::source_kind(&entry.path)
                            } else if workflows {
                                super::workflow_writer::source_kind(&entry.path)
                            } else if reports {
                                super::report_writer::source_kind(&entry.path)
                            } else if registers {
                                super::register_writer::source_kind(&entry.path)
                            } else if charts {
                                super::chart_writer::source_kind(&entry.path)
                            } else if services {
                                super::service_writer::source_kind(&entry.path)
                            } else if journals {
                                super::journal_writer::source_kind(&entry.path)
                            } else if leaves {
                                super::leaf_writer::source_kind(&entry.path)
                            } else if shared {
                                super::shared_writer::source_kind(&entry.path)
                            } else {
                                source_kind(&entry.path)
                            },
                            format!("{:o}", entry.mode),
                            entry.oid,
                            sha,
                            bytes
                        ],
                    )?;
                    Ok(())
                })();
                result.map_err(|e| {
                    consumer_error = Some(e);
                    "writer_rejected".into()
                })
            },
        );
        if let Some(e) = consumer_error {
            return Err(e);
        }
        let receipt = receipt.map_err(Error::Ingest)?;
        let mut chosen = vec![];
        let mut excluded = vec![];
        for e in receipt.manifest.entries() {
            if corpus {
                super::corpus_writer::validate_path(&e.path)?;
                if super::corpus_writer::global_opaque(&e.path) && !global_opaque.contains(&e.path)
                {
                    return Err(Error::Contract("global_scope_unlisted"));
                }
            }
            if workflows {
                super::workflow_writer::validate_path(&e.path)?;
            }
            if reports {
                if root_help {
                    super::corpus_help_writer::validate_path(&e.path)?;
                } else {
                    super::report_writer::validate_path(&e.path)?;
                }
            }
            if registers {
                super::register_writer::validate_path(&e.path)?;
            }
            if charts {
                super::chart_writer::validate_path(&e.path)?;
            }
            if services {
                super::service_writer::validate_path(&e.path)?;
            }
            if journals {
                super::journal_writer::validate_path(&e.path)?;
            }
            if leaves {
                super::leaf_writer::validate_path(&e.path)?;
            }
            if shared {
                super::shared_writer::validate_path(&e.path)?;
            }
            if roots {
                crate::xml221::descriptor_only_path(&e.path).map_err(Error::Xml)?;
            }
            let entry = json!({"path":e.path,"mode":format!("{:o}",e.mode),"blob_oid":e.oid});
            if selected(e)
                || (shared && super::shared_writer::is_path(&e.path))
                || (leaves && super::leaf_writer::is_path(&e.path))
                || (journals && super::journal_writer::is_path(&e.path))
                || (services && super::service_writer::is_path(&e.path))
                || (charts && super::chart_writer::is_path(&e.path))
                || (registers && super::register_writer::is_path(&e.path))
                || (reports && super::report_writer::is_path(&e.path))
                || (workflows && super::workflow_writer::is_path(&e.path))
                || (corpus
                    && (super::corpus_writer::selected_extra(&e.path)
                        || global_opaque.contains(&e.path)))
            {
                chosen.push(entry);
            } else {
                excluded.push(json!({"input":entry,"reason":"OutsideSelectionNotRead"}));
            }
        }
        if corpus {
            let normalized =
                super::corpus_writer::parse_global_opaque(&serde_json::to_string(global_opaque)?)?;
            super::corpus_writer::validate_global_sources(&tx, &normalized)?;
        }
        tx.commit()?;
        let coverage = if let Some(policy) = bin_policy.clone() {
            streaming_facts::derive_corpus_bin(&mut db, stream, policy)?
        } else if corpus {
            streaming_facts::derive_corpus(&mut db, stream)?
        } else if workflows {
            streaming_facts::derive_workflows(&mut db, stream)?
        } else if reports {
            streaming_facts::derive_reports(&mut db, stream)?
        } else if registers {
            streaming_facts::derive_registers(&mut db, stream)?
        } else if charts {
            streaming_facts::derive_charts(&mut db, stream)?
        } else if services {
            streaming_facts::derive_services(&mut db, stream)?
        } else if journals {
            streaming_facts::derive_journals(&mut db, stream)?
        } else if leaves {
            streaming_facts::derive_leaves(&mut db, stream)?
        } else if shared {
            streaming_facts::derive_shared(&mut db, stream)?
        } else if roots {
            streaming_facts::derive_roots(&mut db, stream)?
        } else if common {
            streaming_facts::derive_common_modules(&mut db, stream)?
        } else {
            streaming_facts::derive_companions(&mut db, stream)?
        };
        let tx = db.transaction()?;
        let date: String =
            tx.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')", [], |r| {
                r.get(0)
            })?;
        tx.execute("INSERT INTO metadata VALUES (1,5,?1,'bslens-v5-pending',?2,NULL,?3,?4,?5,?6,?7,'2.21',?8,?9,?10,?11,?12,?13,'not_analyzed')",
            params![if command_owners {super::navigator_owners_writer::PROFILE}else if source_anomalies && lexical_tail {super::navigator_agreement_writer::PROFILE}else if source_anomalies {super::source_anomalies_writer::PROFILE}else if lexical_tail {super::lexical_tail_writer::PROFILE}else if long_headers {super::long_headers_writer::PROFILE}else if source_limits {super::source_limits_writer::PROFILE}else if header_forms {super::header_forms_writer::PROFILE}else if method_regions {super::method_regions_writer::PROFILE}else if mobile_vars {super::mobile_vars_writer::PROFILE}else if async_casing {super::async_casing_writer::PROFILE}else if dates_members {super::dates_members_writer::PROFILE}else if annotated_vars {super::annotated_vars_writer::PROFILE}else if refined {super::refined_writer::PROFILE}else if wrapped {super::wrapped_writer::PROFILE}else if literals {super::literal_writer::PROFILE}else if strings {super::string_writer::PROFILE}else if conditional {super::conditional_writer::PROFILE}else if headers {super::parameter_writer::PROFILE}else if context {super::context_writer::PROFILE}else if root_sidecars {super::sidecar_writer::PROFILE}else if optional_form {super::optional_form_writer::PROFILE}else if diagnostics.is_some() {super::diagnostics_writer::PROFILE}else if configuration {super::corpus_config_writer::PROFILE}else if root_help {super::corpus_help_writer::PROFILE}else if bin_bytes.is_some() {super::corpus_bin_writer::PROFILE}else if corpus {super::corpus_writer::PROFILE}else if workflows {super::workflow_writer::PROFILE}else if reports {super::report_writer::PROFILE}else if registers {super::register_writer::PROFILE}else if charts {super::chart_writer::PROFILE}else if services {super::service_writer::PROFILE}else if journals {super::journal_writer::PROFILE}else if leaves {super::leaf_writer::PROFILE}else if shared {super::shared_writer::PROFILE}else if roots {super::root_writer::PROFILE}else if common {super::common_writer::PROFILE}else{PROFILE},date,source_id,receipt.manifest.commit(),receipt.manifest.tree(),contract::CORE,contract::ADAPTER,
                if source_anomalies && lexical_tail {super::navigator_agreement_writer::LEXICAL}else if source_anomalies {super::source_anomalies_writer::LEXICAL}else if lexical_tail {super::lexical_tail_writer::LEXICAL}else if long_headers {super::long_headers_writer::LEXICAL}else if source_limits {super::source_limits_writer::LEXICAL}else if header_forms {super::header_forms_writer::LEXICAL}else if method_regions {super::method_regions_writer::LEXICAL}else if mobile_vars {super::mobile_vars_writer::LEXICAL}else if async_casing {super::async_casing_writer::LEXICAL}else if dates_members {super::dates_members_writer::LEXICAL}else if annotated_vars {super::annotated_vars_writer::LEXICAL}else if refined {super::refined_writer::LEXICAL}else if wrapped {super::wrapped_writer::LEXICAL}else if literals {super::literal_writer::LEXICAL}else if strings {super::string_writer::LEXICAL}else if conditional {super::conditional_writer::LEXICAL}else if headers {super::parameter_writer::LEXICAL}else if context {super::context_writer::LEXICAL}else{contract::LEXICAL},contract::SCOPE,parameters.to_string(),json!({"selected":chosen,"excluded":excluded}).to_string(),
                coverage.to_string(),limits.json().to_string()])?;
        contract::compatible_scope(&tx)?;
        let id = contract::snapshot_id(&tx)?;
        let facts = contract::facts_sha256(&tx)?;
        tx.execute(
            "UPDATE metadata SET snapshot_id=?1,facts_sha256=?2",
            params![id, facts],
        )?;
        tx.commit()?;
        Ok((id, facts))
    })();
    if matches!(&result,Err(Error::Sqlite(rusqlite::Error::SqliteFailure(e,_))) if e.code==rusqlite::ErrorCode::DiskFull)
    {
        return Err(Error::SqliteFull {
            path: temp.path.clone(),
            sqlite_bytes: limits.sqlite_bytes,
            max_pages,
        });
    }
    let (id, facts) = result?;
    db.close().map_err(|(_, e)| Error::Sqlite(e))?;
    temp.ensure_standalone()?;
    Limits::check(
        "sqlite_bytes",
        limits.sqlite_bytes,
        std::fs::metadata(&temp.path)?.len(),
    )?;
    publish::publish(&dir, temp, id, facts, limits.sqlite_bytes)
}

/// Payloads are saved opaque, never parsed as descriptors or BSL.
pub(crate) fn companion_file(path: &str) -> bool {
    let mut parts = path.split('/');
    matches!(
        parts.next(),
        Some("Catalogs" | "Documents" | "InformationRegisters")
    ) && parts.next().is_some()
        && matches!(parts.next(), Some("Forms" | "Templates"))
}
fn selected(e: &ingest::Entry) -> bool {
    e.selected() || companion_file(&e.path)
}
pub(crate) fn source_kind(path: &str) -> &'static str {
    if companion_file(path) {
        let parts: Vec<_> = path.split('/').collect();
        if parts.len() == 7 && parts[2] == "Forms" && parts[4..] == ["Ext", "Form", "Module.bsl"] {
            return "bsl";
        }
        if parts.len() != 4 || !path.ends_with(".xml") {
            return "payload";
        }
    }
    if path.ends_with(".xml") {
        "xml"
    } else if path.ends_with(".bsl") {
        "bsl"
    } else {
        "payload"
    }
}
pub(super) const COMPANION_DDL: &str = r#"
CREATE TABLE companions (
 object_id INTEGER PRIMARY KEY REFERENCES objects(object_id),
 reference_path TEXT NOT NULL,
 reference_kind TEXT NOT NULL DEFAULT 'xml' CHECK(reference_kind='xml'),
 byte_start INTEGER NOT NULL CHECK(byte_start>=0),
 byte_end INTEGER NOT NULL CHECK(byte_end>byte_start),
 FOREIGN KEY(reference_path,reference_kind) REFERENCES sources(path,kind)
) STRICT;
CREATE TABLE companion_payloads (
 path TEXT PRIMARY KEY,
 source_kind TEXT NOT NULL DEFAULT 'payload' CHECK(source_kind='payload'),
 object_id INTEGER NOT NULL REFERENCES companions(object_id),
 role TEXT NOT NULL CHECK(role IN ('form','template','additional')),
 FOREIGN KEY(path,source_kind) REFERENCES sources(path,kind)
) STRICT;
CREATE INDEX companion_payload_owner ON companion_payloads(object_id,path);
"#;
