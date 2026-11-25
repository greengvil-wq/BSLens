//! One immutable v5 reader per stdio session; no Git, parsing or dynamic open tools.
use crate::{
    linux_v5_reader::{Error, LinuxSnapshotReader, StagingError, TreeLimits},
    mcp_transport::{Exit, RouteError, Session, ToolRouter},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{self, BufRead, Write};

pub const MAX_SOURCE_BYTES: u64 = 65536;
pub const MAX_TREE_NODES: usize = 128;
pub const MAX_TREE_TEXT_BYTES: usize = 32768;

pub struct SnapshotSession {
    transport: Session,
    router: Router,
}
impl SnapshotSession {
    pub fn new(reader: LinuxSnapshotReader) -> Result<Self, Error> {
        let metadata = wire_metadata(reader.info()?)?;
        Ok(Self {
            transport: Session::new(),
            router: Router { reader, metadata },
        })
    }
    pub fn serve<R: BufRead, W: Write>(
        &mut self,
        input: &mut R,
        output: &mut W,
    ) -> io::Result<Exit> {
        self.transport.serve_with_tools(input, output, &self.router)
    }
}

// Fixed field allowlist: never echo coverage selectors, paths, unknown keys or values.
// Scope identity remains exact (the shared S3 contract bounds it to 1024 UTF-8 bytes).
fn wire_metadata(info: Value) -> Result<Value, Error> {
    let bounded_string = |key: &str, cap: usize| -> Result<&str, Error> {
        info.get(key)
            .and_then(Value::as_str)
            .filter(|v| v.len() <= cap)
            .ok_or(Error::Limit)
    };
    let coverage = &info["coverage"];
    let count = |key: &str| coverage.get(key).and_then(Value::as_u64);
    let agreement = info["profile"] == crate::snapshot_v5::navigator_agreement_writer::PROFILE
        || info["profile"] == crate::snapshot_v5::navigator_owners_writer::PROFILE;
    let scoped_anomalies = agreement || info["profile"] == crate::snapshot_v5::source_anomalies_writer::PROFILE;
    let mut accepted = coverage
        .get("parse_counts")
        .and_then(|v| v.get("accepted"))
        .and_then(Value::as_u64);
    let qualified = if scoped_anomalies {
        accepted = Some(accepted.unwrap_or(0));
        coverage["parse_counts"]["parsed_with_source_anomalies"].as_u64().unwrap_or(0)
    } else { 0 };
    let unparsed = count("modules")
        .zip(accepted)
        .and_then(|(total, n)| total.checked_sub(n)?.checked_sub(qualified));
    let digest = format!("{:x}", Sha256::digest(coverage.to_string().as_bytes()));
    let mut result = json!({
        "snapshot_id":bounded_string("snapshot_id",74)?,
        "profile":bounded_string("profile",64)?,
        "scope_id":bounded_string("scope_id",1024)?,
        "relations":"not_analyzed","git_provenance":"not_verified","experimental":true,
        "coverage":{
            "temporary":true,"summary_format":"allowlist-v1","details_omitted":true,
            "objects":count("objects"),"modules":count("modules"),"owner_unconfirmed":count("owner_unconfirmed"),
            "accepted_modules":accepted,"unparsed_modules":unparsed,
            "xml_unverified_count":coverage.get("xml_unverified").and_then(Value::as_array).map(Vec::len),
            "coverage_sha256":digest
        }
    });
    if [
        crate::snapshot_v5::diagnostics_writer::PROFILE,
        crate::snapshot_v5::optional_form_writer::PROFILE,
        crate::snapshot_v5::sidecar_writer::PROFILE,
        crate::snapshot_v5::context_writer::PROFILE,
        crate::snapshot_v5::parameter_writer::PROFILE,
        crate::snapshot_v5::conditional_writer::PROFILE,
        crate::snapshot_v5::string_writer::PROFILE,
        crate::snapshot_v5::literal_writer::PROFILE,
        crate::snapshot_v5::wrapped_writer::PROFILE,
        crate::snapshot_v5::refined_writer::PROFILE,
        crate::snapshot_v5::annotated_vars_writer::PROFILE,
        crate::snapshot_v5::dates_members_writer::PROFILE,
        crate::snapshot_v5::async_casing_writer::PROFILE,
        crate::snapshot_v5::mobile_vars_writer::PROFILE,
        crate::snapshot_v5::method_regions_writer::PROFILE,
        crate::snapshot_v5::header_forms_writer::PROFILE,
        crate::snapshot_v5::source_limits_writer::PROFILE,
        crate::snapshot_v5::long_headers_writer::PROFILE,
        crate::snapshot_v5::lexical_tail_writer::PROFILE,
        crate::snapshot_v5::source_anomalies_writer::PROFILE,
        crate::snapshot_v5::navigator_agreement_writer::PROFILE,
        crate::snapshot_v5::navigator_owners_writer::PROFILE,
    ]
    .contains(&info["profile"].as_str().unwrap_or(""))
    {
        let d = &coverage["xml_diagnostics"];
        result["coverage"]["xml_unverified_count"] =
            d["count"].as_u64().map_or(Value::Null, |n| json!(n));
        result["coverage"]["xml_diagnostics_bytes"] =
            d["bytes"].as_u64().map_or(Value::Null, |n| json!(n));
        let h = d["sha256"]
            .as_str()
            .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or(Error::Limit)?;
        result["coverage"]["xml_diagnostics_sha256"] = json!(h);
    }
    if [
        crate::snapshot_v5::sidecar_writer::PROFILE,
        crate::snapshot_v5::context_writer::PROFILE,
        crate::snapshot_v5::parameter_writer::PROFILE,
        crate::snapshot_v5::conditional_writer::PROFILE,
        crate::snapshot_v5::string_writer::PROFILE,
        crate::snapshot_v5::literal_writer::PROFILE,
        crate::snapshot_v5::wrapped_writer::PROFILE,
        crate::snapshot_v5::refined_writer::PROFILE,
        crate::snapshot_v5::annotated_vars_writer::PROFILE,
        crate::snapshot_v5::dates_members_writer::PROFILE,
        crate::snapshot_v5::async_casing_writer::PROFILE,
        crate::snapshot_v5::mobile_vars_writer::PROFILE,
        crate::snapshot_v5::method_regions_writer::PROFILE,
        crate::snapshot_v5::header_forms_writer::PROFILE,
        crate::snapshot_v5::source_limits_writer::PROFILE,
        crate::snapshot_v5::long_headers_writer::PROFILE,
        crate::snapshot_v5::lexical_tail_writer::PROFILE,
        crate::snapshot_v5::source_anomalies_writer::PROFILE,
        crate::snapshot_v5::navigator_agreement_writer::PROFILE,
        crate::snapshot_v5::navigator_owners_writer::PROFILE,
    ]
    .contains(&info["profile"].as_str().unwrap_or(""))
    {
        result["coverage"]["root_xml_sidecars_count"] = coverage["root_xml_sidecars"]["count"]
            .as_u64()
            .map_or(Value::Null, |n| json!(n));
        result["coverage"]["sidecar_help_html"] = json!("outside_scope_not_read");
    }
    if info["profile"] == crate::snapshot_v5::context_writer::PROFILE {
        result["lexical_guard"] = json!(crate::snapshot_v5::context_writer::LEXICAL);
        result["compiler_context"] = json!("not_semantically_verified");
    }
    if info["profile"] == crate::snapshot_v5::parameter_writer::PROFILE {
        result["lexical_guard"] = json!(crate::snapshot_v5::parameter_writer::LEXICAL);
        result["compiler_context"] = json!("not_semantically_verified");
    }
    if info["profile"] == crate::snapshot_v5::conditional_writer::PROFILE {
        result["lexical_guard"] = json!(crate::snapshot_v5::conditional_writer::LEXICAL);
        result["compiler_context"] = json!("not_semantically_verified");
    }
    if info["profile"] == crate::snapshot_v5::annotated_vars_writer::PROFILE {
        result["lexical_guard"] = json!(crate::snapshot_v5::annotated_vars_writer::LEXICAL);
        result["compiler_context"] = json!("not_semantically_verified");
    }
    if info["profile"] == crate::snapshot_v5::dates_members_writer::PROFILE {
        result["lexical_guard"] = json!(crate::snapshot_v5::dates_members_writer::LEXICAL);
        result["compiler_context"] = json!("not_semantically_verified");
    }
    if info["profile"] == crate::snapshot_v5::async_casing_writer::PROFILE {
        result["lexical_guard"] = json!(crate::snapshot_v5::async_casing_writer::LEXICAL);
        result["compiler_context"] = json!("not_semantically_verified");
    }
    if info["profile"] == crate::snapshot_v5::mobile_vars_writer::PROFILE {
        result["lexical_guard"] = json!(crate::snapshot_v5::mobile_vars_writer::LEXICAL);
        result["compiler_context"] = json!("not_semantically_verified");
    }
    if info["profile"] == crate::snapshot_v5::method_regions_writer::PROFILE {
        result["lexical_guard"] = json!(crate::snapshot_v5::method_regions_writer::LEXICAL);
        result["compiler_context"] = json!("not_semantically_verified");
    }
    if info["profile"] == crate::snapshot_v5::header_forms_writer::PROFILE {
        result["lexical_guard"] = json!(crate::snapshot_v5::header_forms_writer::LEXICAL);
        result["compiler_context"] = json!("not_semantically_verified");
    }
    if info["profile"] == crate::snapshot_v5::source_limits_writer::PROFILE {
        result["lexical_guard"] = json!(crate::snapshot_v5::source_limits_writer::LEXICAL);
        result["compiler_context"] = json!("not_semantically_verified");
    }
    if info["profile"] == crate::snapshot_v5::long_headers_writer::PROFILE {
        result["lexical_guard"] = json!(crate::snapshot_v5::long_headers_writer::LEXICAL);
        result["compiler_context"] = json!("not_semantically_verified");
    }
    if info["profile"] == crate::snapshot_v5::lexical_tail_writer::PROFILE {
        result["lexical_guard"] = json!(crate::snapshot_v5::lexical_tail_writer::LEXICAL);
        result["compiler_context"] = json!("not_semantically_verified");
    }
    if scoped_anomalies {
        result["lexical_guard"] = json!(if agreement {
            crate::snapshot_v5::navigator_agreement_writer::LEXICAL
        } else {
            crate::snapshot_v5::source_anomalies_writer::LEXICAL
        });
        result["compiler_context"] = json!("not_semantically_verified");
        result["coverage"]["parsed_with_source_anomalies_modules"] = json!(qualified);
        result["compiler_validation"] = json!("not_performed");
    }
    if info["profile"] == crate::snapshot_v5::refined_writer::PROFILE {
        result["lexical_guard"] = json!(crate::snapshot_v5::refined_writer::LEXICAL);
        result["compiler_context"] = json!("not_semantically_verified");
    }
    if info["profile"] == crate::snapshot_v5::wrapped_writer::PROFILE {
        result["lexical_guard"] = json!(crate::snapshot_v5::wrapped_writer::LEXICAL);
        result["compiler_context"] = json!("not_semantically_verified");
    }
    if info["profile"] == crate::snapshot_v5::literal_writer::PROFILE {
        result["lexical_guard"] = json!(crate::snapshot_v5::literal_writer::LEXICAL);
        result["compiler_context"] = json!("not_semantically_verified");
    }
    if info["profile"] == crate::snapshot_v5::string_writer::PROFILE {
        result["lexical_guard"] = json!(crate::snapshot_v5::string_writer::LEXICAL);
        result["compiler_context"] = json!("not_semantically_verified");
    }
    if [
        crate::snapshot_v5::inline_writer::PROFILE,
        crate::snapshot_v5::companion_writer::PROFILE,
        crate::snapshot_v5::common_writer::PROFILE,
        crate::snapshot_v5::root_writer::PROFILE,
        crate::snapshot_v5::shared_writer::PROFILE,
        crate::snapshot_v5::leaf_writer::PROFILE,
        crate::snapshot_v5::journal_writer::PROFILE,
        crate::snapshot_v5::service_writer::PROFILE,
        crate::snapshot_v5::chart_writer::PROFILE,
        crate::snapshot_v5::register_writer::PROFILE,
        crate::snapshot_v5::report_writer::PROFILE,
        crate::snapshot_v5::workflow_writer::PROFILE,
        crate::snapshot_v5::corpus_writer::PROFILE,
        crate::snapshot_v5::corpus_bin_writer::PROFILE,
        crate::snapshot_v5::corpus_help_writer::PROFILE,
        crate::snapshot_v5::corpus_config_writer::PROFILE,
        crate::snapshot_v5::diagnostics_writer::PROFILE,
        crate::snapshot_v5::optional_form_writer::PROFILE,
        crate::snapshot_v5::sidecar_writer::PROFILE,
        crate::snapshot_v5::context_writer::PROFILE,
        crate::snapshot_v5::parameter_writer::PROFILE,
        crate::snapshot_v5::conditional_writer::PROFILE,
        crate::snapshot_v5::string_writer::PROFILE,
        crate::snapshot_v5::literal_writer::PROFILE,
        crate::snapshot_v5::wrapped_writer::PROFILE,
        crate::snapshot_v5::refined_writer::PROFILE,
        crate::snapshot_v5::annotated_vars_writer::PROFILE,
        crate::snapshot_v5::dates_members_writer::PROFILE,
        crate::snapshot_v5::async_casing_writer::PROFILE,
        crate::snapshot_v5::mobile_vars_writer::PROFILE,
        crate::snapshot_v5::method_regions_writer::PROFILE,
        crate::snapshot_v5::header_forms_writer::PROFILE,
        crate::snapshot_v5::source_limits_writer::PROFILE,
        crate::snapshot_v5::long_headers_writer::PROFILE,
        crate::snapshot_v5::lexical_tail_writer::PROFILE,
        crate::snapshot_v5::source_anomalies_writer::PROFILE,
        crate::snapshot_v5::navigator_agreement_writer::PROFILE,
        crate::snapshot_v5::navigator_owners_writer::PROFILE,
    ]
    .contains(&info["profile"].as_str().unwrap_or(""))
    {
        result["coverage"]["inline_declarations"] = json!(count("inline_declarations"));
        result["coverage"]["inline_semantics"] = json!("not_analyzed");
    }
    if [
        crate::snapshot_v5::companion_writer::PROFILE,
        crate::snapshot_v5::common_writer::PROFILE,
        crate::snapshot_v5::root_writer::PROFILE,
        crate::snapshot_v5::shared_writer::PROFILE,
        crate::snapshot_v5::leaf_writer::PROFILE,
        crate::snapshot_v5::journal_writer::PROFILE,
        crate::snapshot_v5::service_writer::PROFILE,
        crate::snapshot_v5::chart_writer::PROFILE,
        crate::snapshot_v5::register_writer::PROFILE,
        crate::snapshot_v5::report_writer::PROFILE,
        crate::snapshot_v5::workflow_writer::PROFILE,
        crate::snapshot_v5::corpus_writer::PROFILE,
        crate::snapshot_v5::corpus_bin_writer::PROFILE,
        crate::snapshot_v5::corpus_help_writer::PROFILE,
        crate::snapshot_v5::corpus_config_writer::PROFILE,
        crate::snapshot_v5::diagnostics_writer::PROFILE,
        crate::snapshot_v5::optional_form_writer::PROFILE,
        crate::snapshot_v5::sidecar_writer::PROFILE,
        crate::snapshot_v5::context_writer::PROFILE,
        crate::snapshot_v5::parameter_writer::PROFILE,
        crate::snapshot_v5::conditional_writer::PROFILE,
        crate::snapshot_v5::string_writer::PROFILE,
        crate::snapshot_v5::literal_writer::PROFILE,
        crate::snapshot_v5::wrapped_writer::PROFILE,
        crate::snapshot_v5::refined_writer::PROFILE,
        crate::snapshot_v5::annotated_vars_writer::PROFILE,
        crate::snapshot_v5::dates_members_writer::PROFILE,
        crate::snapshot_v5::async_casing_writer::PROFILE,
        crate::snapshot_v5::mobile_vars_writer::PROFILE,
        crate::snapshot_v5::method_regions_writer::PROFILE,
        crate::snapshot_v5::header_forms_writer::PROFILE,
        crate::snapshot_v5::source_limits_writer::PROFILE,
        crate::snapshot_v5::long_headers_writer::PROFILE,
        crate::snapshot_v5::lexical_tail_writer::PROFILE,
        crate::snapshot_v5::source_anomalies_writer::PROFILE,
        crate::snapshot_v5::navigator_agreement_writer::PROFILE,
        crate::snapshot_v5::navigator_owners_writer::PROFILE,
    ]
    .contains(&info["profile"].as_str().unwrap_or(""))
    {
        result["coverage"]["companion_references"] = json!("resolved_structure_only");
        result["coverage"]["companion_contents"] = json!("not_analyzed");
    }
    if [
        crate::snapshot_v5::corpus_writer::PROFILE,
        crate::snapshot_v5::corpus_bin_writer::PROFILE,
        crate::snapshot_v5::corpus_help_writer::PROFILE,
        crate::snapshot_v5::corpus_config_writer::PROFILE,
        crate::snapshot_v5::diagnostics_writer::PROFILE,
        crate::snapshot_v5::optional_form_writer::PROFILE,
        crate::snapshot_v5::sidecar_writer::PROFILE,
        crate::snapshot_v5::context_writer::PROFILE,
        crate::snapshot_v5::parameter_writer::PROFILE,
        crate::snapshot_v5::conditional_writer::PROFILE,
        crate::snapshot_v5::string_writer::PROFILE,
        crate::snapshot_v5::literal_writer::PROFILE,
        crate::snapshot_v5::wrapped_writer::PROFILE,
        crate::snapshot_v5::refined_writer::PROFILE,
        crate::snapshot_v5::annotated_vars_writer::PROFILE,
        crate::snapshot_v5::dates_members_writer::PROFILE,
        crate::snapshot_v5::async_casing_writer::PROFILE,
        crate::snapshot_v5::mobile_vars_writer::PROFILE,
        crate::snapshot_v5::method_regions_writer::PROFILE,
        crate::snapshot_v5::header_forms_writer::PROFILE,
        crate::snapshot_v5::source_limits_writer::PROFILE,
        crate::snapshot_v5::long_headers_writer::PROFILE,
        crate::snapshot_v5::lexical_tail_writer::PROFILE,
        crate::snapshot_v5::source_anomalies_writer::PROFILE,
        crate::snapshot_v5::navigator_agreement_writer::PROFILE,
        crate::snapshot_v5::navigator_owners_writer::PROFILE,
    ]
    .contains(&info["profile"].as_str().unwrap_or(""))
    {
        result["coverage"]["corpus_structure"] = json!("complete_known_grammar");
        result["coverage"]["global_contents"] = json!("not_analyzed");
    }
    if [
        crate::snapshot_v5::workflow_writer::PROFILE,
        crate::snapshot_v5::corpus_writer::PROFILE,
        crate::snapshot_v5::corpus_bin_writer::PROFILE,
        crate::snapshot_v5::corpus_help_writer::PROFILE,
        crate::snapshot_v5::corpus_config_writer::PROFILE,
        crate::snapshot_v5::diagnostics_writer::PROFILE,
        crate::snapshot_v5::optional_form_writer::PROFILE,
        crate::snapshot_v5::sidecar_writer::PROFILE,
        crate::snapshot_v5::context_writer::PROFILE,
        crate::snapshot_v5::parameter_writer::PROFILE,
        crate::snapshot_v5::conditional_writer::PROFILE,
        crate::snapshot_v5::string_writer::PROFILE,
        crate::snapshot_v5::literal_writer::PROFILE,
        crate::snapshot_v5::wrapped_writer::PROFILE,
        crate::snapshot_v5::refined_writer::PROFILE,
        crate::snapshot_v5::annotated_vars_writer::PROFILE,
        crate::snapshot_v5::dates_members_writer::PROFILE,
        crate::snapshot_v5::async_casing_writer::PROFILE,
        crate::snapshot_v5::mobile_vars_writer::PROFILE,
        crate::snapshot_v5::method_regions_writer::PROFILE,
        crate::snapshot_v5::header_forms_writer::PROFILE,
        crate::snapshot_v5::source_limits_writer::PROFILE,
        crate::snapshot_v5::long_headers_writer::PROFILE,
        crate::snapshot_v5::lexical_tail_writer::PROFILE,
        crate::snapshot_v5::source_anomalies_writer::PROFILE,
        crate::snapshot_v5::navigator_agreement_writer::PROFILE,
        crate::snapshot_v5::navigator_owners_writer::PROFILE,
    ]
    .contains(&info["profile"].as_str().unwrap_or(""))
    {
        result["coverage"]["workflow_structure"] = json!("complete_known_grammar");
        result["coverage"]["workflow_semantics"] = json!("not_analyzed");
    }
    if [
        crate::snapshot_v5::report_writer::PROFILE,
        crate::snapshot_v5::workflow_writer::PROFILE,
        crate::snapshot_v5::corpus_writer::PROFILE,
        crate::snapshot_v5::corpus_bin_writer::PROFILE,
        crate::snapshot_v5::corpus_help_writer::PROFILE,
        crate::snapshot_v5::corpus_config_writer::PROFILE,
        crate::snapshot_v5::diagnostics_writer::PROFILE,
        crate::snapshot_v5::optional_form_writer::PROFILE,
        crate::snapshot_v5::sidecar_writer::PROFILE,
        crate::snapshot_v5::context_writer::PROFILE,
        crate::snapshot_v5::parameter_writer::PROFILE,
        crate::snapshot_v5::conditional_writer::PROFILE,
        crate::snapshot_v5::string_writer::PROFILE,
        crate::snapshot_v5::literal_writer::PROFILE,
        crate::snapshot_v5::wrapped_writer::PROFILE,
        crate::snapshot_v5::refined_writer::PROFILE,
        crate::snapshot_v5::annotated_vars_writer::PROFILE,
        crate::snapshot_v5::dates_members_writer::PROFILE,
        crate::snapshot_v5::async_casing_writer::PROFILE,
        crate::snapshot_v5::mobile_vars_writer::PROFILE,
        crate::snapshot_v5::method_regions_writer::PROFILE,
        crate::snapshot_v5::header_forms_writer::PROFILE,
        crate::snapshot_v5::source_limits_writer::PROFILE,
        crate::snapshot_v5::long_headers_writer::PROFILE,
        crate::snapshot_v5::lexical_tail_writer::PROFILE,
        crate::snapshot_v5::source_anomalies_writer::PROFILE,
        crate::snapshot_v5::navigator_agreement_writer::PROFILE,
        crate::snapshot_v5::navigator_owners_writer::PROFILE,
    ]
    .contains(&info["profile"].as_str().unwrap_or(""))
    {
        result["coverage"]["report_structure"] = json!("complete_known_grammar");
        result["coverage"]["report_semantics"] = json!("not_analyzed");
    }
    if [
        crate::snapshot_v5::register_writer::PROFILE,
        crate::snapshot_v5::report_writer::PROFILE,
        crate::snapshot_v5::workflow_writer::PROFILE,
        crate::snapshot_v5::corpus_writer::PROFILE,
        crate::snapshot_v5::corpus_bin_writer::PROFILE,
        crate::snapshot_v5::corpus_help_writer::PROFILE,
        crate::snapshot_v5::corpus_config_writer::PROFILE,
        crate::snapshot_v5::diagnostics_writer::PROFILE,
        crate::snapshot_v5::optional_form_writer::PROFILE,
        crate::snapshot_v5::sidecar_writer::PROFILE,
        crate::snapshot_v5::context_writer::PROFILE,
        crate::snapshot_v5::parameter_writer::PROFILE,
        crate::snapshot_v5::conditional_writer::PROFILE,
        crate::snapshot_v5::string_writer::PROFILE,
        crate::snapshot_v5::literal_writer::PROFILE,
        crate::snapshot_v5::wrapped_writer::PROFILE,
        crate::snapshot_v5::refined_writer::PROFILE,
        crate::snapshot_v5::annotated_vars_writer::PROFILE,
        crate::snapshot_v5::dates_members_writer::PROFILE,
        crate::snapshot_v5::async_casing_writer::PROFILE,
        crate::snapshot_v5::mobile_vars_writer::PROFILE,
        crate::snapshot_v5::method_regions_writer::PROFILE,
        crate::snapshot_v5::header_forms_writer::PROFILE,
        crate::snapshot_v5::source_limits_writer::PROFILE,
        crate::snapshot_v5::long_headers_writer::PROFILE,
        crate::snapshot_v5::lexical_tail_writer::PROFILE,
        crate::snapshot_v5::source_anomalies_writer::PROFILE,
        crate::snapshot_v5::navigator_agreement_writer::PROFILE,
        crate::snapshot_v5::navigator_owners_writer::PROFILE,
    ]
    .contains(&info["profile"].as_str().unwrap_or(""))
    {
        result["coverage"]["register_structure"] = json!("complete_known_grammar");
        result["coverage"]["register_semantics"] = json!("not_analyzed");
    }
    if [
        crate::snapshot_v5::chart_writer::PROFILE,
        crate::snapshot_v5::register_writer::PROFILE,
        crate::snapshot_v5::report_writer::PROFILE,
        crate::snapshot_v5::workflow_writer::PROFILE,
        crate::snapshot_v5::corpus_writer::PROFILE,
        crate::snapshot_v5::corpus_bin_writer::PROFILE,
        crate::snapshot_v5::corpus_help_writer::PROFILE,
        crate::snapshot_v5::corpus_config_writer::PROFILE,
        crate::snapshot_v5::diagnostics_writer::PROFILE,
        crate::snapshot_v5::optional_form_writer::PROFILE,
        crate::snapshot_v5::sidecar_writer::PROFILE,
        crate::snapshot_v5::context_writer::PROFILE,
        crate::snapshot_v5::parameter_writer::PROFILE,
        crate::snapshot_v5::conditional_writer::PROFILE,
        crate::snapshot_v5::string_writer::PROFILE,
        crate::snapshot_v5::literal_writer::PROFILE,
        crate::snapshot_v5::wrapped_writer::PROFILE,
        crate::snapshot_v5::refined_writer::PROFILE,
        crate::snapshot_v5::annotated_vars_writer::PROFILE,
        crate::snapshot_v5::dates_members_writer::PROFILE,
        crate::snapshot_v5::async_casing_writer::PROFILE,
        crate::snapshot_v5::mobile_vars_writer::PROFILE,
        crate::snapshot_v5::method_regions_writer::PROFILE,
        crate::snapshot_v5::header_forms_writer::PROFILE,
        crate::snapshot_v5::source_limits_writer::PROFILE,
        crate::snapshot_v5::long_headers_writer::PROFILE,
        crate::snapshot_v5::lexical_tail_writer::PROFILE,
        crate::snapshot_v5::source_anomalies_writer::PROFILE,
        crate::snapshot_v5::navigator_agreement_writer::PROFILE,
        crate::snapshot_v5::navigator_owners_writer::PROFILE,
    ]
    .contains(&info["profile"].as_str().unwrap_or(""))
    {
        result["coverage"]["chart_structure"] = json!("complete_known_grammar");
        result["coverage"]["chart_contents"] = json!("not_analyzed");
    }
    if [
        crate::snapshot_v5::service_writer::PROFILE,
        crate::snapshot_v5::chart_writer::PROFILE,
        crate::snapshot_v5::register_writer::PROFILE,
        crate::snapshot_v5::report_writer::PROFILE,
        crate::snapshot_v5::workflow_writer::PROFILE,
        crate::snapshot_v5::corpus_writer::PROFILE,
        crate::snapshot_v5::corpus_bin_writer::PROFILE,
        crate::snapshot_v5::corpus_help_writer::PROFILE,
        crate::snapshot_v5::corpus_config_writer::PROFILE,
        crate::snapshot_v5::diagnostics_writer::PROFILE,
        crate::snapshot_v5::optional_form_writer::PROFILE,
        crate::snapshot_v5::sidecar_writer::PROFILE,
        crate::snapshot_v5::context_writer::PROFILE,
        crate::snapshot_v5::parameter_writer::PROFILE,
        crate::snapshot_v5::conditional_writer::PROFILE,
        crate::snapshot_v5::string_writer::PROFILE,
        crate::snapshot_v5::literal_writer::PROFILE,
        crate::snapshot_v5::wrapped_writer::PROFILE,
        crate::snapshot_v5::refined_writer::PROFILE,
        crate::snapshot_v5::annotated_vars_writer::PROFILE,
        crate::snapshot_v5::dates_members_writer::PROFILE,
        crate::snapshot_v5::async_casing_writer::PROFILE,
        crate::snapshot_v5::mobile_vars_writer::PROFILE,
        crate::snapshot_v5::method_regions_writer::PROFILE,
        crate::snapshot_v5::header_forms_writer::PROFILE,
        crate::snapshot_v5::source_limits_writer::PROFILE,
        crate::snapshot_v5::long_headers_writer::PROFILE,
        crate::snapshot_v5::lexical_tail_writer::PROFILE,
        crate::snapshot_v5::source_anomalies_writer::PROFILE,
        crate::snapshot_v5::navigator_agreement_writer::PROFILE,
        crate::snapshot_v5::navigator_owners_writer::PROFILE,
    ]
    .contains(&info["profile"].as_str().unwrap_or(""))
    {
        result["coverage"]["service_structure"] = json!("complete_known_grammar");
        result["coverage"]["service_semantics"] = json!("not_analyzed");
    }
    if [
        crate::snapshot_v5::journal_writer::PROFILE,
        crate::snapshot_v5::service_writer::PROFILE,
        crate::snapshot_v5::chart_writer::PROFILE,
        crate::snapshot_v5::register_writer::PROFILE,
        crate::snapshot_v5::report_writer::PROFILE,
        crate::snapshot_v5::workflow_writer::PROFILE,
        crate::snapshot_v5::corpus_writer::PROFILE,
        crate::snapshot_v5::corpus_bin_writer::PROFILE,
        crate::snapshot_v5::corpus_help_writer::PROFILE,
        crate::snapshot_v5::corpus_config_writer::PROFILE,
        crate::snapshot_v5::diagnostics_writer::PROFILE,
        crate::snapshot_v5::optional_form_writer::PROFILE,
        crate::snapshot_v5::sidecar_writer::PROFILE,
        crate::snapshot_v5::context_writer::PROFILE,
        crate::snapshot_v5::parameter_writer::PROFILE,
        crate::snapshot_v5::conditional_writer::PROFILE,
        crate::snapshot_v5::string_writer::PROFILE,
        crate::snapshot_v5::literal_writer::PROFILE,
        crate::snapshot_v5::wrapped_writer::PROFILE,
        crate::snapshot_v5::refined_writer::PROFILE,
        crate::snapshot_v5::annotated_vars_writer::PROFILE,
        crate::snapshot_v5::dates_members_writer::PROFILE,
        crate::snapshot_v5::async_casing_writer::PROFILE,
        crate::snapshot_v5::mobile_vars_writer::PROFILE,
        crate::snapshot_v5::method_regions_writer::PROFILE,
        crate::snapshot_v5::header_forms_writer::PROFILE,
        crate::snapshot_v5::source_limits_writer::PROFILE,
        crate::snapshot_v5::long_headers_writer::PROFILE,
        crate::snapshot_v5::lexical_tail_writer::PROFILE,
        crate::snapshot_v5::source_anomalies_writer::PROFILE,
        crate::snapshot_v5::navigator_agreement_writer::PROFILE,
        crate::snapshot_v5::navigator_owners_writer::PROFILE,
    ]
    .contains(&info["profile"].as_str().unwrap_or(""))
    {
        result["coverage"]["journal_structure"] = json!("complete_known_grammar");
        result["coverage"]["journal_contents"] = json!("not_analyzed");
    }
    if [
        crate::snapshot_v5::leaf_writer::PROFILE,
        crate::snapshot_v5::journal_writer::PROFILE,
        crate::snapshot_v5::service_writer::PROFILE,
        crate::snapshot_v5::chart_writer::PROFILE,
        crate::snapshot_v5::register_writer::PROFILE,
        crate::snapshot_v5::report_writer::PROFILE,
        crate::snapshot_v5::workflow_writer::PROFILE,
        crate::snapshot_v5::corpus_writer::PROFILE,
        crate::snapshot_v5::corpus_bin_writer::PROFILE,
        crate::snapshot_v5::corpus_help_writer::PROFILE,
        crate::snapshot_v5::corpus_config_writer::PROFILE,
        crate::snapshot_v5::diagnostics_writer::PROFILE,
        crate::snapshot_v5::optional_form_writer::PROFILE,
        crate::snapshot_v5::sidecar_writer::PROFILE,
        crate::snapshot_v5::context_writer::PROFILE,
        crate::snapshot_v5::parameter_writer::PROFILE,
        crate::snapshot_v5::conditional_writer::PROFILE,
        crate::snapshot_v5::string_writer::PROFILE,
        crate::snapshot_v5::literal_writer::PROFILE,
        crate::snapshot_v5::wrapped_writer::PROFILE,
        crate::snapshot_v5::refined_writer::PROFILE,
        crate::snapshot_v5::annotated_vars_writer::PROFILE,
        crate::snapshot_v5::dates_members_writer::PROFILE,
        crate::snapshot_v5::async_casing_writer::PROFILE,
        crate::snapshot_v5::mobile_vars_writer::PROFILE,
        crate::snapshot_v5::method_regions_writer::PROFILE,
        crate::snapshot_v5::header_forms_writer::PROFILE,
        crate::snapshot_v5::source_limits_writer::PROFILE,
        crate::snapshot_v5::long_headers_writer::PROFILE,
        crate::snapshot_v5::lexical_tail_writer::PROFILE,
        crate::snapshot_v5::source_anomalies_writer::PROFILE,
        crate::snapshot_v5::navigator_agreement_writer::PROFILE,
        crate::snapshot_v5::navigator_owners_writer::PROFILE,
    ]
    .contains(&info["profile"].as_str().unwrap_or(""))
    {
        result["coverage"]["leaf_structure"] = json!("complete_known_grammar");
        result["coverage"]["leaf_contents"] = json!("not_analyzed");
    }
    if [
        crate::snapshot_v5::shared_writer::PROFILE,
        crate::snapshot_v5::leaf_writer::PROFILE,
        crate::snapshot_v5::journal_writer::PROFILE,
        crate::snapshot_v5::service_writer::PROFILE,
        crate::snapshot_v5::chart_writer::PROFILE,
        crate::snapshot_v5::register_writer::PROFILE,
        crate::snapshot_v5::report_writer::PROFILE,
        crate::snapshot_v5::workflow_writer::PROFILE,
        crate::snapshot_v5::corpus_writer::PROFILE,
        crate::snapshot_v5::corpus_bin_writer::PROFILE,
        crate::snapshot_v5::corpus_help_writer::PROFILE,
        crate::snapshot_v5::corpus_config_writer::PROFILE,
        crate::snapshot_v5::diagnostics_writer::PROFILE,
        crate::snapshot_v5::optional_form_writer::PROFILE,
        crate::snapshot_v5::sidecar_writer::PROFILE,
        crate::snapshot_v5::context_writer::PROFILE,
        crate::snapshot_v5::parameter_writer::PROFILE,
        crate::snapshot_v5::conditional_writer::PROFILE,
        crate::snapshot_v5::string_writer::PROFILE,
        crate::snapshot_v5::literal_writer::PROFILE,
        crate::snapshot_v5::wrapped_writer::PROFILE,
        crate::snapshot_v5::refined_writer::PROFILE,
        crate::snapshot_v5::annotated_vars_writer::PROFILE,
        crate::snapshot_v5::dates_members_writer::PROFILE,
        crate::snapshot_v5::async_casing_writer::PROFILE,
        crate::snapshot_v5::mobile_vars_writer::PROFILE,
        crate::snapshot_v5::method_regions_writer::PROFILE,
        crate::snapshot_v5::header_forms_writer::PROFILE,
        crate::snapshot_v5::source_limits_writer::PROFILE,
        crate::snapshot_v5::long_headers_writer::PROFILE,
        crate::snapshot_v5::lexical_tail_writer::PROFILE,
        crate::snapshot_v5::source_anomalies_writer::PROFILE,
        crate::snapshot_v5::navigator_agreement_writer::PROFILE,
        crate::snapshot_v5::navigator_owners_writer::PROFILE,
    ]
    .contains(&info["profile"].as_str().unwrap_or(""))
    {
        result["coverage"]["shared_structure"] = json!("complete_known_grammar");
        result["coverage"]["shared_contents"] = json!("not_analyzed");
    }
    // A second fixed bound includes JSON escaping of scope_id; never truncate identity.
    if result.to_string().len() > 12_288 {
        return Err(Error::Limit);
    }
    Ok(result)
}

/// Stable, non-sensitive startup refusals. No caller paths or source content in stderr.
pub fn startup_code(error: &Error) -> &'static str {
    match error {
        Error::Version => "snapshot_version",
        Error::Profile => "snapshot_profile",
        Error::Identity => "snapshot_identity",
        Error::Limit | Error::Staging(StagingError::Limit) => "snapshot_budget_exceeded",
        Error::Staging(StagingError::Digest) => "snapshot_checksum",
        Error::Staging(StagingError::NotStandalone) => "snapshot_not_standalone",
        Error::Staging(_) => "snapshot_staging",
        Error::Facts | Error::ForeignKey | Error::Schema => "snapshot_integrity",
        _ => "snapshot_rejected",
    }
}
struct Router {
    reader: LinuxSnapshotReader,
    metadata: Value,
}
fn keys(v: &Value, allowed: &[&str]) -> Result<(), RouteError> {
    if v.as_object()
        .is_none_or(|o| o.keys().any(|k| !allowed.contains(&k.as_str())))
    {
        return Err(RouteError::InvalidArgs);
    }
    Ok(())
}
fn text<'a>(v: &'a Value, key: &str, max: usize) -> Result<&'a str, RouteError> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= max)
        .ok_or(RouteError::InvalidArgs)
}
fn number(v: &Value, key: &str) -> Result<i64, RouteError> {
    v.get(key)
        .and_then(Value::as_i64)
        .filter(|n| *n > 0)
        .ok_or(RouteError::InvalidArgs)
}
fn failure(code: &'static str) -> Value {
    let value = json!({"status":"error","code":code});
    json!({"isError":true,"structuredContent":value,"content":[{"type":"text","text":value.to_string()}]})
}
fn refusal(error: Error) -> Value {
    failure(match error {
        Error::Limit => "budget_exceeded",
        Error::Sqlite(rusqlite::Error::QueryReturnedNoRows) => "not_found",
        Error::Range => "source_range",
        _ => "snapshot_read_refused",
    })
}
impl Router {
    fn success(&self, data: Value) -> Value {
        let value = json!({"snapshot":self.metadata,"data":data});
        // The existing transport checks the entire envelope AFTER both representations
        // are serialized, including escaping, request ID and base64 expansion.
        json!({"isError":false,"structuredContent":value,"content":[{"type":"text","text":value.to_string()}]})
    }
    fn tree(&self, args: &Value) -> Result<Value, RouteError> {
        if args["level"] == "companions" {
            if !self.has_companions() {
                return Err(RouteError::InvalidArgs);
            }
            keys(args, &["level", "parent_id", "max_nodes"])?;
            let owner = number(args, "parent_id")?;
            let max = args
                .get("max_nodes")
                .map(|_| number(args, "max_nodes"))
                .transpose()?
                .unwrap_or(MAX_TREE_NODES as i64);
            if max > MAX_TREE_NODES as i64 {
                return Err(RouteError::InvalidArgs);
            }
            return Ok(
                match self.reader.companion_branch(
                    owner,
                    TreeLimits {
                        rows: max as usize,
                        bytes: MAX_TREE_TEXT_BYTES,
                    },
                ) {
                    Ok(v) => self.success(v),
                    Err(e) => refusal(e),
                },
            );
        }
        if args["level"] == "inline" {
            if !self.has_inline() {
                return Err(RouteError::InvalidArgs);
            }
            keys(
                args,
                &["level", "parent_id", "declaration_parent", "max_nodes"],
            )?;
            let owner = number(args, "parent_id")?;
            let parent = args
                .get("declaration_parent")
                .map(|_| text(args, "declaration_parent", 1100))
                .transpose()?;
            let max = args
                .get("max_nodes")
                .map(|_| number(args, "max_nodes"))
                .transpose()?
                .unwrap_or(MAX_TREE_NODES as i64);
            if max > MAX_TREE_NODES as i64 {
                return Err(RouteError::InvalidArgs);
            }
            return Ok(
                match self.reader.declarations(
                    owner,
                    parent,
                    TreeLimits {
                        rows: max as usize,
                        bytes: MAX_TREE_TEXT_BYTES,
                    },
                ) {
                    Ok(v) => self.success(v),
                    Err(e) => refusal(e),
                },
            );
        }
        keys(
            args,
            &[
                "level",
                "parent_id",
                "kind",
                "module_path",
                "max_nodes",
                "name_exact",
            ],
        )?;
        let level = if args.get("level").is_some() {
            text(args, "level", 32)?
        } else {
            "configuration"
        };
        if !["configuration", "kind", "object", "unconfirmed", "module"].contains(&level) {
            return Err(RouteError::InvalidArgs);
        }
        let parent = args
            .get("parent_id")
            .map(|_| number(args, "parent_id"))
            .transpose()?;
        let kind = args
            .get("kind")
            .map(|_| text(args, "kind", 128))
            .transpose()?;
        let path = args
            .get("module_path")
            .map(|_| text(args, "module_path", 1024))
            .transpose()?;
        let name_exact = args
            .get("name_exact")
            .map(|_| text(args, "name_exact", 1024))
            .transpose()?;
        if name_exact.is_some() && !["kind", "module"].contains(&level) {
            return Err(RouteError::InvalidArgs);
        }
        let shape = match level {
            "configuration" => kind.is_none() && path.is_none(),
            "object" => parent.is_some() && kind.is_none() && path.is_none(),
            "kind" => parent.is_some() && kind.is_some() && path.is_none(),
            "module" => parent.is_none() && kind.is_none() && path.is_some(),
            _ => parent.is_none() && kind.is_none() && path.is_none(),
        };
        if !shape {
            return Err(RouteError::InvalidArgs);
        }
        let max = args
            .get("max_nodes")
            .map(|_| number(args, "max_nodes"))
            .transpose()?
            .unwrap_or(MAX_TREE_NODES as i64);
        if max > MAX_TREE_NODES as i64 {
            return Err(RouteError::InvalidArgs);
        }
        let (parent, mut branch) = match self.reader.branch_info(level, parent, kind, path) {
            Ok(v) => v,
            Err(e) => return Ok(refusal(e)),
        };
        if level == "module" {
            if let Some(path) = path {
                if let Err(e) = self.qualify_source(path, &mut branch) {
                    return Ok(refusal(e));
                }
            }
        }
        let nodes = match self.reader.tree_named(
            level,
            parent,
            kind,
            path,
            name_exact,
            TreeLimits {
                rows: max as usize,
                bytes: MAX_TREE_TEXT_BYTES,
            },
        ) {
            Ok(v) => v,
            Err(e) => return Ok(refusal(e)),
        };
        if let Some(name) = name_exact {
            branch["name_exact"] = json!(name);
            branch["name_comparison"] = json!("utf8_binary_case_sensitive_no_normalization");
            branch["match_status"] = json!(if branch["status"] == "unparsed" {
                "unparsed"
            } else if nodes.is_empty() {
                "no_match"
            } else if nodes.len() == 1 {
                "matched"
            } else {
                "ambiguous"
            });
        }
        let mut items = Vec::new();
        for n in nodes {
            let mut item = json!({"type":n.node_type,"key":n.key,"name":n.name,"parse_status":n.status,"reason":n.reason});
            let children = match n.node_type.as_str() {
                "kind" => Some(json!({"level":"kind","parent_id":parent,"kind":n.key})),
                "object" => Some(
                    json!({"level":"object","parent_id":n.key.parse::<i64>().map_err(|_|RouteError::InvalidArgs)?}),
                ),
                "module" => {
                    if [
                        crate::snapshot_v5::journal_writer::PROFILE,
                        crate::snapshot_v5::service_writer::PROFILE,
                        crate::snapshot_v5::chart_writer::PROFILE,
                        crate::snapshot_v5::register_writer::PROFILE,
                        crate::snapshot_v5::report_writer::PROFILE,
                        crate::snapshot_v5::workflow_writer::PROFILE,
                        crate::snapshot_v5::corpus_writer::PROFILE,
                        crate::snapshot_v5::corpus_bin_writer::PROFILE,
                        crate::snapshot_v5::corpus_help_writer::PROFILE,
                        crate::snapshot_v5::corpus_config_writer::PROFILE,
                        crate::snapshot_v5::diagnostics_writer::PROFILE,
                        crate::snapshot_v5::optional_form_writer::PROFILE,
                        crate::snapshot_v5::sidecar_writer::PROFILE,
                        crate::snapshot_v5::context_writer::PROFILE,
                        crate::snapshot_v5::parameter_writer::PROFILE,
                        crate::snapshot_v5::conditional_writer::PROFILE,
                        crate::snapshot_v5::string_writer::PROFILE,
                        crate::snapshot_v5::literal_writer::PROFILE,
                        crate::snapshot_v5::wrapped_writer::PROFILE,
                        crate::snapshot_v5::refined_writer::PROFILE,
                        crate::snapshot_v5::annotated_vars_writer::PROFILE,
                        crate::snapshot_v5::dates_members_writer::PROFILE,
                        crate::snapshot_v5::async_casing_writer::PROFILE,
                        crate::snapshot_v5::mobile_vars_writer::PROFILE,
                        crate::snapshot_v5::method_regions_writer::PROFILE,
                        crate::snapshot_v5::header_forms_writer::PROFILE,
                        crate::snapshot_v5::source_limits_writer::PROFILE,
                        crate::snapshot_v5::long_headers_writer::PROFILE,
                        crate::snapshot_v5::lexical_tail_writer::PROFILE,
                        crate::snapshot_v5::source_anomalies_writer::PROFILE,
                        crate::snapshot_v5::navigator_agreement_writer::PROFILE,
                        crate::snapshot_v5::navigator_owners_writer::PROFILE,
                    ]
                    .contains(&self.metadata["profile"].as_str().unwrap_or(""))
                    {
                        let (_, owner) =
                            match self.reader.branch_info("module", None, None, Some(&n.key)) {
                                Ok(v) => v,
                                Err(e) => return Ok(refusal(e)),
                            };
                        if !owner["owner_declaration"].is_null() {
                            item["owner_declaration"] = owner["owner_declaration"].clone();
                            item["container_object_id"] = owner["container_object_id"].clone();
                        }
                    }
                    if let Err(e) = self.qualify_source(&n.key, &mut item) {
                        return Ok(refusal(e));
                    }
                    Some(json!({"level":"module","module_path":n.key}))
                }
                "unconfirmed" => Some(json!({"level":"unconfirmed"})),
                "method" => {
                    let id = n.key.parse::<i64>().map_err(|_| RouteError::InvalidArgs)?;
                    let location = match self.reader.method_location(id) {
                        Ok(v) => v,
                        Err(e) => return Ok(refusal(e)),
                    };
                    item["source"] = json!({"path":path,"start_line":location["start_line"],"end_line":location["end_line"]});
                    item["location"] = location;
                    if let Some(status) = branch.get("structural_status") {
                        item["structural_status"] = status.clone();
                    }
                    None
                }
                _ => None,
            };
            item["children"] = json!(children);
            items.push(item);
        }
        if level == "object" && self.has_inline() && branch["descriptor_only"] != true {
            if items.len() >= max as usize {
                return Ok(refusal(Error::Limit));
            }
            items.push(json!({"type":"inline_group","name":"Встроенные объявления","children":{"level":"inline","parent_id":parent}}));
        }
        if (level == "object"
            || (level == "configuration"
                && [
                    crate::snapshot_v5::corpus_writer::PROFILE,
                    crate::snapshot_v5::corpus_bin_writer::PROFILE,
                    crate::snapshot_v5::corpus_help_writer::PROFILE,
                    crate::snapshot_v5::corpus_config_writer::PROFILE,
                    crate::snapshot_v5::diagnostics_writer::PROFILE,
                    crate::snapshot_v5::optional_form_writer::PROFILE,
                    crate::snapshot_v5::sidecar_writer::PROFILE,
                    crate::snapshot_v5::context_writer::PROFILE,
                    crate::snapshot_v5::parameter_writer::PROFILE,
                    crate::snapshot_v5::conditional_writer::PROFILE,
                    crate::snapshot_v5::string_writer::PROFILE,
                    crate::snapshot_v5::literal_writer::PROFILE,
                    crate::snapshot_v5::wrapped_writer::PROFILE,
                    crate::snapshot_v5::refined_writer::PROFILE,
                    crate::snapshot_v5::annotated_vars_writer::PROFILE,
                    crate::snapshot_v5::dates_members_writer::PROFILE,
                    crate::snapshot_v5::async_casing_writer::PROFILE,
                    crate::snapshot_v5::mobile_vars_writer::PROFILE,
                    crate::snapshot_v5::method_regions_writer::PROFILE,
                    crate::snapshot_v5::header_forms_writer::PROFILE,
                    crate::snapshot_v5::source_limits_writer::PROFILE,
                    crate::snapshot_v5::long_headers_writer::PROFILE,
                    crate::snapshot_v5::lexical_tail_writer::PROFILE,
                    crate::snapshot_v5::source_anomalies_writer::PROFILE,
                    crate::snapshot_v5::navigator_agreement_writer::PROFILE,
                    crate::snapshot_v5::navigator_owners_writer::PROFILE,
                ]
                .contains(&self.metadata["profile"].as_str().unwrap_or(""))))
            && self.has_companions()
            && branch["descriptor_only"] != true
        {
            if items.len() >= max as usize {
                return Ok(refusal(Error::Limit));
            }
            items.push(json!({"type":"companion_group","name":"Формы, макеты и сохранённые компаньоны","children":{"level":"companions","parent_id":parent}}));
        }
        let mut result =
            json!({"branch":branch,"items":items,"complete_branch":name_exact.is_none()});
        if name_exact.is_some() {
            result["complete_matches"] = json!(branch["status"] != "unparsed");
        }
        Ok(self.success(result))
    }
    fn has_inline(&self) -> bool {
        [
            crate::snapshot_v5::inline_writer::PROFILE,
            crate::snapshot_v5::companion_writer::PROFILE,
            crate::snapshot_v5::common_writer::PROFILE,
            crate::snapshot_v5::root_writer::PROFILE,
            crate::snapshot_v5::shared_writer::PROFILE,
            crate::snapshot_v5::leaf_writer::PROFILE,
            crate::snapshot_v5::journal_writer::PROFILE,
            crate::snapshot_v5::service_writer::PROFILE,
            crate::snapshot_v5::chart_writer::PROFILE,
            crate::snapshot_v5::register_writer::PROFILE,
            crate::snapshot_v5::report_writer::PROFILE,
            crate::snapshot_v5::workflow_writer::PROFILE,
            crate::snapshot_v5::corpus_writer::PROFILE,
            crate::snapshot_v5::corpus_bin_writer::PROFILE,
            crate::snapshot_v5::corpus_help_writer::PROFILE,
            crate::snapshot_v5::corpus_config_writer::PROFILE,
            crate::snapshot_v5::diagnostics_writer::PROFILE,
            crate::snapshot_v5::optional_form_writer::PROFILE,
            crate::snapshot_v5::sidecar_writer::PROFILE,
            crate::snapshot_v5::context_writer::PROFILE,
            crate::snapshot_v5::parameter_writer::PROFILE,
            crate::snapshot_v5::conditional_writer::PROFILE,
            crate::snapshot_v5::string_writer::PROFILE,
            crate::snapshot_v5::literal_writer::PROFILE,
            crate::snapshot_v5::wrapped_writer::PROFILE,
            crate::snapshot_v5::refined_writer::PROFILE,
            crate::snapshot_v5::annotated_vars_writer::PROFILE,
            crate::snapshot_v5::dates_members_writer::PROFILE,
            crate::snapshot_v5::async_casing_writer::PROFILE,
            crate::snapshot_v5::mobile_vars_writer::PROFILE,
            crate::snapshot_v5::method_regions_writer::PROFILE,
            crate::snapshot_v5::header_forms_writer::PROFILE,
            crate::snapshot_v5::source_limits_writer::PROFILE,
            crate::snapshot_v5::long_headers_writer::PROFILE,
            crate::snapshot_v5::lexical_tail_writer::PROFILE,
            crate::snapshot_v5::source_anomalies_writer::PROFILE,
            crate::snapshot_v5::navigator_agreement_writer::PROFILE,
            crate::snapshot_v5::navigator_owners_writer::PROFILE,
        ]
        .contains(&self.metadata["profile"].as_str().unwrap_or(""))
    }
    fn has_companions(&self) -> bool {
        [
            crate::snapshot_v5::companion_writer::PROFILE,
            crate::snapshot_v5::common_writer::PROFILE,
            crate::snapshot_v5::root_writer::PROFILE,
            crate::snapshot_v5::shared_writer::PROFILE,
            crate::snapshot_v5::leaf_writer::PROFILE,
            crate::snapshot_v5::journal_writer::PROFILE,
            crate::snapshot_v5::service_writer::PROFILE,
            crate::snapshot_v5::chart_writer::PROFILE,
            crate::snapshot_v5::register_writer::PROFILE,
            crate::snapshot_v5::report_writer::PROFILE,
            crate::snapshot_v5::workflow_writer::PROFILE,
            crate::snapshot_v5::corpus_writer::PROFILE,
            crate::snapshot_v5::corpus_bin_writer::PROFILE,
            crate::snapshot_v5::corpus_help_writer::PROFILE,
            crate::snapshot_v5::corpus_config_writer::PROFILE,
            crate::snapshot_v5::diagnostics_writer::PROFILE,
            crate::snapshot_v5::optional_form_writer::PROFILE,
            crate::snapshot_v5::sidecar_writer::PROFILE,
            crate::snapshot_v5::context_writer::PROFILE,
            crate::snapshot_v5::parameter_writer::PROFILE,
            crate::snapshot_v5::conditional_writer::PROFILE,
            crate::snapshot_v5::string_writer::PROFILE,
            crate::snapshot_v5::literal_writer::PROFILE,
            crate::snapshot_v5::wrapped_writer::PROFILE,
            crate::snapshot_v5::refined_writer::PROFILE,
            crate::snapshot_v5::annotated_vars_writer::PROFILE,
            crate::snapshot_v5::dates_members_writer::PROFILE,
            crate::snapshot_v5::async_casing_writer::PROFILE,
            crate::snapshot_v5::mobile_vars_writer::PROFILE,
            crate::snapshot_v5::method_regions_writer::PROFILE,
            crate::snapshot_v5::header_forms_writer::PROFILE,
            crate::snapshot_v5::source_limits_writer::PROFILE,
            crate::snapshot_v5::long_headers_writer::PROFILE,
            crate::snapshot_v5::lexical_tail_writer::PROFILE,
            crate::snapshot_v5::source_anomalies_writer::PROFILE,
            crate::snapshot_v5::navigator_agreement_writer::PROFILE,
            crate::snapshot_v5::navigator_owners_writer::PROFILE,
        ]
        .contains(&self.metadata["profile"].as_str().unwrap_or(""))
    }
    fn qualify_source(&self, path: &str, response: &mut Value) -> Result<(), Error> {
        if [
            crate::snapshot_v5::source_anomalies_writer::PROFILE,
            crate::snapshot_v5::navigator_agreement_writer::PROFILE,
            crate::snapshot_v5::navigator_owners_writer::PROFILE,
        ].contains(&self.metadata["profile"].as_str().unwrap_or("")) {
            if let Some(site) = self.reader.source_anomaly(path)? {
                response["structural_status"] = json!("parsed_with_source_anomalies");
                response["source_anomaly"] = site;
                response["compiler_validation"] = json!("not_performed");
            }
        }
        Ok(())
    }

