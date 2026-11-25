//! Experimental Linux v5 reader. Offline verification does not attest Git provenance.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub use crate::linux_v5_staging::Error as StagingError;
use crate::linux_v5_staging::{self, StagedSnapshot};
use crate::snapshot_v5::{self, contract};
use rusqlite::{Connection, DatabaseName, OpenFlags};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

#[derive(Debug)]
pub enum Error {
    Staging(StagingError),
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    Version,
    Schema,
    Profile,
    Identity,
    Facts,
    ForeignKey,
    Range,
    Limit,
    Contract(String),
    SourceDigest,
    VerifyFacts,
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<snapshot_v5::Error> for Error {
    fn from(e: snapshot_v5::Error) -> Self {
        match e {
            snapshot_v5::Error::Contract("user_version") => Self::Version,
            snapshot_v5::Error::Contract("schema") => Self::Schema,
            snapshot_v5::Error::Contract(
                "schema_count" | "schema_field_budget" | "metadata_field_budget",
            ) => Self::Limit,
            snapshot_v5::Error::Contract("profile" | "incompatible_profile") => Self::Profile,
            other => Self::Contract(other.to_string()),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v5 reader: {self:?}")
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

pub struct LinuxSnapshotReader {
    // Field drop order is deliberate: close SQLite before unlinking the copy.
    connection: Connection,
    _copy: StagedSnapshot,
}

/// Trusted caller policy; text bytes are UTF-8 payload bytes across all five fields.
/// Row count independently bounds per-node/container overhead. No partial response.
#[derive(Clone, Copy)]
pub struct TreeLimits {
    pub rows: usize,
    pub bytes: usize,
}

fn read_tree(
    connection: &Connection,
    level: &str,
    parent_id: Option<i64>,
    kind: Option<&str>,
    module_path: Option<&str>,
    limits: TreeLimits,
) -> Result<Vec<TreeNode>> {
    read_tree_named(
        connection,
        level,
        parent_id,
        kind,
        module_path,
        None,
        limits,
    )
}
fn read_tree_named(
    connection: &Connection,
    level: &str,
    parent_id: Option<i64>,
    kind: Option<&str>,
    module_path: Option<&str>,
    name_exact: Option<&str>,
    limits: TreeLimits,
) -> Result<Vec<TreeNode>> {
    if !["configuration", "kind", "object", "unconfirmed", "module"].contains(&level) {
        return Err(Error::Range);
    }
    if name_exact.is_some_and(|n| n.is_empty() || n.len() > 1024)
        || (name_exact.is_some() && !["kind", "module"].contains(&level))
    {
        return Err(Error::Range);
    }
    let sql = include_str!("../spikes/linux-v5/branch.sql");
    let (cte, order) = sql
        .split_once("SELECT node_type,node_key,name,status,reason_code FROM branch")
        .ok_or(Error::Schema)?;
    let preflight = format!("{cte} SELECT length(CAST(node_type AS BLOB)),length(CAST(node_key AS BLOB)),length(CAST(name AS BLOB)),coalesce(length(CAST(status AS BLOB)),0),coalesce(length(CAST(reason_code AS BLOB)),0) FROM branch WHERE (:name_exact IS NULL OR name COLLATE BINARY = :name_exact) LIMIT :row_limit");
    let row_limit = i64::try_from(limits.rows)
        .ok()
        .and_then(|n| n.checked_add(1))
        .ok_or(Error::Limit)?;
    let mut query = connection.prepare(&preflight)?;
    let mut rows = query.query(rusqlite::named_params! {":level":level, ":parent_id":parent_id, ":kind":kind, ":module_path":module_path, ":name_exact":name_exact, ":row_limit":row_limit})?;
    let mut count = 0usize;
    let mut bytes = 0usize;
    while let Some(row) = rows.next()? {
        if count == limits.rows {
            return Err(Error::Limit);
        }
        for i in 0..5 {
            let size = usize::try_from(row.get::<_, i64>(i)?).map_err(|_| Error::Limit)?;
            bytes = bytes
                .checked_add(size)
                .filter(|n| *n <= limits.bytes)
                .ok_or(Error::Limit)?;
        }
        count += 1;
    }
    // The immutable private copy cannot change between preflight and retrieval.
    // ORDER BY and Rust strings only see a branch already bounded in rows and bytes.
    let selected=format!("{cte} SELECT node_type,node_key,name,status,reason_code FROM branch WHERE (:name_exact IS NULL OR name COLLATE BINARY = :name_exact) {order}");
    let mut query = connection.prepare(&selected)?;
    let mut rows = query.query(rusqlite::named_params! {":level":level, ":parent_id":parent_id, ":kind":kind, ":module_path":module_path, ":name_exact":name_exact})?;
    let mut nodes = Vec::new();
    let mut remaining = limits.bytes;
    while let Some(row) = rows.next()? {
        if nodes.len() == limits.rows {
            return Err(Error::Limit);
        }
        // Recheck borrowed bytes before any String allocation.
        for i in 0..5 {
            use rusqlite::types::ValueRef;
            let size = match row.get_ref(i)? {
                ValueRef::Text(v) => v.len(),
                ValueRef::Null => 0,
                _ => return Err(Error::Schema),
            };
            remaining = remaining.checked_sub(size).ok_or(Error::Limit)?;
        }
        nodes.push(TreeNode {
            node_type: row.get(0)?,
            key: row.get(1)?,
            name: row.get(2)?,
            status: row.get(3)?,
            reason: row.get(4)?,
        });
    }
    Ok(nodes)
}
impl LinuxSnapshotReader {
    pub fn open(
        path: &Path,
        staging: &Path,
        expected: Option<&str>,
        max_bytes: u64,
    ) -> Result<Self> {
        Self::open_with(path, staging, expected, max_bytes, |copy| {
            Connection::open_with_flags(
                copy,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
        })
    }

    // Private seam verifies the checksum/header ordering without opening SQLite in tests.
    fn open_with(
        path: &Path,
        staging: &Path,
        expected: Option<&str>,
        max_bytes: u64,
        open_sqlite: impl FnOnce(&Path) -> rusqlite::Result<Connection>,
    ) -> Result<Self> {
        let copy =
            linux_v5_staging::stage(path, staging, expected, max_bytes).map_err(Error::Staging)?;
        copy.validate_standalone().map_err(Error::Staging)?;
        // SQLite receives only a checksum-verified, rollback-mode private copy.
        let connection = open_sqlite(&copy.path)?;
        connection.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")?;
        connection.execute_batch("BEGIN;")?;
        // One shared bounded schema/profile/metadata/compatibility validator from S3.
        contract::compatible_scope(&connection)?;
        let (id, facts): (String, Option<String>) = connection.query_row(
            "SELECT snapshot_id,facts_sha256 FROM metadata WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if id != copy.snapshot_id || contract::snapshot_id(&connection)? != id {
            return Err(Error::Identity);
        }
        if connection
            .prepare("PRAGMA foreign_key_check")?
            .query([])?
            .next()?
            .is_some()
        {
            return Err(Error::ForeignKey);
        }
        let actual = contract::facts_sha256(&connection)?;
        if facts.as_deref() != Some(actual.as_str()) {
            return Err(Error::Facts);
        }
        Ok(Self {
            connection,
            _copy: copy,
        })
    }

    pub(crate) fn sppr_exclusion(&self, path: &str) -> Result<Option<(String, u64)>> {
        let raw = snapshot_v5::navigator_command_help_writer::read_manifest(&self.connection)?;
        let list = raw["excluded"].as_array().ok_or(Error::Range)?;
        for entry in list {
            if entry["input"]["path"] == path {
                if let (Some(reason), Some(size)) =
                    (entry["reason"].as_str(), entry["size_bytes"].as_u64())
                {
                    return Ok(Some((reason.to_owned(), size)));
                }
            }
        }
        Ok(None)
    }
    pub fn source(&self, path: &str, start: i64, end: i64, max_bytes: u64) -> Result<Vec<u8>> {
        self.source_span(path, start, end, max_bytes)
            .map(|(bytes, _, _)| bytes)
            .map_err(|e| match e {
                Error::Limit => Error::Range,
                other => other,
            })
    }

    pub(crate) fn source_span(
        &self,
        path: &str,
        start: i64,
        end: i64,
        max_bytes: u64,
    ) -> Result<(Vec<u8>, i64, i64)> {
        if start < 1 || end < start {
            return Err(Error::Range);
        }
        let (rowid, first, last): (i64, i64, i64) = self.connection.query_row(
            include_str!("../spikes/linux-v5/range.sql"),
            rusqlite::named_params! {":path":path,":start_line":start,":end_line":end},
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if first < 0 || last < first {
            return Err(Error::Range);
        }
        let count = u64::try_from(last - first).map_err(|_| Error::Range)?;
        if count > max_bytes {
            return Err(Error::Limit);
        }
        let mut blob =
            self.connection
                .blob_open(DatabaseName::Main, "sources", "content", rowid, true)?;
        if last > blob.len() as i64 {
            return Err(Error::Range);
        }
        blob.seek(SeekFrom::Start(first as u64))?;
        let mut bytes = vec![0; usize::try_from(count).map_err(|_| Error::Range)?];
        blob.read_exact(&mut bytes)?;
        Ok((bytes, first, last))
    }

    /// One SQL branch, including parse status and rejection reason; no BSL parsing.
    pub fn tree(
        &self,
        level: &str,
        parent_id: Option<i64>,
        kind: Option<&str>,
        module_path: Option<&str>,
        limits: TreeLimits,
    ) -> Result<Vec<TreeNode>> {
        read_tree(
            &self.connection,
            level,
            parent_id,
            kind,
            module_path,
            limits,
        )
    }

    /// Exact UTF-8/BINARY name filter applied in SQL before branch budgets.
    pub fn tree_named(
        &self,
        level: &str,
        parent_id: Option<i64>,
        kind: Option<&str>,
        module_path: Option<&str>,
        name_exact: Option<&str>,
        limits: TreeLimits,
    ) -> Result<Vec<TreeNode>> {
        read_tree_named(
            &self.connection,
            level,
            parent_id,
            kind,
            module_path,
            name_exact,
            limits,
        )
    }

    // Small internal read-only additions required by MCP: distinguish an absent branch
    // from an accepted module with zero methods, and return stored source coordinates.
    #[cfg(feature = "experimental-snapshot-mcp")]
    pub(crate) fn branch_info(
        &self,
        level: &str,
        parent: Option<i64>,
        kind: Option<&str>,
        path: Option<&str>,
    ) -> Result<(Option<i64>, serde_json::Value)> {
        use serde_json::json;
        let missing = || Error::Sqlite(rusqlite::Error::QueryReturnedNoRows);
        match level {
            "configuration" | "object" => {
                let id = if let Some(id) = parent {
                    id
                } else if level == "configuration" {
                    self.connection.query_row(
                        "SELECT object_id FROM objects WHERE kind='Configuration'",
                        [],
                        |r| r.get(0),
                    )?
                } else {
                    return Err(missing());
                };
                let n:i64=self.connection.query_row("SELECT length(CAST(name AS BLOB)) FROM objects WHERE object_id=?1 AND ((?2='configuration' AND kind='Configuration') OR (?2='object' AND kind<>'Configuration'))",rusqlite::params![id,level],|r|r.get(0))?;
                if n > 1024 {
                    return Err(Error::Limit);
                }
                let name: String = self.connection.query_row(
                    "SELECT name FROM objects WHERE object_id=?1",
                    [id],
                    |r| r.get(0),
                )?;
                let mut branch = json!({"status":"ok","level":level,"parent_id":id,"name":name});
                let profile: String =
                    self.connection
                        .query_row("SELECT profile FROM metadata", [], |r| r.get(0))?;
                if level == "configuration"
                    && [
                        snapshot_v5::corpus_writer::PROFILE,
                        snapshot_v5::corpus_bin_writer::PROFILE,
                        snapshot_v5::corpus_help_writer::PROFILE,
                        snapshot_v5::corpus_config_writer::PROFILE,
                        snapshot_v5::diagnostics_writer::PROFILE,
                        snapshot_v5::optional_form_writer::PROFILE,
                        snapshot_v5::sidecar_writer::PROFILE,
                        snapshot_v5::context_writer::PROFILE,
                        snapshot_v5::parameter_writer::PROFILE,
                        snapshot_v5::conditional_writer::PROFILE,
                        snapshot_v5::string_writer::PROFILE,
                        snapshot_v5::literal_writer::PROFILE,
                        snapshot_v5::wrapped_writer::PROFILE,
                        snapshot_v5::refined_writer::PROFILE,
                        snapshot_v5::annotated_vars_writer::PROFILE,
                        snapshot_v5::dates_members_writer::PROFILE,
                        snapshot_v5::async_casing_writer::PROFILE,
                        snapshot_v5::mobile_vars_writer::PROFILE,
                        snapshot_v5::method_regions_writer::PROFILE,
                        snapshot_v5::header_forms_writer::PROFILE,
                        snapshot_v5::source_limits_writer::PROFILE,
                        snapshot_v5::long_headers_writer::PROFILE,
                        snapshot_v5::lexical_tail_writer::PROFILE,
                        snapshot_v5::source_anomalies_writer::PROFILE,
                        snapshot_v5::navigator_agreement_writer::PROFILE,
                        snapshot_v5::navigator_owners_writer::PROFILE,
                        snapshot_v5::navigator_sppr::PROFILE,
                    ]
                    .contains(&profile.as_str())
                {
                    let mut presence = json!({});
                    for module in [
                        "ManagedApplicationModule",
                        "OrdinaryApplicationModule",
                        "SessionModule",
                        "ExternalConnectionModule",
                    ] {
                        let path = format!("Ext/{module}.bsl");
                        let status:String=self.connection.query_row("SELECT coalesce((SELECT CASE parse_status WHEN 'accepted' THEN 'accepted' WHEN 'unparsed' THEN 'unparsed' ELSE 'unknown' END FROM modules WHERE path=?1 AND owner_id=?2),'absent')",rusqlite::params![path,id],|r|r.get(0))?;
                        presence[module] = json!(status);
                    }
                    branch["configuration_modules"] = presence;
                }
                if [
                    snapshot_v5::root_writer::PROFILE,
                    snapshot_v5::shared_writer::PROFILE,
                    snapshot_v5::leaf_writer::PROFILE,
                    snapshot_v5::journal_writer::PROFILE,
                    snapshot_v5::service_writer::PROFILE,
                    snapshot_v5::chart_writer::PROFILE,
                    snapshot_v5::register_writer::PROFILE,
                    snapshot_v5::report_writer::PROFILE,
                    snapshot_v5::workflow_writer::PROFILE,
                    snapshot_v5::corpus_writer::PROFILE,
                    snapshot_v5::corpus_bin_writer::PROFILE,
                    snapshot_v5::corpus_help_writer::PROFILE,
                    snapshot_v5::corpus_config_writer::PROFILE,
                    snapshot_v5::diagnostics_writer::PROFILE,
                    snapshot_v5::optional_form_writer::PROFILE,
                    snapshot_v5::sidecar_writer::PROFILE,
                    snapshot_v5::context_writer::PROFILE,
                    snapshot_v5::parameter_writer::PROFILE,
                    snapshot_v5::conditional_writer::PROFILE,
                    snapshot_v5::string_writer::PROFILE,
                    snapshot_v5::literal_writer::PROFILE,
                    snapshot_v5::wrapped_writer::PROFILE,
                    snapshot_v5::refined_writer::PROFILE,
                    snapshot_v5::annotated_vars_writer::PROFILE,
                    snapshot_v5::dates_members_writer::PROFILE,
                    snapshot_v5::async_casing_writer::PROFILE,
                    snapshot_v5::mobile_vars_writer::PROFILE,
                    snapshot_v5::method_regions_writer::PROFILE,
                    snapshot_v5::header_forms_writer::PROFILE,
                    snapshot_v5::source_limits_writer::PROFILE,
                    snapshot_v5::long_headers_writer::PROFILE,
                    snapshot_v5::lexical_tail_writer::PROFILE,
                    snapshot_v5::source_anomalies_writer::PROFILE,
                    snapshot_v5::navigator_agreement_writer::PROFILE,
                    snapshot_v5::navigator_owners_writer::PROFILE,
                    snapshot_v5::navigator_sppr::PROFILE,
                ]
                .contains(&profile.as_str())
                    && level == "object"
                {
                    let (a,b):(u64,u64)=self.connection.query_row("SELECT length(CAST(xml_path AS BLOB)),length(CAST(kind AS BLOB)) FROM objects WHERE object_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?)))?;
                    if a > 1024 || b > 128 {
                        return Err(Error::Limit);
                    }
                    let (path,kind,len):(String,String,u64)=self.connection.query_row("SELECT o.xml_path,o.kind,length(s.content) FROM objects o JOIN sources s ON o.xml_path=s.path WHERE o.object_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
                    if [snapshot_v5::optional_form_writer::PROFILE,snapshot_v5::sidecar_writer::PROFILE,snapshot_v5::context_writer::PROFILE,snapshot_v5::parameter_writer::PROFILE,snapshot_v5::conditional_writer::PROFILE,snapshot_v5::string_writer::PROFILE,snapshot_v5::literal_writer::PROFILE,snapshot_v5::wrapped_writer::PROFILE,snapshot_v5::refined_writer::PROFILE,snapshot_v5::annotated_vars_writer::PROFILE,snapshot_v5::dates_members_writer::PROFILE,snapshot_v5::async_casing_writer::PROFILE,snapshot_v5::mobile_vars_writer::PROFILE,snapshot_v5::method_regions_writer::PROFILE,snapshot_v5::header_forms_writer::PROFILE,snapshot_v5::source_limits_writer::PROFILE,snapshot_v5::long_headers_writer::PROFILE,snapshot_v5::lexical_tail_writer::PROFILE,snapshot_v5::source_anomalies_writer::PROFILE,snapshot_v5::navigator_agreement_writer::PROFILE,snapshot_v5::navigator_owners_writer::PROFILE,snapshot_v5::navigator_sppr::PROFILE].contains(&profile.as_str()) && kind=="Form" && self.connection.query_row("SELECT p.kind IN ('AccumulationRegister','ChartOfCharacteristicTypes') FROM objects o JOIN objects p ON p.object_id=o.parent_id WHERE o.object_id=?1",[id],|r|r.get::<_,bool>(0))? {
                        let module=format!("{}/Ext/Form/Module.bsl",path.strip_suffix(".xml").ok_or(Error::Range)?);
                        let presence:String=self.connection.query_row("SELECT coalesce((SELECT parse_status FROM modules WHERE path=?1 AND owner_id=?2),'absent')",rusqlite::params![module,id],|r|r.get(0))?;
                        branch["form_module_presence"]=json!(match presence.as_str(){"accepted"=>"accepted","unparsed"=>"unparsed","absent"=>"absent",_=>"unknown"});
                    }
                    branch["descriptor_only"] = json!(crate::xml221::root_locator(&kind)
                        .is_some_and(|r| matches!(r.kind, crate::xml221::Kind::Descriptor(_))));
                    branch["source"] = json!({"saved_path":path,"start_byte":0,"end_byte":len});
                    if [
                        snapshot_v5::journal_writer::PROFILE,
                        snapshot_v5::service_writer::PROFILE,
                        snapshot_v5::chart_writer::PROFILE,
                        snapshot_v5::register_writer::PROFILE,
                        snapshot_v5::report_writer::PROFILE,
                        snapshot_v5::workflow_writer::PROFILE,
                        snapshot_v5::corpus_writer::PROFILE,
                        snapshot_v5::corpus_bin_writer::PROFILE,
                        snapshot_v5::corpus_help_writer::PROFILE,
                        snapshot_v5::corpus_config_writer::PROFILE,
                        snapshot_v5::diagnostics_writer::PROFILE,
                        snapshot_v5::optional_form_writer::PROFILE,
                        snapshot_v5::sidecar_writer::PROFILE,
                        snapshot_v5::context_writer::PROFILE,
                        snapshot_v5::parameter_writer::PROFILE,
                        snapshot_v5::conditional_writer::PROFILE,
                        snapshot_v5::string_writer::PROFILE,
                        snapshot_v5::literal_writer::PROFILE,
                        snapshot_v5::wrapped_writer::PROFILE,
                        snapshot_v5::refined_writer::PROFILE,
                        snapshot_v5::annotated_vars_writer::PROFILE,
                        snapshot_v5::dates_members_writer::PROFILE,
                        snapshot_v5::async_casing_writer::PROFILE,
                        snapshot_v5::mobile_vars_writer::PROFILE,
                        snapshot_v5::method_regions_writer::PROFILE,
                        snapshot_v5::header_forms_writer::PROFILE,
                        snapshot_v5::source_limits_writer::PROFILE,
                        snapshot_v5::long_headers_writer::PROFILE,
                        snapshot_v5::lexical_tail_writer::PROFILE,
                        snapshot_v5::source_anomalies_writer::PROFILE,
                        snapshot_v5::navigator_agreement_writer::PROFILE,
                        snapshot_v5::navigator_owners_writer::PROFILE,
                        snapshot_v5::navigator_sppr::PROFILE,
                    ]
                    .contains(&profile.as_str())
                        && ["Enum", "DocumentJournal"].contains(&kind.as_str())
                    {
                        let module = format!(
                            "{}/Ext/ManagerModule.bsl",
                            path.strip_suffix(".xml").ok_or(Error::Range)?
                        );
                        let status:String=self.connection.query_row("SELECT coalesce((SELECT CASE parse_status WHEN 'accepted' THEN 'accepted' WHEN 'unparsed' THEN 'unparsed' ELSE 'unknown' END FROM modules WHERE path=?1 AND owner_id=?2),'absent')",rusqlite::params![module,id],|r|r.get(0))?;
                        branch["manager_presence"] = json!(status);
                    }

                    if [
                        snapshot_v5::leaf_writer::PROFILE,
                        snapshot_v5::journal_writer::PROFILE,
                        snapshot_v5::service_writer::PROFILE,
                        snapshot_v5::chart_writer::PROFILE,
                        snapshot_v5::register_writer::PROFILE,
                        snapshot_v5::report_writer::PROFILE,
                        snapshot_v5::workflow_writer::PROFILE,
                        snapshot_v5::corpus_writer::PROFILE,
                        snapshot_v5::corpus_bin_writer::PROFILE,
                        snapshot_v5::corpus_help_writer::PROFILE,
                        snapshot_v5::corpus_config_writer::PROFILE,
                        snapshot_v5::diagnostics_writer::PROFILE,
                        snapshot_v5::optional_form_writer::PROFILE,
                        snapshot_v5::sidecar_writer::PROFILE,
                        snapshot_v5::context_writer::PROFILE,
                        snapshot_v5::parameter_writer::PROFILE,
                        snapshot_v5::conditional_writer::PROFILE,
                        snapshot_v5::string_writer::PROFILE,
                        snapshot_v5::literal_writer::PROFILE,
                        snapshot_v5::wrapped_writer::PROFILE,
                        snapshot_v5::refined_writer::PROFILE,
                        snapshot_v5::annotated_vars_writer::PROFILE,
                        snapshot_v5::dates_members_writer::PROFILE,
                        snapshot_v5::async_casing_writer::PROFILE,
                        snapshot_v5::mobile_vars_writer::PROFILE,
                        snapshot_v5::method_regions_writer::PROFILE,
                        snapshot_v5::header_forms_writer::PROFILE,
                        snapshot_v5::source_limits_writer::PROFILE,
                        snapshot_v5::long_headers_writer::PROFILE,
                        snapshot_v5::lexical_tail_writer::PROFILE,
                        snapshot_v5::source_anomalies_writer::PROFILE,
                        snapshot_v5::navigator_agreement_writer::PROFILE,
                        snapshot_v5::navigator_owners_writer::PROFILE,
                        snapshot_v5::navigator_sppr::PROFILE,
                    ]
                    .contains(&profile.as_str())
                    {
                        if kind == "Constant" {
                            let mut presence = json!({});
                            for module in ["ManagerModule", "ValueManagerModule"] {
                                let source = format!(
                                    "{}/Ext/{module}.bsl",
                                    path.strip_suffix(".xml").ok_or(Error::Range)?
                                );
                                // Fixed status vocabulary; never copy unbounded stored strings.
                                let status: String = self.connection.query_row("SELECT coalesce((SELECT CASE parse_status WHEN 'accepted' THEN 'accepted' WHEN 'unparsed' THEN 'unparsed' ELSE 'unknown' END FROM modules WHERE path=?1 AND owner_id=?2),'absent')", rusqlite::params![source,id], |r|r.get(0))?;
                                presence[module] = json!(status);
                            }
                            branch["module_presence"] = presence;
                        }
                        if kind == "ScheduledJob" {
                            let present: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM companion_payloads WHERE object_id=?1)",[id],|r|r.get(0))?;
                            branch["schedule_presence"] = json!(if present {
                                "present_unparsed"
                            } else {
                                "absent"
                            });
                        }
                    }
                }
                Ok((Some(id), branch))
            }
            "kind" => {
                let id = parent.ok_or_else(missing)?;
                let found: bool = self.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM objects WHERE parent_id=?1 AND kind=?2)",
                    rusqlite::params![id, kind],
                    |r| r.get(0),
                )?;
                if !found {
                    return Err(missing());
                }
                Ok((
                    Some(id),
                    json!({"status":"ok","level":level,"parent_id":id,"kind":kind}),
                ))
            }
            "unconfirmed" => {
                let found: bool = self.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM modules WHERE owner_id IS NULL)",
                    [],
                    |r| r.get(0),
                )?;
                if !found {
                    return Err(missing());
                }
                Ok((
                    None,
                    json!({"status":"ok","level":level,"ownership":"unconfirmed"}),
                ))
            }
            "module" => {
                let path = path.ok_or_else(missing)?;
                let size:i64=self.connection.query_row("SELECT length(CAST(parse_status AS BLOB))+coalesce(length(CAST(reason_code AS BLOB)),0) FROM modules WHERE path=?1",[path],|r|r.get(0))?;
                if size > 128 {
                    return Err(Error::Limit);
                }
                let (status, reason): (String, Option<String>) = self.connection.query_row(
                    "SELECT parse_status,reason_code FROM modules WHERE path=?1",
                    [path],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                if !["accepted", "unparsed"].contains(&status.as_str()) {
                    return Err(Error::Schema);
                }
                let mut branch = json!({"status":status,"reason":reason,"level":level,"path":path});
                if self.journal_profile()? {
                    let size:u64=self.connection.query_row("SELECT coalesce((SELECT length(CAST(declaration_key AS BLOB)) FROM inline_module_owners WHERE module_path=?1),0)",[path],|r|r.get(0))?;
                    if size > 1100 {
                        return Err(Error::Limit);
                    }
                    if size > 0 {
                        let (key,owner):(String,i64)=self.connection.query_row("SELECT declaration_key,owner_id FROM inline_module_owners WHERE module_path=?1",[path],|r|Ok((r.get(0)?,r.get(1)?)))?;
                        branch["owner_declaration"] = json!(key);
                        branch["container_object_id"] = json!(owner);
                    }
                }
                Ok((None, branch))
            }
            _ => Err(Error::Range),
        }
    }

    #[cfg(feature = "experimental-snapshot-mcp")]
    pub(crate) fn source_anomaly(&self, path: &str) -> Result<Option<serde_json::Value>> {
        use rusqlite::OptionalExtension;
        let row: Option<(String,u64,u64,u64)> = self.connection.query_row(
            "SELECT code,physical_line,byte_start,byte_end FROM bsl_source_anomalies WHERE module_path=?1",
            [path], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
        ).optional()?;
        Ok(row.map(|(code, line, start, end)| {
            serde_json::json!({
                "code":code,"physical_line":line,"start_byte":start,"end_byte":end,
                "line_units":"physical_1_based_inclusive","byte_units":"0_based_end_exclusive"
            })
        }))
    }

    #[cfg(feature = "experimental-snapshot-mcp")]
    pub(crate) fn method_location(&self, id: i64) -> Result<serde_json::Value> {
        let (start,end,a,b,exported,kind):(i64,i64,i64,i64,bool,String)=self.connection.query_row(
            "SELECT start_line,end_line,byte_start,byte_end,exported,CASE kind WHEN 'procedure' THEN 'procedure' WHEN 'function' THEN 'function' ELSE 'unknown' END FROM methods WHERE method_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;
        let mut location = serde_json::json!({"kind":kind,"start_line":start,"end_line":end,"start_byte":a,"end_byte":b,"exported":exported,"line_units":"physical_1_based_inclusive","byte_units":"0_based_end_exclusive"});
        let profile: String =
            self.connection
                .query_row("SELECT profile FROM metadata", [], |r| r.get(0))?;
        if [
            snapshot_v5::conditional_writer::PROFILE,
            snapshot_v5::string_writer::PROFILE,
            snapshot_v5::literal_writer::PROFILE,
            snapshot_v5::wrapped_writer::PROFILE,
            snapshot_v5::refined_writer::PROFILE,
            snapshot_v5::annotated_vars_writer::PROFILE,
            snapshot_v5::dates_members_writer::PROFILE,
            snapshot_v5::async_casing_writer::PROFILE,
            snapshot_v5::mobile_vars_writer::PROFILE,
            snapshot_v5::method_regions_writer::PROFILE,
            snapshot_v5::header_forms_writer::PROFILE,
            snapshot_v5::source_limits_writer::PROFILE,
            snapshot_v5::long_headers_writer::PROFILE,
            snapshot_v5::lexical_tail_writer::PROFILE,
            snapshot_v5::source_anomalies_writer::PROFILE,
            snapshot_v5::navigator_agreement_writer::PROFILE,
            snapshot_v5::navigator_owners_writer::PROFILE,
            snapshot_v5::navigator_sppr::PROFILE,
        ]
        .contains(&profile.as_str())
        {
            let n: u64 = self.connection.query_row(
                "SELECT length(CAST(branch_path AS BLOB)) FROM method_branches WHERE method_id=?1",
                [id],
                |r| r.get(0),
            )?;
            if n > 4096 {
                return Err(Error::Limit);
            }
            let path: String = self.connection.query_row(
                "SELECT branch_path FROM method_branches WHERE method_id=?1",
                [id],
                |r| r.get(0),
            )?;
            location["declaration_branch_path"] =
                serde_json::from_str(&path).map_err(|_| Error::Schema)?;
            location["conditional_evaluation"] = serde_json::json!("not_evaluated");
        }
        Ok(location)
    }

    pub fn companion_branch(&self, owner: i64, limits: TreeLimits) -> Result<serde_json::Value> {
        let p: String = self
            .connection
            .query_row("SELECT profile FROM metadata", [], |r| r.get(0))?;
        if ![
            snapshot_v5::companion_writer::PROFILE,
            snapshot_v5::common_writer::PROFILE,
            snapshot_v5::root_writer::PROFILE,
            snapshot_v5::shared_writer::PROFILE,
            snapshot_v5::leaf_writer::PROFILE,
            snapshot_v5::journal_writer::PROFILE,
            snapshot_v5::service_writer::PROFILE,
            snapshot_v5::chart_writer::PROFILE,
            snapshot_v5::register_writer::PROFILE,
            snapshot_v5::report_writer::PROFILE,
            snapshot_v5::workflow_writer::PROFILE,
            snapshot_v5::corpus_writer::PROFILE,
            snapshot_v5::corpus_bin_writer::PROFILE,
            snapshot_v5::corpus_help_writer::PROFILE,
            snapshot_v5::corpus_config_writer::PROFILE,
            snapshot_v5::diagnostics_writer::PROFILE,
            snapshot_v5::optional_form_writer::PROFILE,
            snapshot_v5::sidecar_writer::PROFILE,
            snapshot_v5::context_writer::PROFILE,
            snapshot_v5::parameter_writer::PROFILE,
            snapshot_v5::conditional_writer::PROFILE,
            snapshot_v5::string_writer::PROFILE,
            snapshot_v5::literal_writer::PROFILE,
            snapshot_v5::wrapped_writer::PROFILE,
            snapshot_v5::refined_writer::PROFILE,
            snapshot_v5::annotated_vars_writer::PROFILE,
            snapshot_v5::dates_members_writer::PROFILE,
            snapshot_v5::async_casing_writer::PROFILE,
            snapshot_v5::mobile_vars_writer::PROFILE,
            snapshot_v5::method_regions_writer::PROFILE,
            snapshot_v5::header_forms_writer::PROFILE,
            snapshot_v5::source_limits_writer::PROFILE,
            snapshot_v5::long_headers_writer::PROFILE,
            snapshot_v5::lexical_tail_writer::PROFILE,
            snapshot_v5::source_anomalies_writer::PROFILE,
            snapshot_v5::navigator_agreement_writer::PROFILE,
            snapshot_v5::navigator_owners_writer::PROFILE,
            snapshot_v5::navigator_sppr::PROFILE,
        ]
        .contains(&p.as_str())
        {
            return Err(Error::Profile);
        }
        self.connection.query_row(
            "SELECT object_id FROM objects WHERE object_id=?1",
            [owner],
            |_| Ok(()),
        )?;
        let leaf: bool = [snapshot_v5::leaf_writer::PROFILE,snapshot_v5::journal_writer::PROFILE,snapshot_v5::chart_writer::PROFILE,snapshot_v5::register_writer::PROFILE,snapshot_v5::report_writer::PROFILE,snapshot_v5::workflow_writer::PROFILE,snapshot_v5::corpus_writer::PROFILE,snapshot_v5::corpus_bin_writer::PROFILE,snapshot_v5::corpus_help_writer::PROFILE,snapshot_v5::corpus_config_writer::PROFILE,snapshot_v5::diagnostics_writer::PROFILE,snapshot_v5::optional_form_writer::PROFILE,snapshot_v5::sidecar_writer::PROFILE,snapshot_v5::context_writer::PROFILE,snapshot_v5::parameter_writer::PROFILE,snapshot_v5::conditional_writer::PROFILE,snapshot_v5::string_writer::PROFILE,snapshot_v5::literal_writer::PROFILE,snapshot_v5::wrapped_writer::PROFILE,snapshot_v5::refined_writer::PROFILE,snapshot_v5::annotated_vars_writer::PROFILE,snapshot_v5::dates_members_writer::PROFILE,snapshot_v5::async_casing_writer::PROFILE,snapshot_v5::mobile_vars_writer::PROFILE,snapshot_v5::method_regions_writer::PROFILE,snapshot_v5::header_forms_writer::PROFILE,snapshot_v5::source_limits_writer::PROFILE,snapshot_v5::long_headers_writer::PROFILE,snapshot_v5::lexical_tail_writer::PROFILE,snapshot_v5::source_anomalies_writer::PROFILE,snapshot_v5::navigator_agreement_writer::PROFILE,snapshot_v5::navigator_owners_writer::PROFILE,snapshot_v5::navigator_sppr::PROFILE].contains(&p.as_str()) && self.connection.query_row("SELECT kind IN ('CommonPicture','Constant','Role','ScheduledJob','XDTOPackage','DocumentJournal','ChartOfAccounts','ChartOfCalculationTypes','ChartOfCharacteristicTypes','AccountingRegister','AccumulationRegister','CalculationRegister','DataProcessor','Report','ExchangePlan','BusinessProcess','Task') OR (?2 AND kind IN ('Configuration','Subsystem')) FROM objects WHERE object_id=?1",rusqlite::params![owner,[snapshot_v5::corpus_writer::PROFILE,snapshot_v5::corpus_bin_writer::PROFILE,snapshot_v5::corpus_help_writer::PROFILE,snapshot_v5::corpus_config_writer::PROFILE,snapshot_v5::diagnostics_writer::PROFILE,snapshot_v5::optional_form_writer::PROFILE,snapshot_v5::sidecar_writer::PROFILE,snapshot_v5::context_writer::PROFILE,snapshot_v5::parameter_writer::PROFILE,snapshot_v5::conditional_writer::PROFILE,snapshot_v5::string_writer::PROFILE,snapshot_v5::literal_writer::PROFILE,snapshot_v5::wrapped_writer::PROFILE,snapshot_v5::refined_writer::PROFILE,snapshot_v5::annotated_vars_writer::PROFILE,snapshot_v5::dates_members_writer::PROFILE,snapshot_v5::async_casing_writer::PROFILE,snapshot_v5::mobile_vars_writer::PROFILE,snapshot_v5::method_regions_writer::PROFILE,snapshot_v5::header_forms_writer::PROFILE,snapshot_v5::source_limits_writer::PROFILE,snapshot_v5::long_headers_writer::PROFILE,snapshot_v5::lexical_tail_writer::PROFILE,snapshot_v5::source_anomalies_writer::PROFILE,snapshot_v5::navigator_agreement_writer::PROFILE,snapshot_v5::navigator_owners_writer::PROFILE,snapshot_v5::navigator_sppr::PROFILE].contains(&p.as_str())],|r|r.get(0))?;
        let leaf=leaf || ([snapshot_v5::sidecar_writer::PROFILE,snapshot_v5::context_writer::PROFILE,snapshot_v5::parameter_writer::PROFILE,snapshot_v5::conditional_writer::PROFILE,snapshot_v5::string_writer::PROFILE,snapshot_v5::literal_writer::PROFILE,snapshot_v5::wrapped_writer::PROFILE,snapshot_v5::refined_writer::PROFILE,snapshot_v5::annotated_vars_writer::PROFILE,snapshot_v5::dates_members_writer::PROFILE,snapshot_v5::async_casing_writer::PROFILE,snapshot_v5::mobile_vars_writer::PROFILE,snapshot_v5::method_regions_writer::PROFILE,snapshot_v5::header_forms_writer::PROFILE,snapshot_v5::source_limits_writer::PROFILE,snapshot_v5::long_headers_writer::PROFILE,snapshot_v5::lexical_tail_writer::PROFILE,snapshot_v5::source_anomalies_writer::PROFILE,snapshot_v5::navigator_agreement_writer::PROFILE,snapshot_v5::navigator_owners_writer::PROFILE,snapshot_v5::navigator_sppr::PROFILE].contains(&p.as_str()) && self.connection.query_row("SELECT kind IN ('Catalog','Document','InformationRegister') FROM objects WHERE object_id=?1",[owner],|r|r.get::<_,bool>(0))?);
        // All rows preflighted before text/JSON allocation; immutable connection.
        let sql="SELECT 'descriptor' AS role,o.xml_path AS path,o.name AS name,o.object_id AS id,0 AS a,length(s.content) AS b FROM objects o JOIN sources s ON s.path=o.xml_path WHERE o.parent_id=?1 AND (o.kind IN ('Form','Template') OR (o.kind='Subsystem' AND (SELECT profile FROM metadata) IN ('linux-v5-corpus1-xml221-r09','linux-v5-corpus-bin1-xml221-r09','linux-v5-corpus-help1-xml221-r09','linux-v5-corpus-config1-xml221-r09','linux-v5-diagnostics1-xml221-r09','linux-v5-optional-form1-xml221-r09','linux-v5-root-sidecar1-xml221-r09','linux-v5-context1-xml221-r09','linux-v5-parameters1-xml221-r09','linux-v5-conditionals1-xml221-r09','linux-v5-strings1-xml221-r09','linux-v5-literals1-xml221-r09','linux-v5-wrapped1-xml221-r09','linux-v5-refinements1-xml221-r09','linux-v5-annotated-vars1-xml221-r09','linux-v5-dates-members1-xml221-r09','linux-v5-async-casing1-xml221-r09','linux-v5-mobile-vars1-xml221-r09','linux-v5-method-regions1-xml221-r09','linux-v5-header-forms1-xml221-r09','linux-v5-source-limits1-xml221-r09')) OR (o.kind='Recalculation' AND (SELECT profile FROM metadata) IN ('linux-v5-register1-xml221-r09','linux-v5-report1-xml221-r09','linux-v5-workflow1-xml221-r09','linux-v5-corpus1-xml221-r09','linux-v5-corpus-bin1-xml221-r09','linux-v5-corpus-help1-xml221-r09','linux-v5-corpus-config1-xml221-r09','linux-v5-diagnostics1-xml221-r09','linux-v5-optional-form1-xml221-r09','linux-v5-root-sidecar1-xml221-r09','linux-v5-context1-xml221-r09','linux-v5-parameters1-xml221-r09','linux-v5-conditionals1-xml221-r09','linux-v5-strings1-xml221-r09','linux-v5-literals1-xml221-r09','linux-v5-wrapped1-xml221-r09','linux-v5-refinements1-xml221-r09','linux-v5-annotated-vars1-xml221-r09','linux-v5-dates-members1-xml221-r09','linux-v5-async-casing1-xml221-r09','linux-v5-mobile-vars1-xml221-r09','linux-v5-method-regions1-xml221-r09','linux-v5-header-forms1-xml221-r09','linux-v5-source-limits1-xml221-r09'))) UNION ALL SELECT p.role,p.path,p.path,p.object_id,0,length(s.content) FROM companion_payloads p JOIN sources s ON s.path=p.path WHERE p.object_id=?1 UNION ALL SELECT 'reference',c.reference_path,c.reference_path,c.object_id,c.byte_start,c.byte_end FROM companions c WHERE c.object_id=?1";
        let mut q=self.connection.prepare(&format!("SELECT length(CAST(role AS BLOB))+length(CAST(path AS BLOB))+length(CAST(name AS BLOB))+256 FROM ({sql}) LIMIT ?2"))?;
        let mut rows = q.query(rusqlite::params![
            owner,
            i64::try_from(limits.rows)
                .ok()
                .and_then(|n| n.checked_add(1))
                .ok_or(Error::Limit)?
        ])?;
        let mut count = 0;
        let mut bytes = 0usize;
        while let Some(r) = rows.next()? {
            count += 1;
            bytes = bytes
                .checked_add(r.get::<_, usize>(0)?)
                .ok_or(Error::Limit)?;
            if count > limits.rows || bytes > limits.bytes {
                return Err(Error::Limit);
            }
        }
        let mut q = self.connection.prepare(&format!(
            "SELECT * FROM ({sql}) ORDER BY role COLLATE BINARY,path COLLATE BINARY"
        ))?;
        let rows=q.query_map([owner],|r| {let role:String=r.get(0)?;let path:String=r.get(1)?;let id:i64=r.get(3)?; Ok(serde_json::json!({"type":"companion","role":role,"name":r.get::<_,String>(2)?,"object_id":id,"path":path,"content":"not_analyzed","source":{"saved_path":path,"start_byte":r.get::<_,i64>(4)?,"end_byte":r.get::<_,i64>(5)?},"children":if role=="descriptor" {serde_json::json!({"level":"object","parent_id":id})}else{serde_json::Value::Null}}))})?;
        Ok(
            serde_json::json!({"items":rows.collect::<std::result::Result<Vec<_>,_>>()?,"complete_branch":true,"coverage":"structure_only","contents":"not_analyzed","payload_presence":if self.connection.query_row("SELECT EXISTS(SELECT 1 FROM companion_payloads WHERE object_id=?1 AND (?2 OR role<>'additional'))",rusqlite::params![owner,leaf],|r|r.get::<_,bool>(0))? {"present_unparsed"}else if self.connection.query_row("SELECT EXISTS(SELECT 1 FROM companions WHERE object_id=?1)",[owner],|r|r.get::<_,bool>(0))? {"absent"}else{"not_applicable"}}),
        )
    }
    pub fn saved_range(
        &self,
        path: &str,
        start: u64,
        end: u64,
        max: usize,
    ) -> Result<(Vec<u8>, String)> {
        let p: String = self
            .connection
            .query_row("SELECT profile FROM metadata", [], |r| r.get(0))?;
        if ![
            snapshot_v5::companion_writer::PROFILE,
            snapshot_v5::common_writer::PROFILE,
            snapshot_v5::root_writer::PROFILE,
            snapshot_v5::shared_writer::PROFILE,
            snapshot_v5::leaf_writer::PROFILE,
            snapshot_v5::journal_writer::PROFILE,
            snapshot_v5::service_writer::PROFILE,
            snapshot_v5::chart_writer::PROFILE,
            snapshot_v5::register_writer::PROFILE,
            snapshot_v5::report_writer::PROFILE,
            snapshot_v5::workflow_writer::PROFILE,
            snapshot_v5::corpus_writer::PROFILE,
            snapshot_v5::corpus_bin_writer::PROFILE,
            snapshot_v5::corpus_help_writer::PROFILE,
            snapshot_v5::corpus_config_writer::PROFILE,
            snapshot_v5::diagnostics_writer::PROFILE,
            snapshot_v5::optional_form_writer::PROFILE,
            snapshot_v5::sidecar_writer::PROFILE,
            snapshot_v5::context_writer::PROFILE,
            snapshot_v5::parameter_writer::PROFILE,
            snapshot_v5::conditional_writer::PROFILE,
            snapshot_v5::string_writer::PROFILE,
            snapshot_v5::literal_writer::PROFILE,
            snapshot_v5::wrapped_writer::PROFILE,
            snapshot_v5::refined_writer::PROFILE,
            snapshot_v5::annotated_vars_writer::PROFILE,
            snapshot_v5::dates_members_writer::PROFILE,
            snapshot_v5::async_casing_writer::PROFILE,
            snapshot_v5::mobile_vars_writer::PROFILE,
            snapshot_v5::method_regions_writer::PROFILE,
            snapshot_v5::header_forms_writer::PROFILE,
            snapshot_v5::source_limits_writer::PROFILE,
            snapshot_v5::long_headers_writer::PROFILE,
            snapshot_v5::lexical_tail_writer::PROFILE,
            snapshot_v5::source_anomalies_writer::PROFILE,
            snapshot_v5::navigator_agreement_writer::PROFILE,
            snapshot_v5::navigator_owners_writer::PROFILE,
            snapshot_v5::navigator_sppr::PROFILE,
        ]
        .contains(&p.as_str())
        {
            return Err(Error::Profile);
        }
        if end < start || end - start > max as u64 {
            return Err(Error::Limit);
        }
        let (rowid, len, kind): (i64, u64, String) = self.connection.query_row(
            "SELECT rowid,length(content),kind FROM sources WHERE path=?1",
            [path],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if end > len {
            return Err(Error::Range);
        }
        let mut blob =
            self.connection
                .blob_open(DatabaseName::Main, "sources", "content", rowid, true)?;
        use std::io::{Seek, SeekFrom};
        blob.seek(SeekFrom::Start(start))?;
        let mut bytes = vec![0; (end - start) as usize];
        blob.read_exact(&mut bytes)?;
        Ok((bytes, kind))
    }
    fn require_inline(&self) -> Result<()> {
        let p: String = self
            .connection
            .query_row("SELECT profile FROM metadata", [], |r| r.get(0))?;
        if ![
            snapshot_v5::inline_writer::PROFILE,
            snapshot_v5::companion_writer::PROFILE,
            snapshot_v5::common_writer::PROFILE,
            snapshot_v5::root_writer::PROFILE,
            snapshot_v5::shared_writer::PROFILE,
            snapshot_v5::leaf_writer::PROFILE,
            snapshot_v5::journal_writer::PROFILE,
            snapshot_v5::service_writer::PROFILE,
            snapshot_v5::chart_writer::PROFILE,
            snapshot_v5::register_writer::PROFILE,
            snapshot_v5::report_writer::PROFILE,
            snapshot_v5::workflow_writer::PROFILE,
            snapshot_v5::corpus_writer::PROFILE,
            snapshot_v5::corpus_bin_writer::PROFILE,
            snapshot_v5::corpus_help_writer::PROFILE,
            snapshot_v5::corpus_config_writer::PROFILE,
            snapshot_v5::diagnostics_writer::PROFILE,
            snapshot_v5::optional_form_writer::PROFILE,
            snapshot_v5::sidecar_writer::PROFILE,
            snapshot_v5::context_writer::PROFILE,
            snapshot_v5::parameter_writer::PROFILE,
            snapshot_v5::conditional_writer::PROFILE,
            snapshot_v5::string_writer::PROFILE,
            snapshot_v5::literal_writer::PROFILE,
            snapshot_v5::wrapped_writer::PROFILE,
            snapshot_v5::refined_writer::PROFILE,
            snapshot_v5::annotated_vars_writer::PROFILE,
            snapshot_v5::dates_members_writer::PROFILE,
            snapshot_v5::async_casing_writer::PROFILE,
            snapshot_v5::mobile_vars_writer::PROFILE,
            snapshot_v5::method_regions_writer::PROFILE,
            snapshot_v5::header_forms_writer::PROFILE,
            snapshot_v5::source_limits_writer::PROFILE,
            snapshot_v5::long_headers_writer::PROFILE,
            snapshot_v5::lexical_tail_writer::PROFILE,
            snapshot_v5::source_anomalies_writer::PROFILE,
            snapshot_v5::navigator_agreement_writer::PROFILE,
            snapshot_v5::navigator_owners_writer::PROFILE,
            snapshot_v5::navigator_sppr::PROFILE,
        ]
        .contains(&p.as_str())
        {
            return Err(Error::Profile);
        }
        Ok(())
    }
    fn journal_profile(&self) -> Result<bool> {
        let profile: String =
            self.connection
                .query_row("SELECT profile FROM metadata", [], |r| r.get(0))?;
        Ok(matches!(
            profile.as_str(),
            snapshot_v5::journal_writer::PROFILE
                | snapshot_v5::service_writer::PROFILE
                | snapshot_v5::chart_writer::PROFILE
                | snapshot_v5::register_writer::PROFILE
                | snapshot_v5::report_writer::PROFILE
                | snapshot_v5::workflow_writer::PROFILE
                | snapshot_v5::corpus_writer::PROFILE
                | snapshot_v5::corpus_bin_writer::PROFILE
                | snapshot_v5::corpus_help_writer::PROFILE
                | snapshot_v5::corpus_config_writer::PROFILE
                | snapshot_v5::diagnostics_writer::PROFILE
                | snapshot_v5::optional_form_writer::PROFILE
                | snapshot_v5::sidecar_writer::PROFILE
                | snapshot_v5::context_writer::PROFILE
                | snapshot_v5::parameter_writer::PROFILE
                | snapshot_v5::conditional_writer::PROFILE
                | snapshot_v5::string_writer::PROFILE
                | snapshot_v5::literal_writer::PROFILE
                | snapshot_v5::wrapped_writer::PROFILE
                | snapshot_v5::refined_writer::PROFILE
                | snapshot_v5::annotated_vars_writer::PROFILE
                | snapshot_v5::dates_members_writer::PROFILE
                | snapshot_v5::async_casing_writer::PROFILE
                | snapshot_v5::mobile_vars_writer::PROFILE
                | snapshot_v5::method_regions_writer::PROFILE
                | snapshot_v5::header_forms_writer::PROFILE
                | snapshot_v5::source_limits_writer::PROFILE
                | snapshot_v5::long_headers_writer::PROFILE
                | snapshot_v5::lexical_tail_writer::PROFILE
                | snapshot_v5::source_anomalies_writer::PROFILE
                | snapshot_v5::navigator_agreement_writer::PROFILE
                | snapshot_v5::navigator_owners_writer::PROFILE
                | snapshot_v5::navigator_sppr::PROFILE
        ))
    }
    pub fn declarations(
        &self,
        owner: i64,
        parent: Option<&str>,
        limits: TreeLimits,
    ) -> Result<serde_json::Value> {
        self.require_inline()?;
        self.connection.query_row(
            "SELECT object_id FROM objects WHERE object_id=?1",
            [owner],
            |r| r.get::<_, i64>(0),
        )?;
        if let Some(key) = parent {
            self.connection.query_row("SELECT declaration_key FROM inline_declarations WHERE declaration_key=?1 AND owner_id=?2",rusqlite::params![key,owner],|_|Ok(()))?;
        }
        let cap = i64::try_from(limits.rows)
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or(Error::Limit)?;
        let mut q=self.connection.prepare("SELECT length(CAST(declaration_key AS BLOB))+coalesce(length(CAST(parent_key AS BLOB)),0)+length(CAST(kind AS BLOB))+length(CAST(name AS BLOB))+length(CAST(xml_path AS BLOB))+128 FROM inline_declarations WHERE owner_id=?1 AND parent_key IS ?2 LIMIT ?3")?;
        let mut rows = q.query(rusqlite::params![owner, parent, cap])?;
        let mut count = 0;
        let mut bytes = 0usize;
        while let Some(r) = rows.next()? {
            count += 1;
            bytes = bytes
                .checked_add(r.get::<_, usize>(0)?)
                .ok_or(Error::Limit)?;
            if count > limits.rows || bytes > limits.bytes {
                return Err(Error::Limit);
            }
        }
        drop(rows);
        drop(q);
        let mut q=self.connection.prepare("SELECT declaration_key,parent_key,kind,name,xml_path,byte_start,byte_end FROM inline_declarations WHERE owner_id=?1 AND parent_key IS ?2 ORDER BY kind COLLATE BINARY,name COLLATE BINARY,declaration_key COLLATE BINARY")?;
        let mut items=q.query_map(rusqlite::params![owner,parent],|r| {
            let key:String=r.get(0)?;let kind:String=r.get(2)?;
            Ok(serde_json::json!({"type":"inline","key":key,"parent_key":r.get::<_,Option<String>>(1)?,"owner_id":owner,"kind":kind,"name":r.get::<_,String>(3)?,"path":r.get::<_,String>(4)?,"start_byte":r.get::<_,i64>(5)?,"end_byte":r.get::<_,i64>(6)?,"byte_units":"0_based_end_exclusive","semantics":"not_analyzed","source":{"declaration_key":key},"children":if ["TabularSection","URLTemplate","Operation"].contains(&kind.as_str()) {serde_json::json!({"level":"inline","parent_id":owner,"declaration_parent":key})}else{serde_json::Value::Null}}))
        })?.collect::<std::result::Result<Vec<_>,_>>()?;
        if self.journal_profile()? {
            // Extend the same trusted response budget before allocating module paths.
            for item in &mut items {
                let key = item["key"].as_str().ok_or(Error::Schema)?;
                let size:u64=self.connection.query_row("SELECT coalesce((SELECT length(CAST(module_path AS BLOB)) FROM inline_module_owners WHERE declaration_key=?1 AND owner_id=?2),0)",rusqlite::params![key,owner],|r|r.get(0))?;
                if size > 1024 {
                    return Err(Error::Limit);
                }
                if size > 0 {
                    bytes = bytes
                        .checked_add(size as usize * 2 + 128)
                        .ok_or(Error::Limit)?;
                    if bytes > limits.bytes {
                        return Err(Error::Limit);
                    }
                    let module:String=self.connection.query_row("SELECT module_path FROM inline_module_owners WHERE declaration_key=?1 AND owner_id=?2",rusqlite::params![key,owner],|r|r.get(0))?;
                    item["module_path"] = serde_json::json!(module);
                    item["children"] = serde_json::json!({"level":"module","module_path":module});
                }
            }
        }
        Ok(
            serde_json::json!({"branch":{"owner_id":owner,"parent_key":parent},"items":items,"complete_branch":true}),
        )
    }
    pub fn declaration_source(
        &self,
        key: &str,
        max_bytes: u64,
    ) -> Result<(Vec<u8>, serde_json::Value)> {
        self.require_inline()?;
        let (rowid,start,end,length,pathlen):(i64,i64,i64,i64,i64)=self.connection.query_row("SELECT s.rowid,d.byte_start,d.byte_end,length(s.content),length(CAST(d.xml_path AS BLOB)) FROM inline_declarations d JOIN sources s ON s.path=d.xml_path WHERE d.declaration_key=?1",[key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
        if start < 0
            || end <= start
            || end > length
            || (end - start) as u64 > max_bytes
            || pathlen > 1024
        {
            return Err(Error::Limit);
        }
        let path: String = self.connection.query_row(
            "SELECT xml_path FROM inline_declarations WHERE declaration_key=?1",
            [key],
            |r| r.get(0),
        )?;
        let mut blob =
            self.connection
                .blob_open(DatabaseName::Main, "sources", "content", rowid, true)?;
        blob.seek(SeekFrom::Start(start as u64))?;
        let mut bytes = vec![0; (end - start) as usize];
        blob.read_exact(&mut bytes)?;
        Ok((
            bytes,
            serde_json::json!({"path":path,"start_byte":start,"end_byte":end,"byte_units":"0_based_end_exclusive","semantics":"not_analyzed"}),
        ))
    }

    /// Returns scope and temporary coverage; no Git-origin claim.
    pub fn info(&self) -> Result<serde_json::Value> {
        let (id, scope, coverage, profile): (String, String, String, String) = self
            .connection
            .query_row(
            "SELECT snapshot_id,scope_id,coverage_draft,profile FROM metadata WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        Ok(
            serde_json::json!({"snapshot_id":id,"profile":profile,"scope_id":scope,"coverage":serde_json::from_str::<serde_json::Value>(&coverage).map_err(|e|Error::Contract(e.to_string()))?,"relations":"not_analyzed","git_provenance":"not_verified","experimental":true}),
        )
    }

    /// Explicit offline re-derivation. Limits are caller policy, never trusted from metadata.
    pub fn verify(&self, limits: snapshot_v5::Limits) -> Result<()> {
        let profile: String =
            self.connection
                .query_row("SELECT profile FROM metadata", [], |r| r.get(0))?;
        if [
            snapshot_v5::streaming_writer::PROFILE,
            snapshot_v5::inline_writer::PROFILE,
            snapshot_v5::companion_writer::PROFILE,
            snapshot_v5::common_writer::PROFILE,
            snapshot_v5::root_writer::PROFILE,
            snapshot_v5::shared_writer::PROFILE,
            snapshot_v5::leaf_writer::PROFILE,
            snapshot_v5::journal_writer::PROFILE,
            snapshot_v5::service_writer::PROFILE,
            snapshot_v5::chart_writer::PROFILE,
            snapshot_v5::register_writer::PROFILE,
            snapshot_v5::report_writer::PROFILE,
            snapshot_v5::workflow_writer::PROFILE,
            snapshot_v5::corpus_writer::PROFILE,
            snapshot_v5::corpus_bin_writer::PROFILE,
            snapshot_v5::corpus_help_writer::PROFILE,
            snapshot_v5::corpus_config_writer::PROFILE,
            snapshot_v5::diagnostics_writer::PROFILE,
            snapshot_v5::optional_form_writer::PROFILE,
            snapshot_v5::sidecar_writer::PROFILE,
            snapshot_v5::context_writer::PROFILE,
            snapshot_v5::parameter_writer::PROFILE,
            snapshot_v5::conditional_writer::PROFILE,
            snapshot_v5::string_writer::PROFILE,
            snapshot_v5::literal_writer::PROFILE,
            snapshot_v5::wrapped_writer::PROFILE,
            snapshot_v5::refined_writer::PROFILE,
            snapshot_v5::annotated_vars_writer::PROFILE,
            snapshot_v5::dates_members_writer::PROFILE,
            snapshot_v5::async_casing_writer::PROFILE,
            snapshot_v5::mobile_vars_writer::PROFILE,
            snapshot_v5::method_regions_writer::PROFILE,
            snapshot_v5::header_forms_writer::PROFILE,
            snapshot_v5::source_limits_writer::PROFILE,
            snapshot_v5::long_headers_writer::PROFILE,
            snapshot_v5::lexical_tail_writer::PROFILE,
            snapshot_v5::source_anomalies_writer::PROFILE,
            snapshot_v5::navigator_agreement_writer::PROFILE,
            snapshot_v5::navigator_owners_writer::PROFILE,
            snapshot_v5::navigator_sppr::PROFILE,
        ]
        .contains(&profile.as_str())
        {
            return Err(Error::Contract("stream_verify_limits_required".into()));
        }
        verify_saved_sources(
            &self.connection,
            self._copy.path.parent().ok_or(Error::Range)?,
            limits,
        )
    }
    /// New profile requires explicit caller facts/coverage/time policy as well.
    pub fn verify_streaming(&self, limits: snapshot_v5::streaming_facts::Limits) -> Result<()> {
        self.verify_streaming_mode(limits, None, None)
    }
    pub fn verify_streaming_with_opaque_bin(
        &self,
        limits: snapshot_v5::streaming_facts::Limits,
        bin_bytes: u64,
    ) -> Result<()> {
        self.verify_streaming_mode(limits, Some(bin_bytes), None)
    }
    pub fn verify_streaming_with_diagnostics(
        &self,
        limits: snapshot_v5::streaming_facts::Limits,
        bin_bytes: u64,
        diagnostics_bytes: u64,
    ) -> Result<()> {
        self.verify_streaming_mode(limits, Some(bin_bytes), Some(diagnostics_bytes))
    }
    fn verify_streaming_mode(
        &self,
        limits: snapshot_v5::streaming_facts::Limits,
        bin_bytes: Option<u64>,
        diagnostics_bytes: Option<u64>,
    ) -> Result<()> {
        limits.validate()?;
        let profile: String =
            self.connection
                .query_row("SELECT profile FROM metadata", [], |r| r.get(0))?;
        if ![
            snapshot_v5::streaming_writer::PROFILE,
            snapshot_v5::inline_writer::PROFILE,
            snapshot_v5::companion_writer::PROFILE,
            snapshot_v5::common_writer::PROFILE,
            snapshot_v5::root_writer::PROFILE,
            snapshot_v5::shared_writer::PROFILE,
            snapshot_v5::leaf_writer::PROFILE,
            snapshot_v5::journal_writer::PROFILE,
            snapshot_v5::service_writer::PROFILE,
            snapshot_v5::chart_writer::PROFILE,
            snapshot_v5::register_writer::PROFILE,
            snapshot_v5::report_writer::PROFILE,
            snapshot_v5::workflow_writer::PROFILE,
            snapshot_v5::corpus_writer::PROFILE,
            snapshot_v5::corpus_bin_writer::PROFILE,
            snapshot_v5::corpus_help_writer::PROFILE,
            snapshot_v5::corpus_config_writer::PROFILE,
            snapshot_v5::diagnostics_writer::PROFILE,
            snapshot_v5::optional_form_writer::PROFILE,
            snapshot_v5::sidecar_writer::PROFILE,
            snapshot_v5::context_writer::PROFILE,
            snapshot_v5::parameter_writer::PROFILE,
            snapshot_v5::conditional_writer::PROFILE,
            snapshot_v5::string_writer::PROFILE,
            snapshot_v5::literal_writer::PROFILE,
            snapshot_v5::wrapped_writer::PROFILE,
            snapshot_v5::refined_writer::PROFILE,
            snapshot_v5::annotated_vars_writer::PROFILE,
            snapshot_v5::dates_members_writer::PROFILE,
            snapshot_v5::async_casing_writer::PROFILE,
            snapshot_v5::mobile_vars_writer::PROFILE,
            snapshot_v5::method_regions_writer::PROFILE,
            snapshot_v5::header_forms_writer::PROFILE,
            snapshot_v5::source_limits_writer::PROFILE,
            snapshot_v5::long_headers_writer::PROFILE,
            snapshot_v5::lexical_tail_writer::PROFILE,
            snapshot_v5::source_anomalies_writer::PROFILE,
            snapshot_v5::navigator_agreement_writer::PROFILE,
            snapshot_v5::navigator_owners_writer::PROFILE,
            snapshot_v5::navigator_sppr::PROFILE,
        ]
        .contains(&profile.as_str())
        {
            return Err(Error::Profile);
        }
        verify_saved_sources_mode(
            &self.connection,
            self._copy.path.parent().ok_or(Error::Range)?,
            limits.sources,
            Some(limits),
            bin_bytes,
            diagnostics_bytes,
        )
    }
}

fn verify_saved_sources(
    original: &Connection,
    staging: &Path,
    limits: snapshot_v5::Limits,
) -> Result<()> {
    verify_saved_sources_mode(original, staging, limits, None, None, None)
}
fn verify_saved_sources_mode(
    original: &Connection,
    staging: &Path,
    limits: snapshot_v5::Limits,
    stream: Option<snapshot_v5::streaming_facts::Limits>,
    bin_bytes: Option<u64>,
    diagnostics_bytes: Option<u64>,
) -> Result<()> {
    use sha1::Sha1;
    use sha2::{Digest, Sha256};
    let profile: String = original.query_row("SELECT profile FROM metadata", [], |r| r.get(0))?;
    if ([
        snapshot_v5::diagnostics_writer::PROFILE,
        snapshot_v5::optional_form_writer::PROFILE,
        snapshot_v5::sidecar_writer::PROFILE,
        snapshot_v5::context_writer::PROFILE,
        snapshot_v5::parameter_writer::PROFILE,
        snapshot_v5::conditional_writer::PROFILE,
        snapshot_v5::string_writer::PROFILE,
        snapshot_v5::literal_writer::PROFILE,
        snapshot_v5::wrapped_writer::PROFILE,
        snapshot_v5::refined_writer::PROFILE,
        snapshot_v5::annotated_vars_writer::PROFILE,
        snapshot_v5::dates_members_writer::PROFILE,
        snapshot_v5::async_casing_writer::PROFILE,
        snapshot_v5::mobile_vars_writer::PROFILE,
        snapshot_v5::method_regions_writer::PROFILE,
        snapshot_v5::header_forms_writer::PROFILE,
        snapshot_v5::source_limits_writer::PROFILE,
        snapshot_v5::long_headers_writer::PROFILE,
        snapshot_v5::lexical_tail_writer::PROFILE,
        snapshot_v5::source_anomalies_writer::PROFILE,
        snapshot_v5::navigator_agreement_writer::PROFILE,
        snapshot_v5::navigator_owners_writer::PROFILE,
        snapshot_v5::navigator_sppr::PROFILE,
    ]
    .contains(&profile.as_str()))
        != diagnostics_bytes.is_some()
    {
        return Err(Error::Contract("diagnostics_verify_limit_required".into()));
    }
    if let Some(n) = diagnostics_bytes {
        snapshot_v5::diagnostics_writer::check_limit(n)?;
    }
    let bin_policy = if [
        snapshot_v5::corpus_bin_writer::PROFILE,
        snapshot_v5::corpus_help_writer::PROFILE,
        snapshot_v5::corpus_config_writer::PROFILE,
        snapshot_v5::diagnostics_writer::PROFILE,
        snapshot_v5::optional_form_writer::PROFILE,
        snapshot_v5::sidecar_writer::PROFILE,
        snapshot_v5::context_writer::PROFILE,
        snapshot_v5::parameter_writer::PROFILE,
        snapshot_v5::conditional_writer::PROFILE,
        snapshot_v5::string_writer::PROFILE,
        snapshot_v5::literal_writer::PROFILE,
        snapshot_v5::wrapped_writer::PROFILE,
        snapshot_v5::refined_writer::PROFILE,
        snapshot_v5::annotated_vars_writer::PROFILE,
        snapshot_v5::dates_members_writer::PROFILE,
        snapshot_v5::async_casing_writer::PROFILE,
        snapshot_v5::mobile_vars_writer::PROFILE,
        snapshot_v5::method_regions_writer::PROFILE,
        snapshot_v5::header_forms_writer::PROFILE,
        snapshot_v5::source_limits_writer::PROFILE,
        snapshot_v5::long_headers_writer::PROFILE,
        snapshot_v5::lexical_tail_writer::PROFILE,
        snapshot_v5::source_anomalies_writer::PROFILE,
        snapshot_v5::navigator_agreement_writer::PROFILE,
        snapshot_v5::navigator_owners_writer::PROFILE,
        snapshot_v5::navigator_sppr::PROFILE,
    ]
    .contains(&profile.as_str())
    {
        let stored = snapshot_v5::corpus_bin_writer::stored_policy(original)?;
        let n = bin_bytes.ok_or(Error::Contract("opaque_bin_verify_limit_required".into()))?;
        let mut policy = snapshot_v5::corpus_bin_writer::Policy::with_template_limit(
            n,
            &stored.globals,
            stored.large_templates,
        )?;
        if stored.omitted_templates.iter().any(|_| n != stored.bytes) {
            return Err(Error::Contract("sppr_verify_omission_limit".into()));
        }
        policy.omitted_templates = stored.omitted_templates.clone();
        policy.root_help = stored.root_help;
        policy.configuration = stored.configuration;
        policy.diagnostics = diagnostics_bytes;
        policy.optional_form = stored.optional_form;
        policy.root_sidecars = stored.root_sidecars;
        policy.context = stored.context;
        policy.headers = stored.headers;
        policy.conditional = stored.conditional;
        policy.strings = stored.strings;
        policy.literals = stored.literals;
        policy.wrapped = stored.wrapped;
        policy.refined = stored.refined;
        policy.annotated_vars = stored.annotated_vars;
        policy.dates_members = stored.dates_members;
        policy.async_casing = stored.async_casing;
        policy.mobile_vars = stored.mobile_vars;
        policy.method_regions = stored.method_regions;
        policy.header_forms = stored.header_forms;
        policy.source_limits = stored.source_limits;
        policy.long_headers = stored.long_headers;
        policy.lexical_tail = stored.lexical_tail;
        policy.source_anomalies = stored.source_anomalies;
        policy.command_owners = stored.command_owners;
        policy.omit_command_help = stored.omit_command_help;
        policy.processor_help = stored.processor_help;
        Some(policy)
    } else {
        if bin_bytes.is_some() {
            return Err(Error::Contract("opaque_bin_profile_required".into()));
        }
        None
    };
    let (count, total): (u64, u64) = original.query_row(
        "SELECT count(*),coalesce(sum(length(content)),0) FROM sources",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if count > limits.source_count || total > limits.total_source_bytes {
        return Err(Error::Limit);
    }
    let (scratch_guard, handle) = linux_v5_staging::temporary(staging).map_err(Error::Staging)?;
    drop(handle);
    // Declaration order ensures transaction/connection close before the RAII unlink.
    let mut scratch =
        Connection::open_with_flags(&scratch_guard.path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let pages = (limits.sqlite_bytes / 4096).min(4_294_967_294);
    if pages == 0 {
        return Err(Error::Limit);
    }
    scratch.pragma_update(None, "page_size", 4096)?;
    scratch.pragma_update(None, "max_page_count", pages)?;
    let inline=original.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='inline_declarations' AND type='table')",[],|r|r.get::<_,bool>(0))?;
    let companions = original.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='companions')",
        [],
        |r| r.get::<_, bool>(0),
    )?;
    let common = original.query_row("SELECT profile FROM metadata", [], |r| {
        r.get::<_, String>(0)
    })? == snapshot_v5::common_writer::PROFILE;
    let roots = original.query_row("SELECT profile FROM metadata", [], |r| {
        r.get::<_, String>(0)
    })? == snapshot_v5::root_writer::PROFILE;
    let shared = original.query_row("SELECT profile FROM metadata", [], |r| {
        r.get::<_, String>(0)
    })? == snapshot_v5::shared_writer::PROFILE;
    let leaves = original.query_row("SELECT profile FROM metadata", [], |r| {
        r.get::<_, String>(0)
    })? == snapshot_v5::leaf_writer::PROFILE;
    let journals = original.query_row("SELECT profile FROM metadata", [], |r| {
        r.get::<_, String>(0)
    })? == snapshot_v5::journal_writer::PROFILE;
    let services = original.query_row("SELECT profile FROM metadata", [], |r| {
        r.get::<_, String>(0)
    })? == snapshot_v5::service_writer::PROFILE;
    let charts = original.query_row("SELECT profile FROM metadata", [], |r| {
        r.get::<_, String>(0)
    })? == snapshot_v5::chart_writer::PROFILE;
    let registers = original.query_row("SELECT profile FROM metadata", [], |r| {
        r.get::<_, String>(0)
    })? == snapshot_v5::register_writer::PROFILE;
    let reports = original.query_row("SELECT profile FROM metadata", [], |r| {
        r.get::<_, String>(0)
    })? == snapshot_v5::report_writer::PROFILE;
    let workflows = original.query_row("SELECT profile FROM metadata", [], |r| {
        r.get::<_, String>(0)
    })? == snapshot_v5::workflow_writer::PROFILE;
    let corpus = original.query_row("SELECT profile FROM metadata", [], |r| {
        r.get::<_, String>(0)
    })? == snapshot_v5::corpus_writer::PROFILE;
    if bin_policy
        .as_ref()
        .is_some_and(|b| b.processor_help.is_some())
    {
        scratch.execute_batch(&snapshot_v5::navigator_sppr::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.command_owners) {
        scratch.execute_batch(&snapshot_v5::navigator_owners_writer::schema())?;
    } else if bin_policy
        .as_ref()
        .is_some_and(|b| b.source_anomalies && b.lexical_tail)
    {
        scratch.execute_batch(&snapshot_v5::navigator_agreement_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.source_anomalies) {
        scratch.execute_batch(&snapshot_v5::source_anomalies_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.lexical_tail) {
        scratch.execute_batch(&snapshot_v5::lexical_tail_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.long_headers) {
        scratch.execute_batch(&snapshot_v5::long_headers_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.source_limits) {
        scratch.execute_batch(&snapshot_v5::source_limits_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.header_forms) {
        scratch.execute_batch(&snapshot_v5::header_forms_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.method_regions) {
        scratch.execute_batch(&snapshot_v5::method_regions_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.mobile_vars) {
        scratch.execute_batch(&snapshot_v5::mobile_vars_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.async_casing) {
        scratch.execute_batch(&snapshot_v5::async_casing_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.dates_members) {
        scratch.execute_batch(&snapshot_v5::dates_members_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.annotated_vars) {
        scratch.execute_batch(&snapshot_v5::annotated_vars_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.refined) {
        scratch.execute_batch(&snapshot_v5::refined_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.wrapped) {
        scratch.execute_batch(&snapshot_v5::wrapped_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.literals) {
        scratch.execute_batch(&snapshot_v5::literal_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.strings) {
        scratch.execute_batch(&snapshot_v5::string_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.conditional) {
        scratch.execute_batch(&snapshot_v5::conditional_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.headers) {
        scratch.execute_batch(&snapshot_v5::parameter_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.context) {
        scratch.execute_batch(&snapshot_v5::context_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.root_sidecars) {
        scratch.execute_batch(&snapshot_v5::sidecar_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.optional_form) {
        scratch.execute_batch(&snapshot_v5::optional_form_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.diagnostics.is_some()) {
        scratch.execute_batch(&snapshot_v5::diagnostics_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.configuration) {
        scratch.execute_batch(&snapshot_v5::corpus_config_writer::schema())?;
    } else if bin_policy.as_ref().is_some_and(|b| b.root_help) {
        scratch.execute_batch(&snapshot_v5::corpus_help_writer::schema())?;
    } else if bin_policy.is_some() {
        scratch.execute_batch(&snapshot_v5::corpus_bin_writer::schema())?;
    } else if corpus {
        scratch.execute_batch(&snapshot_v5::corpus_writer::schema())?;
    } else if workflows {
        scratch.execute_batch(&snapshot_v5::workflow_writer::schema())?;
    } else if reports {
        scratch.execute_batch(&snapshot_v5::report_writer::schema())?;
    } else if registers {
        scratch.execute_batch(&snapshot_v5::register_writer::schema())?;
    } else if charts {
        scratch.execute_batch(&snapshot_v5::chart_writer::schema())?;
    } else if services {
        scratch.execute_batch(&snapshot_v5::service_writer::schema())?;
    } else if journals {
        scratch.execute_batch(&snapshot_v5::journal_writer::schema())?;
    } else if leaves {
        scratch.execute_batch(&snapshot_v5::leaf_writer::schema())?;
    } else if shared {
        scratch.execute_batch(&snapshot_v5::shared_writer::schema())?;
    } else if roots {
        scratch.execute_batch(&snapshot_v5::root_writer::schema())?;
    } else if common {
        scratch.execute_batch(&snapshot_v5::common_writer::schema())?;
    } else if companions {
        scratch.execute_batch(&snapshot_v5::companion_writer::schema())?;
    } else if inline {
        scratch.execute_batch(&snapshot_v5::inline_writer::schema())?;
    } else {
        scratch.execute_batch(contract::SCHEMA)?;
    }
    scratch.execute_batch("PRAGMA trusted_schema=OFF")?;
    let tx = scratch.transaction()?;
    let mut query=original.prepare("SELECT rowid,length(content),length(CAST(path AS BLOB)),length(CAST(kind AS BLOB)),length(CAST(mode AS BLOB)),length(CAST(blob_oid AS BLOB)),length(CAST(sha256 AS BLOB)) FROM sources ORDER BY path COLLATE BINARY")?;
    let mut rows = query.query([])?;
    let mut buffer = [0u8; 64 * 1024];
    while let Some(row) = rows.next()? {
        let rowid: i64 = row.get(0)?;
        let length: u64 = row.get(1)?;
        if length > i32::MAX as u64 {
            return Err(Error::Limit);
        }
        for (i, cap) in [
            (2, 16_384u64),
            (3, if companions { 7 } else { 3 }),
            (4, 6),
            (5, 64),
            (6, 64),
        ] {
            if row.get::<_, u64>(i)? > cap {
                return Err(Error::Limit);
            }
        }
        let (path, kind, mode, oid, sha): (String, String, String, String, String) = original
            .query_row(
                "SELECT path,kind,mode,blob_oid,sha256 FROM sources WHERE rowid=?1",
                [rowid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?;
        if length
            > bin_policy.as_ref().map_or(limits.source_bytes, |b| {
                b.cap(&path, &kind, limits.source_bytes)
            })
        {
            return Err(Error::Limit);
        }
        if ![40, 64].contains(&oid.len()) {
            return Err(Error::SourceDigest);
        }
        tx.execute("INSERT INTO sources(path,kind,mode,blob_oid,sha256,content) VALUES (?1,?2,?3,?4,?5,zeroblob(?6))",rusqlite::params![path,kind,mode,oid,sha,length])?;
        let mut input =
            original.blob_open(DatabaseName::Main, "sources", "content", rowid, true)?;
        let mut output = tx.blob_open(
            DatabaseName::Main,
            "sources",
            "content",
            tx.last_insert_rowid(),
            false,
        )?;
        let mut content_hash = Sha256::new();
        let mut git_sha1 = Sha1::new();
        let mut git_sha256 = Sha256::new();
        let header = format!("blob {length}\0");
        git_sha1.update(header.as_bytes());
        git_sha256.update(header.as_bytes());
        loop {
            let n = input.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            content_hash.update(&buffer[..n]);
            git_sha1.update(&buffer[..n]);
            git_sha256.update(&buffer[..n]);
            output.write_all(&buffer[..n])?;
        }
        let actual_oid = if oid.len() == 40 {
            format!("{:x}", git_sha1.finalize())
        } else {
            format!("{:x}", git_sha256.finalize())
        };
        if format!("{:x}", content_hash.finalize()) != sha || actual_oid != oid {
            return Err(Error::SourceDigest);
        }
    }
    if let Some(policy) = stream {
        tx.commit()?;
        let omit_command_help = bin_policy.as_ref().is_some_and(|b| b.omit_command_help);
        let processor_help = bin_policy.as_ref().and_then(|b| b.processor_help.clone());
        let mut coverage = if let Some(bin) = bin_policy {
            snapshot_v5::streaming_facts::derive_corpus_bin(&mut scratch, policy, bin)?
        } else if corpus {
            snapshot_v5::streaming_facts::derive_corpus(&mut scratch, policy)?
        } else if workflows {
            snapshot_v5::streaming_facts::derive_workflows(&mut scratch, policy)?
        } else if reports {
            snapshot_v5::streaming_facts::derive_reports(&mut scratch, policy)?
        } else if registers {
            snapshot_v5::streaming_facts::derive_registers(&mut scratch, policy)?
        } else if charts {
            snapshot_v5::streaming_facts::derive_charts(&mut scratch, policy)?
        } else if services {
            snapshot_v5::streaming_facts::derive_services(&mut scratch, policy)?
        } else if journals {
            snapshot_v5::streaming_facts::derive_journals(&mut scratch, policy)?
        } else if leaves {
            snapshot_v5::streaming_facts::derive_leaves(&mut scratch, policy)?
        } else if shared {
            snapshot_v5::streaming_facts::derive_shared(&mut scratch, policy)?
        } else if roots {
            snapshot_v5::streaming_facts::derive_roots(&mut scratch, policy)?
        } else if common {
            snapshot_v5::streaming_facts::derive_common_modules(&mut scratch, policy)?
        } else if companions {
            snapshot_v5::streaming_facts::derive_companions(&mut scratch, policy)?
        } else if inline {
            snapshot_v5::streaming_facts::derive_inline(&mut scratch, policy)?
        } else {
            snapshot_v5::streaming_facts::derive(&mut scratch, policy)?
        };
        if omit_command_help {
            let manifest = snapshot_v5::navigator_command_help_writer::read_manifest(original)?;
            snapshot_v5::navigator_command_help_writer::apply_coverage(&mut coverage, &manifest)?;
            if let Some(owners) = processor_help {
                snapshot_v5::navigator_sppr::apply_coverage(
                    &mut coverage,
                    &manifest,
                    &owners,
                    snapshot_v5::corpus_bin_writer::stored_policy(original)?.bytes,
                )?;
            }
        }
        let saved: String =
            original.query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))?;
        if serde_json::from_str::<serde_json::Value>(&saved)
            .map_err(|e| Error::Contract(e.to_string()))?
            != coverage
        {
            return Err(Error::VerifyFacts);
        }
        if contract::facts_sha256(&scratch)? != contract::facts_sha256(original)? {
            return Err(Error::VerifyFacts);
        }
    } else {
        snapshot_v5::derive_facts(&tx)?;
        if contract::facts_sha256(&tx)? != contract::facts_sha256(original)? {
            return Err(Error::VerifyFacts);
        }
        drop(tx);
    }
    scratch.close().map_err(|(_, e)| Error::Sqlite(e))?;
    drop(scratch_guard);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wal_signature_and_checksum_fail_before_sqlite_without_sidecars() {
        use sha2::{Digest, Sha256};
        use std::{fs, os::unix::fs::PermissionsExt};
        let root = std::env::temp_dir().join(format!("bslens-s4-wal-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let staging = root.join("stage");
        fs::create_dir(&staging).unwrap();
        fs::set_permissions(&staging, fs::Permissions::from_mode(0o700)).unwrap();
        let original = root.join("fixture.sqlite");
        let db = Connection::open(&original).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE example(value); INSERT INTO example VALUES(1); PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
        drop(db);
        let wal = fs::read(&original).unwrap();
        assert_eq!(&wal[18..20], &[2, 2]);
        let check = |bytes: &[u8], corrupt: bool| {
            let path = root.join(format!(
                "bslens-v5-{}.{}.sqlite",
                "0".repeat(64),
                format!("{:x}", Sha256::digest(bytes))
            ));
            fs::write(&path, bytes).unwrap();
            if corrupt {
                fs::write(&path, b"bad checksum and bad header").unwrap();
            }
            let mut called = false;
            let result = LinuxSnapshotReader::open_with(&path, &staging, None, 1_000_000, |_| {
                called = true;
                panic!("SQLite must not be called")
            });
            assert!(!called);
            if corrupt {
                assert!(matches!(result, Err(Error::Staging(StagingError::Digest))));
            } else {
                assert!(matches!(
                    result,
                    Err(Error::Staging(StagingError::NotStandalone))
                ));
            }
            assert_eq!(
                fs::read_dir(&staging).unwrap().count(),
                0,
                "no private copies or WAL/SHM/journal sidecars"
            );
        };
        check(&wal, false);
        check(&wal, true);
        let mut bad = wal.clone();
        bad[..16].fill(0);
        bad[18] = 1;
        bad[19] = 1;
        check(&bad, false);
        let mut modes = wal.clone();
        modes[18] = 1;
        check(&modes, false);
        modes[18] = 2;
        modes[19] = 1;
        check(&modes, false);
        check(b"short", false);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_and_tree_use_saved_unparsed_bytes_without_parser() {
        use sha2::{Digest, Sha256};
        use std::{fs, os::unix::fs::PermissionsExt};
        let root = std::env::temp_dir().join(format!("bslens-s4-reader-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let raw = root.join("build.sqlite");
        let db = Connection::open(&raw).unwrap();
        db.execute_batch(include_str!("../spikes/linux-v5/schema.sql"))
            .unwrap();
        let bytes = b"#unsupported\r\nsecond\nlast";
        db.execute(
            "INSERT INTO sources VALUES('x.bsl','bsl','100644',?1,?2,?3)",
            rusqlite::params![
                "0".repeat(40),
                format!("{:x}", Sha256::digest(bytes)),
                bytes
            ],
        )
        .unwrap();
        db.execute_batch("INSERT INTO modules VALUES('x.bsl','bsl',NULL,'unparsed','DirectiveUnsupported');
          INSERT INTO source_lines VALUES('x.bsl','bsl',1,0,14),('x.bsl','bsl',2,14,21),('x.bsl','bsl',3,21,25);").unwrap();
        // This explicitly constructed fixture is not an S3 integration test.
        // Derive physical offsets for the fixture, independently of any product parser.
        let first = bytes.iter().position(|b| *b == b'\n').unwrap() + 1;
        let second = bytes[first..].iter().position(|b| *b == b'\n').unwrap() + 1 + first;
        db.execute(
            "UPDATE source_lines SET byte_end=?1 WHERE line_number=1",
            [first as i64],
        )
        .unwrap();
        db.execute(
            "UPDATE source_lines SET byte_start=?1,byte_end=?2 WHERE line_number=2",
            [first as i64, second as i64],
        )
        .unwrap();
        db.execute(
            "UPDATE source_lines SET byte_start=?1,byte_end=?2 WHERE line_number=3",
            [second as i64, bytes.len() as i64],
        )
        .unwrap();
        drop(db);
        let named = root.join(format!(
            "bslens-v5-0000000000000000000000000000000000000000000000000000000000000000.{:x}.sqlite",
            Sha256::digest(fs::read(&raw).unwrap())
        ));
        fs::rename(&raw, &named).unwrap();
        let copy = linux_v5_staging::stage(&named, &root, None, 1_000_000).unwrap();
        let copy_path = copy.path.clone();
        let connection =
            Connection::open_with_flags(&copy.path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        connection
            .execute_batch("PRAGMA query_only=ON;PRAGMA trusted_schema=OFF")
            .unwrap();
        let reader = LinuxSnapshotReader {
            connection,
            _copy: copy,
        };
        assert_eq!(reader.source("x.bsl", 2, 3, 100).unwrap(), b"second\nlast");
        assert!(matches!(reader.source("x.bsl", 2, 3, 2), Err(Error::Range)));
        let nodes = reader
            .tree(
                "unconfirmed",
                None,
                None,
                None,
                TreeLimits {
                    rows: 10,
                    bytes: 1024,
                },
            )
            .unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].status.as_deref(), Some("unparsed"));
        drop(reader);
        assert!(!copy_path.exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn tree_row_and_utf8_byte_limits_are_exact_and_reject_giant_field() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../spikes/linux-v5/schema.sql"))
            .unwrap();
        // Isolate branch reader from S3 and FK validation; no production fixture claim.
        db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        db.execute(
            "INSERT INTO modules(path,parse_status) VALUES (?1,'accepted')",
            ["я.bsl"],
        )
        .unwrap();
        let exact = "module".len() + 2 * "я.bsl".len() + "accepted".len();
        let fetch = |rows, bytes| {
            read_tree(
                &db,
                "unconfirmed",
                None,
                None,
                None,
                TreeLimits { rows, bytes },
            )
        };
        assert_eq!(fetch(1, exact).unwrap().len(), 1);
        assert!(matches!(fetch(1, exact - 1), Err(Error::Limit)));
        assert!(matches!(fetch(0, exact), Err(Error::Limit)));
        for i in 0..100 {
            db.execute(
                "INSERT INTO modules(path,parse_status) VALUES (?1,'accepted')",
                [format!("{i}.bsl")],
            )
            .unwrap();
        }
        assert!(matches!(fetch(10, 1_000_000), Err(Error::Limit)));
        db.execute("DELETE FROM modules", []).unwrap();
        db.execute(
            "INSERT INTO modules(path,parse_status) VALUES (?1,'accepted')",
            ["x".repeat(1_000_000)],
        )
        .unwrap();
        assert!(matches!(fetch(1, 1024), Err(Error::Limit)));
    }
}

#[derive(Debug)]
pub struct TreeNode {
    pub node_type: String,
    pub key: String,
    pub name: String,
    pub status: Option<String>,
    pub reason: Option<String>,
}
