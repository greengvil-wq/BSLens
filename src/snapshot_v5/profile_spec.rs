//! Named, immutable policies for the existing companion profile family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Feature {
    Off,
    On,
}
impl Feature {
    pub(super) fn enabled(self) -> bool {
        self == Self::On
    }
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Features {
    pub(super) common: Feature,
    pub(super) roots: Feature,
    pub(super) shared: Feature,
    pub(super) leaves: Feature,
    pub(super) journals: Feature,
    pub(super) services: Feature,
    pub(super) charts: Feature,
    pub(super) registers: Feature,
    pub(super) reports: Feature,
    pub(super) workflows: Feature,
    pub(super) corpus: Feature,
    pub(super) bin_profile: Feature,
    pub(super) root_help: Feature,
    pub(super) configuration: Feature,
    pub(super) diagnostics: Feature,
    pub(super) optional_form: Feature,
    pub(super) root_sidecars: Feature,
    pub(super) context: Feature,
    pub(super) headers: Feature,
    pub(super) conditional: Feature,
    pub(super) strings: Feature,
    pub(super) literals: Feature,
    pub(super) wrapped: Feature,
    pub(super) refined: Feature,
    pub(super) annotated_vars: Feature,
    pub(super) dates_members: Feature,
    pub(super) async_casing: Feature,
    pub(super) mobile_vars: Feature,
    pub(super) method_regions: Feature,
    pub(super) header_forms: Feature,
    pub(super) source_limits: Feature,
    pub(super) long_headers: Feature,
    pub(super) lexical_tail: Feature,
    pub(super) source_anomalies: Feature,
    pub(super) command_owners: Feature,
    pub(super) large_templates: Feature,
}
#[derive(Clone, Copy, Debug)]
pub(super) enum LexicalPolicy {
    AnnotatedVarsWriter,
    AsyncCasingWriter,
    Contract,
    ConditionalWriter,
    ContextWriter,
    DatesMembersWriter,
    HeaderFormsWriter,
    LexicalTailWriter,
    LiteralWriter,
    LongHeadersWriter,
    MethodRegionsWriter,
    MobileVarsWriter,
    NavigatorAgreementWriter,
    ParameterWriter,
    RefinedWriter,
    SourceAnomaliesWriter,
    SourceLimitsWriter,
    StringWriter,
    WrappedWriter,
}
impl LexicalPolicy {
    pub(super) fn value(self) -> &'static str {
        match self {
            Self::AnnotatedVarsWriter => super::annotated_vars_writer::LEXICAL,
            Self::AsyncCasingWriter => super::async_casing_writer::LEXICAL,
            Self::Contract => super::contract::LEXICAL,
            Self::ConditionalWriter => super::conditional_writer::LEXICAL,
            Self::ContextWriter => super::context_writer::LEXICAL,
            Self::DatesMembersWriter => super::dates_members_writer::LEXICAL,
            Self::HeaderFormsWriter => super::header_forms_writer::LEXICAL,
            Self::LexicalTailWriter => super::lexical_tail_writer::LEXICAL,
            Self::LiteralWriter => super::literal_writer::LEXICAL,
            Self::LongHeadersWriter => super::long_headers_writer::LEXICAL,
            Self::MethodRegionsWriter => super::method_regions_writer::LEXICAL,
            Self::MobileVarsWriter => super::mobile_vars_writer::LEXICAL,
            Self::NavigatorAgreementWriter => super::navigator_agreement_writer::LEXICAL,
            Self::ParameterWriter => super::parameter_writer::LEXICAL,
            Self::RefinedWriter => super::refined_writer::LEXICAL,
            Self::SourceAnomaliesWriter => super::source_anomalies_writer::LEXICAL,
            Self::SourceLimitsWriter => super::source_limits_writer::LEXICAL,
            Self::StringWriter => super::string_writer::LEXICAL,
            Self::WrappedWriter => super::wrapped_writer::LEXICAL,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProcessorHelpPolicy {
    Disabled,
    Sppr,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct ProfileSpec {
    #[allow(dead_code)]
    pub(super) name: &'static str,
    pub(super) features: Features,
    pub(super) lexical: LexicalPolicy,
    pub(super) omit_command_help: Feature,
    #[allow(dead_code)]
    pub(super) processor_help: ProcessorHelpPolicy,
}
pub(super) const COMPANION: ProfileSpec = ProfileSpec {
    name: "linux-v5-companion1-xml221-r09",
    features: Features {
        common: Feature::Off,
        roots: Feature::Off,
        shared: Feature::Off,
        leaves: Feature::Off,
        journals: Feature::Off,
        services: Feature::Off,
        charts: Feature::Off,
        registers: Feature::Off,
        reports: Feature::Off,
        workflows: Feature::Off,
        corpus: Feature::Off,
        bin_profile: Feature::Off,
        root_help: Feature::Off,
        configuration: Feature::Off,
        diagnostics: Feature::Off,
        optional_form: Feature::Off,
        root_sidecars: Feature::Off,
        context: Feature::Off,
        headers: Feature::Off,
        conditional: Feature::Off,
        strings: Feature::Off,
        literals: Feature::Off,
        wrapped: Feature::Off,
        refined: Feature::Off,
        annotated_vars: Feature::Off,
        dates_members: Feature::Off,
        async_casing: Feature::Off,
        mobile_vars: Feature::Off,
        method_regions: Feature::Off,
        header_forms: Feature::Off,
        source_limits: Feature::Off,
        long_headers: Feature::Off,
        lexical_tail: Feature::Off,
        source_anomalies: Feature::Off,
        command_owners: Feature::Off,
        large_templates: Feature::Off,
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const COMMON: ProfileSpec = ProfileSpec {
    name: "linux-v5-common1-xml221-r09",
    features: Features {
        common: Feature::On,
        ..COMPANION.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const ROOT: ProfileSpec = ProfileSpec {
    name: "linux-v5-roots1-xml221-r09",
    features: Features {
        roots: Feature::On,
        ..COMMON.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const SHARED: ProfileSpec = ProfileSpec {
    name: "linux-v5-shared1-xml221-r09",
    features: Features {
        shared: Feature::On,
        ..ROOT.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const LEAF: ProfileSpec = ProfileSpec {
    name: "linux-v5-leaf1-xml221-r09",
    features: Features {
        leaves: Feature::On,
        ..SHARED.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const JOURNAL: ProfileSpec = ProfileSpec {
    name: "linux-v5-journal1-xml221-r09",
    features: Features {
        journals: Feature::On,
        ..LEAF.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const SERVICE: ProfileSpec = ProfileSpec {
    name: "linux-v5-service1-xml221-r09",
    features: Features {
        services: Feature::On,
        ..JOURNAL.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const CHART: ProfileSpec = ProfileSpec {
    name: "linux-v5-chart1-xml221-r09",
    features: Features {
        charts: Feature::On,
        ..SERVICE.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const REGISTER: ProfileSpec = ProfileSpec {
    name: "linux-v5-register1-xml221-r09",
    features: Features {
        registers: Feature::On,
        ..CHART.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const REPORT: ProfileSpec = ProfileSpec {
    name: "linux-v5-report1-xml221-r09",
    features: Features {
        reports: Feature::On,
        ..REGISTER.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const WORKFLOW: ProfileSpec = ProfileSpec {
    name: "linux-v5-workflow1-xml221-r09",
    features: Features {
        workflows: Feature::On,
        ..REPORT.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const CORPUS: ProfileSpec = ProfileSpec {
    name: "linux-v5-corpus1-xml221-r09",
    features: Features {
        corpus: Feature::On,
        ..WORKFLOW.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const CORPUS_BIN: ProfileSpec = ProfileSpec {
    name: "linux-v5-corpus-bin1-xml221-r09",
    features: Features {
        bin_profile: Feature::On,
        ..CORPUS.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const CORPUS_HELP: ProfileSpec = ProfileSpec {
    name: "linux-v5-corpus-help1-xml221-r09",
    features: Features {
        root_help: Feature::On,
        ..CORPUS_BIN.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const CORPUS_CONFIG: ProfileSpec = ProfileSpec {
    name: "linux-v5-corpus-config1-xml221-r09",
    features: Features {
        configuration: Feature::On,
        ..CORPUS_HELP.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const DIAGNOSTICS: ProfileSpec = ProfileSpec {
    name: "linux-v5-diagnostics1-xml221-r09",
    features: Features {
        diagnostics: Feature::On,
        ..CORPUS_CONFIG.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const OPTIONAL_FORM: ProfileSpec = ProfileSpec {
    name: "linux-v5-optional-form1-xml221-r09",
    features: Features {
        optional_form: Feature::On,
        ..DIAGNOSTICS.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const SIDECAR: ProfileSpec = ProfileSpec {
    name: "linux-v5-root-sidecar1-xml221-r09",
    features: Features {
        root_sidecars: Feature::On,
        ..OPTIONAL_FORM.features
    },
    lexical: LexicalPolicy::Contract,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const CONTEXT: ProfileSpec = ProfileSpec {
    name: "linux-v5-context1-xml221-r09",
    features: Features {
        context: Feature::On,
        ..SIDECAR.features
    },
    lexical: LexicalPolicy::ContextWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const PARAMETER: ProfileSpec = ProfileSpec {
    name: "linux-v5-parameters1-xml221-r09",
    features: Features {
        headers: Feature::On,
        ..CONTEXT.features
    },
    lexical: LexicalPolicy::ParameterWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const CONDITIONAL: ProfileSpec = ProfileSpec {
    name: "linux-v5-conditionals1-xml221-r09",
    features: Features {
        conditional: Feature::On,
        ..PARAMETER.features
    },
    lexical: LexicalPolicy::ConditionalWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const STRING: ProfileSpec = ProfileSpec {
    name: "linux-v5-strings1-xml221-r09",
    features: Features {
        strings: Feature::On,
        ..CONDITIONAL.features
    },
    lexical: LexicalPolicy::StringWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const LITERAL: ProfileSpec = ProfileSpec {
    name: "linux-v5-literals1-xml221-r09",
    features: Features {
        literals: Feature::On,
        ..STRING.features
    },
    lexical: LexicalPolicy::LiteralWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const WRAPPED: ProfileSpec = ProfileSpec {
    name: "linux-v5-wrapped1-xml221-r09",
    features: Features {
        wrapped: Feature::On,
        ..LITERAL.features
    },
    lexical: LexicalPolicy::WrappedWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const REFINED: ProfileSpec = ProfileSpec {
    name: "linux-v5-refinements1-xml221-r09",
    features: Features {
        refined: Feature::On,
        ..WRAPPED.features
    },
    lexical: LexicalPolicy::RefinedWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const ANNOTATED_VARS: ProfileSpec = ProfileSpec {
    name: "linux-v5-annotated-vars1-xml221-r09",
    features: Features {
        annotated_vars: Feature::On,
        ..REFINED.features
    },
    lexical: LexicalPolicy::AnnotatedVarsWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const DATES_MEMBERS: ProfileSpec = ProfileSpec {
    name: "linux-v5-dates-members1-xml221-r09",
    features: Features {
        dates_members: Feature::On,
        ..ANNOTATED_VARS.features
    },
    lexical: LexicalPolicy::DatesMembersWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const ASYNC_CASING: ProfileSpec = ProfileSpec {
    name: "linux-v5-async-casing1-xml221-r09",
    features: Features {
        async_casing: Feature::On,
        ..DATES_MEMBERS.features
    },
    lexical: LexicalPolicy::AsyncCasingWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const MOBILE_VARS: ProfileSpec = ProfileSpec {
    name: "linux-v5-mobile-vars1-xml221-r09",
    features: Features {
        mobile_vars: Feature::On,
        ..ASYNC_CASING.features
    },
    lexical: LexicalPolicy::MobileVarsWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const METHOD_REGIONS: ProfileSpec = ProfileSpec {
    name: "linux-v5-method-regions1-xml221-r09",
    features: Features {
        method_regions: Feature::On,
        ..MOBILE_VARS.features
    },
    lexical: LexicalPolicy::MethodRegionsWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const HEADER_FORMS: ProfileSpec = ProfileSpec {
    name: "linux-v5-header-forms1-xml221-r09",
    features: Features {
        header_forms: Feature::On,
        ..METHOD_REGIONS.features
    },
    lexical: LexicalPolicy::HeaderFormsWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const SOURCE_LIMITS: ProfileSpec = ProfileSpec {
    name: "linux-v5-source-limits1-xml221-r09",
    features: Features {
        source_limits: Feature::On,
        ..HEADER_FORMS.features
    },
    lexical: LexicalPolicy::SourceLimitsWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const LONG_HEADERS: ProfileSpec = ProfileSpec {
    name: "linux-v5-long-headers1-xml221-r09",
    features: Features {
        long_headers: Feature::On,
        ..SOURCE_LIMITS.features
    },
    lexical: LexicalPolicy::LongHeadersWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const SOURCE_ANOMALIES: ProfileSpec = ProfileSpec {
    name: "linux-v5-source-anomalies1-xml221-r09",
    features: Features {
        source_anomalies: Feature::On,
        ..SOURCE_LIMITS.features
    },
    lexical: LexicalPolicy::SourceAnomaliesWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const LEXICAL_TAIL: ProfileSpec = ProfileSpec {
    name: "linux-v5-lexical-tail1-xml221-r09",
    features: Features {
        lexical_tail: Feature::On,
        ..LONG_HEADERS.features
    },
    lexical: LexicalPolicy::LexicalTailWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const NAVIGATOR_AGREEMENT: ProfileSpec = ProfileSpec {
    name: "linux-v5-navigator-agreement1-xml221-r09",
    features: Features {
        source_anomalies: Feature::On,
        ..LEXICAL_TAIL.features
    },
    lexical: LexicalPolicy::NavigatorAgreementWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const NAVIGATOR_OWNERS: ProfileSpec = ProfileSpec {
    name: "linux-v5-navigator-owners1-xml221-r09",
    features: Features {
        command_owners: Feature::On,
        ..NAVIGATOR_AGREEMENT.features
    },
    lexical: LexicalPolicy::NavigatorAgreementWriter,
    omit_command_help: Feature::Off,
    processor_help: ProcessorHelpPolicy::Disabled,
};
pub(super) const NAVIGATOR_SPPR: ProfileSpec = ProfileSpec {
    name: "linux-v5-navigator-sppr1-xml221-r09",
    features: Features {
        large_templates: Feature::On,
        ..NAVIGATOR_OWNERS.features
    },
    lexical: LexicalPolicy::NavigatorAgreementWriter,
    omit_command_help: Feature::On,
    processor_help: ProcessorHelpPolicy::Sppr,
};
#[allow(dead_code)]
pub(super) const ALL: &[&ProfileSpec] = &[
    &ANNOTATED_VARS,
    &ASYNC_CASING,
    &CHART,
    &COMMON,
    &COMPANION,
    &CONDITIONAL,
    &CONTEXT,
    &CORPUS_BIN,
    &CORPUS_CONFIG,
    &CORPUS_HELP,
    &CORPUS,
    &DATES_MEMBERS,
    &DIAGNOSTICS,
    &HEADER_FORMS,
    &JOURNAL,
    &LEAF,
    &LEXICAL_TAIL,
    &LITERAL,
    &LONG_HEADERS,
    &METHOD_REGIONS,
    &MOBILE_VARS,
    &NAVIGATOR_AGREEMENT,
    &NAVIGATOR_OWNERS,
    &NAVIGATOR_SPPR,
    &OPTIONAL_FORM,
    &PARAMETER,
    &REFINED,
    &REGISTER,
    &REPORT,
    &ROOT,
    &SERVICE,
    &SHARED,
    &SIDECAR,
    &SOURCE_ANOMALIES,
    &SOURCE_LIMITS,
    &STRING,
    &WORKFLOW,
    &WRAPPED,
];
#[allow(dead_code)]
pub(super) fn by_name(name: &str) -> Option<&'static ProfileSpec> {
    ALL.iter().copied().find(|p| p.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn baseline_equivalence() {
        let rows = include_str!("profile_baseline.tsv").lines();
        let mut count = 0;
        for row in rows {
            let cols: Vec<_> = row.split('\t').collect();
            let p = by_name(cols[0]).expect("missing profile");
            let expected: Vec<_> = cols[2].split(", ").skip(1).collect();
            let actual = [
                p.features.common.enabled(),
                p.features.roots.enabled(),
                p.features.shared.enabled(),
                p.features.leaves.enabled(),
                p.features.journals.enabled(),
                p.features.services.enabled(),
                p.features.charts.enabled(),
                p.features.registers.enabled(),
                p.features.reports.enabled(),
                p.features.workflows.enabled(),
                p.features.corpus.enabled(),
                p.features.bin_profile.enabled(),
                p.features.root_help.enabled(),
                p.features.configuration.enabled(),
                p.features.diagnostics.enabled(),
                p.features.optional_form.enabled(),
                p.features.root_sidecars.enabled(),
                p.features.context.enabled(),
                p.features.headers.enabled(),
                p.features.conditional.enabled(),
                p.features.strings.enabled(),
                p.features.literals.enabled(),
                p.features.wrapped.enabled(),
                p.features.refined.enabled(),
                p.features.annotated_vars.enabled(),
                p.features.dates_members.enabled(),
                p.features.async_casing.enabled(),
                p.features.mobile_vars.enabled(),
                p.features.method_regions.enabled(),
                p.features.header_forms.enabled(),
                p.features.source_limits.enabled(),
                p.features.long_headers.enabled(),
                p.features.lexical_tail.enabled(),
                p.features.source_anomalies.enabled(),
                p.features.command_owners.enabled(),
                p.features.large_templates.enabled(),
            ];
            assert_eq!(actual.len(), expected.len());
            for (a, e) in actual.iter().zip(expected) {
                assert_eq!(*a, e == "true", "{}", cols[0]);
            }
            assert_eq!(p.lexical.value(), cols[5].split_once('=').unwrap().1);
            let b: Vec<_> = cols[4].split(", ").collect();
            let build_flags = [
                (7, p.features.common.enabled()),
                (8, p.features.roots.enabled()),
                (9, p.features.shared.enabled()),
                (10, p.features.leaves.enabled()),
                (11, p.features.journals.enabled()),
                (12, p.features.services.enabled()),
                (13, p.features.charts.enabled()),
                (14, p.features.registers.enabled()),
                (15, p.features.reports.enabled()),
                (16, p.features.workflows.enabled()),
                (17, p.features.corpus.enabled()),
                (20, p.features.root_help.enabled()),
                (21, p.features.configuration.enabled()),
                (23, p.features.optional_form.enabled()),
                (24, p.features.root_sidecars.enabled()),
                (25, p.features.context.enabled()),
                (26, p.features.headers.enabled()),
                (27, p.features.conditional.enabled()),
                (28, p.features.strings.enabled()),
                (29, p.features.literals.enabled()),
                (30, p.features.wrapped.enabled()),
                (31, p.features.refined.enabled()),
                (32, p.features.annotated_vars.enabled()),
                (33, p.features.dates_members.enabled()),
                (34, p.features.async_casing.enabled()),
                (35, p.features.mobile_vars.enabled()),
                (36, p.features.method_regions.enabled()),
                (37, p.features.header_forms.enabled()),
                (38, p.features.source_limits.enabled()),
                (39, p.features.long_headers.enabled()),
                (40, p.features.lexical_tail.enabled()),
                (41, p.features.source_anomalies.enabled()),
                (42, p.features.command_owners.enabled()),
                (43, p.features.large_templates.enabled()),
            ];
            for (i, a) in build_flags {
                assert_eq!(a, b[i] == "true", "{} at {}", cols[0], i);
            }
            assert_eq!(
                p.features.bin_profile.enabled(),
                b[19] != "None",
                "{}",
                cols[0]
            );
            assert_eq!(
                p.features.diagnostics.enabled(),
                b[22] != "None",
                "{}",
                cols[0]
            );
            assert_eq!(
                p.omit_command_help.enabled(),
                b[44] == "true",
                "{}",
                cols[0]
            );
            assert_eq!(
                p.processor_help == ProcessorHelpPolicy::Sppr,
                b[45] == "Some(processor_help)",
                "{}",
                cols[0]
            );
            count += 1;
        }
        assert_eq!(count, ALL.len());
    }
}