    fn source(&self, args: &Value) -> Result<Value, RouteError> {
        if args.get("saved_path").is_some() {
            if !self.has_companions() {
                return Err(RouteError::InvalidArgs);
            }
            keys(args, &["saved_path", "start_byte", "end_byte"])?;
            let path = text(args, "saved_path", 1024)?;
            let a = args["start_byte"].as_u64().ok_or(RouteError::InvalidArgs)?;
            let b = args["end_byte"].as_u64().ok_or(RouteError::InvalidArgs)?;
            return Ok(
                match self
                    .reader
                    .saved_range(path, a, b, MAX_SOURCE_BYTES as usize)
                {
                    Ok((bytes, kind)) => {
                        let (encoding, data) = if kind == "payload" {
                            ("base64", base64(&bytes))
                        } else {
                            match std::str::from_utf8(&bytes) {
                                Ok(s) => ("utf-8", s.to_owned()),
                                Err(_) => ("base64", base64(&bytes)),
                            }
                        };
                        self.success(json!({"path":path,"kind":kind,"contents":"not_analyzed","start_byte":a,"end_byte":b,"byte_units":"0_based_end_exclusive","encoding":encoding,"data":data,"byte_length":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}))
                    }
                    Err(e) => refusal(e),
                },
            );
        }
        if args.get("declaration_key").is_some() {
            if !self.has_inline() {
                return Err(RouteError::InvalidArgs);
            }
            keys(args, &["declaration_key"])?;
            let key = text(args, "declaration_key", 1100)?;
            return Ok(
                match self.reader.declaration_source(key, MAX_SOURCE_BYTES) {
                    Ok((bytes, mut location)) => {
                        let (encoding, data) = match std::str::from_utf8(&bytes) {
                            Ok(s) => ("utf-8", s.to_owned()),
                            Err(_) => ("base64", base64(&bytes)),
                        };
                        location["byte_length"] = json!(bytes.len());
                        location["sha256"] = json!(format!("{:x}", Sha256::digest(&bytes)));
                        location["encoding"] = json!(encoding);
                        location["data"] = json!(data);
                        self.success(location)
                    }
                    Err(e) => refusal(e),
                },
            );
        }
        keys(args, &["path", "start_line", "end_line"])?;
        let path = text(args, "path", 1024)?;
        if path.starts_with('/')
            || path.contains(['\\', '\0'])
            || path
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
        {
            return Err(RouteError::InvalidArgs);
        }
        let start = number(args, "start_line")?;
        let end = number(args, "end_line")?;
        if end < start {
            return Err(RouteError::InvalidArgs);
        }
        let (_, module) = match self.reader.branch_info("module", None, None, Some(path)) {
            Ok(v) => v,
            Err(e) => return Ok(refusal(e)),
        };
        let (bytes, a, b) = match self.reader.source_span(path, start, end, MAX_SOURCE_BYTES) {
            Ok(v) => v,
            Err(e) => return Ok(refusal(e)),
        };
        let (encoding, data) = match std::str::from_utf8(&bytes) {
            Ok(s) => ("utf-8", s.to_owned()),
            Err(_) => ("base64", base64(&bytes)),
        };
        let mut response = json!({"status":"ok","path":path,"parse_status":module["status"],"reason":module["reason"],"start_line":start,"end_line":end,"line_units":"physical_1_based_inclusive","start_byte":a,"end_byte":b,"byte_units":"0_based_end_exclusive","byte_length":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"encoding":encoding,"data":data});
        if let Err(e) = self.qualify_source(path, &mut response) {
            return Ok(refusal(e));
        }
        Ok(self.success(response))
    }
}
impl ToolRouter for Router {
    fn list(&self) -> Value {
        let annotations = json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false});
        let mut result = json!({"tools":[
            {"name":"metadata","description":"Fixed autonomous v5 profile, scope and temporary coverage; relations not analyzed, Git provenance not verified","inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":annotations},
            {"name":"tree","description":"One bounded stored branch; use children arguments, accepted and unparsed are distinct","inputSchema":{"type":"object","additionalProperties":false,"properties":{"level":{"enum":["configuration","kind","object","module","unconfirmed","inline"]},"parent_id":{"type":"integer","minimum":1},"declaration_parent":{"type":"string","maxLength":1100},"kind":{"type":"string","maxLength":128},"module_path":{"type":"string","maxLength":1024},"max_nodes":{"type":"integer","minimum":1,"maximum":MAX_TREE_NODES}}},"annotations":annotations},
            {"name":"source","description":"Exact physical line bytes; encoding is utf-8 or padded RFC4648 base64, never lossy","inputSchema":{"type":"object","additionalProperties":false,"oneOf":[{"required":["path","start_line","end_line"]},{"required":["declaration_key"]}],"properties":{"declaration_key":{"type":"string","maxLength":1100},"path":{"type":"string","maxLength":1024},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}}},"annotations":annotations}
        ]});
        result["tools"][1]["inputSchema"]["properties"]["name_exact"] = json!({"type":"string","minLength":1,"maxLength":1024,"description":"Exact case-sensitive UTF-8 name, no normalization; only kind (objects) or module (methods). Follow returned children; all matches or a budget refusal."});
        if self.has_inline() {
            result["tools"][2]["description"]=json!("Exact BSL physical lines or stored inline XML byte range; UTF-8 or Base64, never lossy");
        }
        if !self.has_inline() {
            result["tools"][1]["inputSchema"]["properties"]["level"]["enum"] =
                json!(["configuration", "kind", "object", "module", "unconfirmed"]);
            result["tools"][1]["inputSchema"]["properties"]
                .as_object_mut()
                .unwrap()
                .remove("declaration_parent");
            let schema = result["tools"][2]["inputSchema"].as_object_mut().unwrap();
            schema.remove("oneOf");
            schema.insert("required".into(), json!(["path", "start_line", "end_line"]));
            schema
                .get_mut("properties")
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove("declaration_key");
        }
        if self.has_companions() {
            result["tools"][1]["inputSchema"]["properties"]["level"]["enum"]
                .as_array_mut()
                .unwrap()
                .push(json!("companions"));
            let schema = &mut result["tools"][2]["inputSchema"];
            schema["oneOf"]
                .as_array_mut()
                .unwrap()
                .push(json!({"required":["saved_path","start_byte","end_byte"]}));
            schema["properties"]["saved_path"] = json!({"type":"string","maxLength":1024});
            schema["properties"]["start_byte"] = json!({"type":"integer","minimum":0});
            schema["properties"]["end_byte"] = json!({"type":"integer","minimum":0});
        }
        result
    }
    fn call(&self, params: &Value) -> Result<Value, RouteError> {
        keys(params, &["name", "arguments", "_meta"])?;
        let empty = json!({});
        let args = params.get("arguments").unwrap_or(&empty);
        if !args.is_object() {
            return Err(RouteError::InvalidArgs);
        }
        match text(params, "name", 64)? {
            "metadata" => {
                keys(args, &[])?;
                Ok(self.success(json!({"status":"ok","source_budget_bytes":MAX_SOURCE_BYTES,"tree_nodes":MAX_TREE_NODES,"tree_text_budget_bytes":MAX_TREE_TEXT_BYTES})))
            }
            "tree" => self.tree(args),
            "source" => self.source(args),
            _ => Err(RouteError::InvalidArgs),
        }
    }
    fn instructions(&self) -> &'static str {
        "Experimental Linux x86_64 v5 snapshot, one immutable reader per session. Read-only metadata/tree/source; no Git or BSL reparse. Relations not analyzed; Git provenance not verified."
    }
}
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        result.push(ALPHABET[(a >> 2) as usize] as char);
        result.push(ALPHABET[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        result.push(if chunk.len() > 1 {
            ALPHABET[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            ALPHABET[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn base64_rfc4648_vectors_and_binary() {
        for (input, expected) in [
            (b"".as_slice(), ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"fooba", "Zm9vYmE="),
            (b"foobar", "Zm9vYmFy"),
            (&[0xff, 0, 0x80], "/wCA"),
        ] {
            assert_eq!(base64(input), expected);
        }
    }
}
