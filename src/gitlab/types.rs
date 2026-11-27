//! Shared local adapter result types.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Eligible,
    ExcludedVendorUpdate,
    Indeterminate,
}

macro_rules! reason_codes {
    ($($name:ident => $value:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum ReasonCode { $($name),+ }
        impl ReasonCode {
            pub const ALL: &'static [Self] = &[$(Self::$name),+];
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$name => $value),+ }
            }
        }
    };
}
reason_codes! {
    VendorVersionChangedWithoutSupportChange => "vendor_version_changed_without_support_change",
    SupportSettingsChanged => "support_settings_changed",
    SupportFileAdded => "support_file_added",
    SupportFileRemoved => "support_file_removed",
    ScaleThreshold => "scale_threshold",
    VendorServicePath => "vendor_service_path",
    MetadataUnavailable => "metadata_unavailable",
    MetadataFormatUnsupported => "metadata_format_unsupported",
    MultipleMergeBases => "multiple_merge_bases",
    MergeBaseMismatch => "merge_base_mismatch",
    StaleTarget => "stale_target",
    ShallowRepository => "shallow_repository",
    MissingObject => "missing_object",
    NotACommit => "not_a_commit",
    UnsupportedEntry => "unsupported_entry",
    NonUtf8Path => "non_utf8_path",
    EmptyScope => "empty_scope",
    LimitExceeded => "limit_exceeded",
    CommitSetMismatch => "commit_set_mismatch",
    GitFailure => "git_failure",
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Change {
    Added,
    Modified,
    Deleted,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangedPath {
    pub path: String,
    pub change: Change,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Evidence {
    pub rule_id: Option<String>,
    pub paths: Vec<String>,
    pub base_oid: Option<String>,
    pub head_oid: Option<String>,
    pub properties: Vec<PropertyChange>,
    pub changed_count: Option<usize>,
    pub scope_count: Option<usize>,
    pub scale_min_paths: Option<usize>,
    pub scale_min_percent: Option<u32>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyChange {
    pub name: String,
    pub base_value: String,
    pub head_value: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Finding {
    pub reason: ReasonCode,
    pub evidence: Evidence,
}
impl Finding {
    pub fn new(reason: ReasonCode) -> Self {
        Self {
            reason,
            evidence: Evidence::default(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VendorPolicy {
    pub config_root: String,
    pub vendor_dirs: Vec<String>,
    pub scale_min_paths: usize,
    pub scale_min_percent: u32,
}
impl Default for VendorPolicy {
    fn default() -> Self {
        Self {
            config_root: ".".into(),
            vendor_dirs: Vec::new(),
            scale_min_paths: 100,
            scale_min_percent: 20,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_paths: usize,
    pub max_output_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_paths: 100_000,
            max_output_bytes: 64 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalInput {
    pub base: String,
    pub start: String,
    pub head: String,
    pub target: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reason_codes_are_unique_snake_case() {
        let mut seen = std::collections::HashSet::new();
        for code in ReasonCode::ALL {
            let s = code.as_str();
            assert!(seen.insert(s));
            assert!(s
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'));
            assert!(!s.starts_with('_') && !s.ends_with('_') && !s.contains("__"));
        }
    }
}
