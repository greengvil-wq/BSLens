//! Bounded structural subset, not a validated Git snapshot or platform model.
use quick_xml::{events::Event, Reader};
use roxmltree::{Document, Node, ParsingOptions};
use std::collections::{BTreeMap, BTreeSet};

pub const NS: &str = "http://v8.1c.ru/8.3/MDClasses";
pub const MAX_FILES: usize = 4096;
pub const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_XML_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_DEPTH: usize = 64;
pub const MAX_EVENTS: usize = 20000;
pub const MAX_NODES: u32 = 10000;
pub const MAX_CONFIGURATION_NODES: u32 = 16384;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    Limit,
    Path,
    Utf8,
    Encoding,
    Dtd,
    ProcessingInstruction,
    Malformed,
    Namespace,
    Version,
    Shape,
    Identifier,
    Uuid,
    Duplicate,
    Missing,
    UnsupportedType,
    NestedUnsupported,
    NameMismatch,
    OrphanXml,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlError {
    pub code: Code,
    pub path: String,
}
impl std::fmt::Display for XmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.path)
    }
}
impl std::error::Error for XmlError {}
type Result<T> = std::result::Result<T, XmlError>;
fn error(code: Code, path: &str) -> XmlError {
    XmlError {
        code,
        path: path.into(),
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Descriptor(DescriptorKind),
    Configuration,
    Catalog,
    Document,
    InformationRegister,
    CommonModule,
    CommonCommand,
    CommonForm,
    CommonTemplate,
    CommonPicture,
    Constant,
    Role,
    ScheduledJob,
    XDTOPackage,
    Enum,
    DocumentJournal,
    HTTPService,
    WebService,
    IntegrationService,
    Sequence,
    SettingsStorage,
    Subsystem,
    ExchangePlan,
    BusinessProcess,
    Task,
    DataProcessor,
    Report,
    AccountingRegister,
    AccumulationRegister,
    CalculationRegister,
    Recalculation,
    ChartOfAccounts,
    ChartOfCalculationTypes,
    ChartOfCharacteristicTypes,

    Form,
    Template,
}
impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Descriptor(_) => {
                ROOT_LOCATORS
                    .iter()
                    .find(|r| r.kind == self)
                    .unwrap()
                    .xml_type
            }
            Self::CommonCommand => "CommonCommand",
            Self::CommonForm => "CommonForm",
            Self::CommonTemplate => "CommonTemplate",
            Self::CommonPicture => "CommonPicture",
            Self::Constant => "Constant",
            Self::Role => "Role",
            Self::ScheduledJob => "ScheduledJob",
            Self::XDTOPackage => "XDTOPackage",
            Self::Enum => "Enum",
            Self::DocumentJournal => "DocumentJournal",
            Self::HTTPService => "HTTPService",
            Self::WebService => "WebService",
            Self::IntegrationService => "IntegrationService",
            Self::Sequence => "Sequence",
            Self::SettingsStorage => "SettingsStorage",
            Self::Subsystem => "Subsystem",
            Self::ExchangePlan => "ExchangePlan",
            Self::BusinessProcess => "BusinessProcess",
            Self::Task => "Task",
            Self::DataProcessor => "DataProcessor",
            Self::Report => "Report",
            Self::AccountingRegister => "AccountingRegister",
            Self::AccumulationRegister => "AccumulationRegister",
            Self::CalculationRegister => "CalculationRegister",
            Self::Recalculation => "Recalculation",
            Self::ChartOfAccounts => "ChartOfAccounts",
            Self::ChartOfCalculationTypes => "ChartOfCalculationTypes",
            Self::ChartOfCharacteristicTypes => "ChartOfCharacteristicTypes",

            Self::CommonModule => "CommonModule",
            Self::Form => "Form",
            Self::Template => "Template",
            Self::Configuration => "Configuration",
            Self::Catalog => "Catalog",
            Self::Document => "Document",
            Self::InformationRegister => "InformationRegister",
        }
    }
    pub(crate) fn folder(self) -> &'static str {
        match self {
            Self::Descriptor(_) => {
                ROOT_LOCATORS
                    .iter()
                    .find(|r| r.kind == self)
                    .unwrap()
                    .directory
            }
            Self::Catalog => "Catalogs",
            Self::Document => "Documents",
            Self::InformationRegister => "InformationRegisters",
            Self::CommonCommand => "CommonCommands",
            Self::CommonForm => "CommonForms",
            Self::CommonTemplate => "CommonTemplates",
            Self::CommonPicture => "CommonPictures",
            Self::Constant => "Constants",
            Self::Role => "Roles",
            Self::ScheduledJob => "ScheduledJobs",
            Self::XDTOPackage => "XDTOPackages",
            Self::Enum => "Enums",
            Self::DocumentJournal => "DocumentJournals",
            Self::HTTPService => "HTTPServices",
            Self::WebService => "WebServices",
            Self::IntegrationService => "IntegrationServices",
            Self::Sequence => "Sequences",
            Self::SettingsStorage => "SettingsStorages",
            Self::Subsystem => "Subsystems",
            Self::ExchangePlan => "ExchangePlans",
            Self::BusinessProcess => "BusinessProcesses",
            Self::Task => "Tasks",
            Self::DataProcessor => "DataProcessors",
            Self::Report => "Reports",
            Self::AccountingRegister => "AccountingRegisters",
            Self::AccumulationRegister => "AccumulationRegisters",
            Self::CalculationRegister => "CalculationRegisters",
            Self::Recalculation => "Recalculations",
            Self::ChartOfAccounts => "ChartsOfAccounts",
            Self::ChartOfCalculationTypes => "ChartsOfCalculationTypes",
            Self::ChartOfCharacteristicTypes => "ChartsOfCharacteristicTypes",

            Self::CommonModule => "CommonModules",
            Self::Form => "Forms",
            Self::Template => "Templates",
            Self::Configuration => "",
        }
    }
    pub(crate) fn modules(self) -> &'static [&'static str] {
        match self {
            Self::CommonCommand => &["CommandModule"],
            Self::Sequence | Self::Subsystem => &[],
            Self::SettingsStorage => &["ManagerModule"],
            Self::ExchangePlan | Self::BusinessProcess | Self::Task => {
                &["ManagerModule", "ObjectModule"]
            }
            Self::DataProcessor | Self::Report => &["ManagerModule", "ObjectModule"],
            Self::AccountingRegister => &["RecordSetModule"],
            Self::AccumulationRegister => &["ManagerModule", "RecordSetModule"],
            Self::CalculationRegister | Self::Recalculation => &[],
            Self::ChartOfAccounts
            | Self::ChartOfCalculationTypes
            | Self::ChartOfCharacteristicTypes => &["ManagerModule", "ObjectModule"],
            Self::Constant => &["ManagerModule", "ValueManagerModule"],
            Self::Enum | Self::DocumentJournal => &["ManagerModule"],
            Self::CommonPicture | Self::Role | Self::ScheduledJob | Self::XDTOPackage => &[],
            Self::CommonModule
            | Self::HTTPService
            | Self::WebService
            | Self::IntegrationService => &["Module"],
            Self::Form | Self::CommonForm => &["Form/Module"],
            Self::Template | Self::CommonTemplate | Self::Descriptor(_) => &[],
            Self::Configuration => &[
                "ManagedApplicationModule",
                "OrdinaryApplicationModule",
                "ExternalConnectionModule",
                "SessionModule",
            ],
            Self::Catalog | Self::Document => &["ObjectModule", "ManagerModule"],
            Self::InformationRegister => &["RecordSetModule", "ManagerModule"],
        }
    }
}
/// Exact allowlist. No English pluralization or fallback filesystem probing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DescriptorKind {
    CommandGroup,
    CommonAttribute,
    DefinedType,
    EventSubscription,
    FilterCriterion,
    FunctionalOption,
    FunctionalOptionsParameter,
    Language,
    PaletteColor,
    SessionParameter,
    StyleItem,
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct RootLocator {
    pub kind: Kind,
    pub xml_type: &'static str,
    pub directory: &'static str,
}
pub(crate) const ROOT_LOCATORS: &[RootLocator] = &[
    RootLocator {
        kind: Kind::Catalog,
        xml_type: "Catalog",
        directory: "Catalogs",
    },
    RootLocator {
        kind: Kind::Document,
        xml_type: "Document",
        directory: "Documents",
    },
    RootLocator {
        kind: Kind::InformationRegister,
        xml_type: "InformationRegister",
        directory: "InformationRegisters",
    },
    RootLocator {
        kind: Kind::CommonModule,
        xml_type: "CommonModule",
        directory: "CommonModules",
    },
    RootLocator {
        kind: Kind::Descriptor(DescriptorKind::CommandGroup),
        xml_type: "CommandGroup",
        directory: "CommandGroups",
    },
    RootLocator {
        kind: Kind::Descriptor(DescriptorKind::CommonAttribute),
        xml_type: "CommonAttribute",
        directory: "CommonAttributes",
    },
    RootLocator {
        kind: Kind::Descriptor(DescriptorKind::DefinedType),
        xml_type: "DefinedType",
        directory: "DefinedTypes",
    },
    RootLocator {
        kind: Kind::Descriptor(DescriptorKind::EventSubscription),
        xml_type: "EventSubscription",
        directory: "EventSubscriptions",
    },
    RootLocator {
        kind: Kind::Descriptor(DescriptorKind::FilterCriterion),
        xml_type: "FilterCriterion",
        directory: "FilterCriteria",
    },
    RootLocator {
        kind: Kind::Descriptor(DescriptorKind::FunctionalOption),
        xml_type: "FunctionalOption",
        directory: "FunctionalOptions",
    },
    RootLocator {
        kind: Kind::Descriptor(DescriptorKind::FunctionalOptionsParameter),
        xml_type: "FunctionalOptionsParameter",
        directory: "FunctionalOptionsParameters",
    },
    RootLocator {
        kind: Kind::Descriptor(DescriptorKind::Language),
        xml_type: "Language",
        directory: "Languages",
    },
    RootLocator {
        kind: Kind::Descriptor(DescriptorKind::PaletteColor),
        xml_type: "PaletteColor",
        directory: "PaletteColors",
    },
    RootLocator {
        kind: Kind::Descriptor(DescriptorKind::SessionParameter),
        xml_type: "SessionParameter",
        directory: "SessionParameters",
    },
    RootLocator {
        kind: Kind::Descriptor(DescriptorKind::StyleItem),
        xml_type: "StyleItem",
        directory: "StyleItems",
    },
];
impl RootLocator {
    pub(crate) fn path(self, name: &str) -> Result<String> {
        if !identifier(name) {
            return Err(error(Code::Identifier, "Configuration.xml"));
        }
        let path = format!("{}/{}.xml", self.directory, name);
        path_check(&path)?;
        if path.len() > 1024 {
            return Err(error(Code::Limit, "Configuration.xml"));
        }
        Ok(path)
    }
}
pub(crate) fn root_locator(xml_type: &str) -> Option<RootLocator> {
    ROOT_LOCATORS
        .iter()
        .find(|r| r.xml_type == xml_type)
        .copied()
}
pub(crate) fn root_locator_with_corpus(
    name: &str,
    shared: bool,
    leaves: bool,
    journals: bool,
    services: bool,
    charts: bool,
    registers: bool,
    reports: bool,
    workflows: bool,
    corpus: bool,
) -> Option<RootLocator> {
    root_locator_with_workflows(
        name, shared, leaves, journals, services, charts, registers, reports, workflows,
    )
    .or_else(|| {
        if !corpus {
            return None;
        }
        match name {
            "Sequence" => Some(RootLocator {
                kind: Kind::Sequence,
                xml_type: "Sequence",
                directory: "Sequences",
            }),
            "SettingsStorage" => Some(RootLocator {
                kind: Kind::SettingsStorage,
                xml_type: "SettingsStorage",
                directory: "SettingsStorages",
            }),
            "Subsystem" => Some(RootLocator {
                kind: Kind::Subsystem,
                xml_type: "Subsystem",
                directory: "Subsystems",
            }),
            _ => None,
        }
    })
}
pub(crate) fn root_locator_with_workflows(
    name: &str,
    shared: bool,
    leaves: bool,
    journals: bool,
    services: bool,
    charts: bool,
    registers: bool,
    reports: bool,
    workflows: bool,
) -> Option<RootLocator> {
    root_locator_with_reports(
        name, shared, leaves, journals, services, charts, registers, reports,
    )
    .or_else(|| {
        if !workflows {
            return None;
        }
        match name {
            "ExchangePlan" => Some(RootLocator {
                kind: Kind::ExchangePlan,
                xml_type: "ExchangePlan",
                directory: "ExchangePlans",
            }),
            "BusinessProcess" => Some(RootLocator {
                kind: Kind::BusinessProcess,
                xml_type: "BusinessProcess",
                directory: "BusinessProcesses",
            }),
            "Task" => Some(RootLocator {
                kind: Kind::Task,
                xml_type: "Task",
                directory: "Tasks",
            }),
            _ => None,
        }
    })
}
pub(crate) fn root_locator_with_reports(
    name: &str,
    shared: bool,
    leaves: bool,
    journals: bool,
    services: bool,
    charts: bool,
    registers: bool,
    reports: bool,
) -> Option<RootLocator> {
    root_locator_with_registers(name, shared, leaves, journals, services, charts, registers)
        .or_else(|| {
            if !reports {
                return None;
            }
            match name {
                "DataProcessor" => Some(RootLocator {
                    kind: Kind::DataProcessor,
                    xml_type: "DataProcessor",
                    directory: "DataProcessors",
                }),
                "Report" => Some(RootLocator {
                    kind: Kind::Report,
                    xml_type: "Report",
                    directory: "Reports",
                }),
                _ => None,
            }
        })
}
pub(crate) fn root_locator_with_registers(
    name: &str,
    shared: bool,
    leaves: bool,
    journals: bool,
    services: bool,
    charts: bool,
    registers: bool,
) -> Option<RootLocator> {
    root_locator_with_charts(name, shared, leaves, journals, services, charts).or_else(|| {
        if !registers {
            return None;
        }
        match name {
            "AccountingRegister" => Some(RootLocator {
                kind: Kind::AccountingRegister,
                xml_type: "AccountingRegister",
                directory: "AccountingRegisters",
            }),
            "AccumulationRegister" => Some(RootLocator {
                kind: Kind::AccumulationRegister,
                xml_type: "AccumulationRegister",
                directory: "AccumulationRegisters",
            }),
            "CalculationRegister" => Some(RootLocator {
                kind: Kind::CalculationRegister,
                xml_type: "CalculationRegister",
                directory: "CalculationRegisters",
            }),
            _ => None,
        }
    })
}
pub(crate) fn root_locator_with_charts(
    name: &str,
    shared: bool,
    leaves: bool,
    journals: bool,
    services: bool,
    charts: bool,
) -> Option<RootLocator> {
    root_locator_with_services(name, shared, leaves, journals, services).or_else(|| {
        if charts {
            match name {
                "ChartOfAccounts" => Some(RootLocator {
                    kind: Kind::ChartOfAccounts,
                    xml_type: "ChartOfAccounts",
                    directory: "ChartsOfAccounts",
                }),
                "ChartOfCalculationTypes" => Some(RootLocator {
                    kind: Kind::ChartOfCalculationTypes,
                    xml_type: "ChartOfCalculationTypes",
                    directory: "ChartsOfCalculationTypes",
                }),
                "ChartOfCharacteristicTypes" => Some(RootLocator {
                    kind: Kind::ChartOfCharacteristicTypes,
                    xml_type: "ChartOfCharacteristicTypes",
                    directory: "ChartsOfCharacteristicTypes",
                }),
                _ => None,
            }
        } else {
            None
        }
    })
}
pub(crate) fn root_locator_with_services(
    name: &str,
    shared: bool,
    leaves: bool,
    journals: bool,
    services: bool,
) -> Option<RootLocator> {
    root_locator_with_journals(name, shared, leaves, journals).or_else(|| {
        if services {
            match name {
                "HTTPService" => Some(RootLocator {
                    kind: Kind::HTTPService,
                    xml_type: "HTTPService",
                    directory: "HTTPServices",
                }),
                "WebService" => Some(RootLocator {
                    kind: Kind::WebService,
                    xml_type: "WebService",
                    directory: "WebServices",
                }),
                "IntegrationService" => Some(RootLocator {
                    kind: Kind::IntegrationService,
                    xml_type: "IntegrationService",
                    directory: "IntegrationServices",
                }),
                _ => None,
            }
        } else {
            None
        }
    })
}
pub(crate) fn root_locator_with_journals(
    name: &str,
    shared: bool,
    leaves: bool,
    journals: bool,
) -> Option<RootLocator> {
    root_locator_with_leaves(name, shared, leaves).or_else(|| {
        if journals {
            match name {
                "Enum" => Some(RootLocator {
                    kind: Kind::Enum,
                    xml_type: "Enum",
                    directory: "Enums",
                }),
                "DocumentJournal" => Some(RootLocator {
                    kind: Kind::DocumentJournal,
                    xml_type: "DocumentJournal",
                    directory: "DocumentJournals",
                }),
                _ => None,
            }
        } else {
            None
        }
    })
}
pub(crate) fn root_locator_with_leaves(
    name: &str,
    shared: bool,
    leaves: bool,
) -> Option<RootLocator> {
    root_locator_with_shared(name, shared).or_else(|| {
        if leaves {
            match name {
                "CommonPicture" => Some(RootLocator {
                    kind: Kind::CommonPicture,
                    xml_type: "CommonPicture",
                    directory: "CommonPictures",
                }),
                "Constant" => Some(RootLocator {
                    kind: Kind::Constant,
                    xml_type: "Constant",
                    directory: "Constants",
                }),
                "Role" => Some(RootLocator {
                    kind: Kind::Role,
                    xml_type: "Role",
                    directory: "Roles",
                }),
                "ScheduledJob" => Some(RootLocator {
                    kind: Kind::ScheduledJob,
                    xml_type: "ScheduledJob",
                    directory: "ScheduledJobs",
                }),
                "XDTOPackage" => Some(RootLocator {
                    kind: Kind::XDTOPackage,
                    xml_type: "XDTOPackage",
                    directory: "XDTOPackages",
                }),
                _ => None,
            }
        } else {
            None
        }
    })
}
pub(crate) fn root_locator_with_shared(name: &str, shared: bool) -> Option<RootLocator> {
    root_locator(name).or_else(|| {
        if shared {
            match name {
                "CommonCommand" => Some(RootLocator {
                    kind: Kind::CommonCommand,
                    xml_type: "CommonCommand",
                    directory: "CommonCommands",
                }),
                "CommonForm" => Some(RootLocator {
                    kind: Kind::CommonForm,
                    xml_type: "CommonForm",
                    directory: "CommonForms",
                }),
                "CommonTemplate" => Some(RootLocator {
                    kind: Kind::CommonTemplate,
                    xml_type: "CommonTemplate",
                    directory: "CommonTemplates",
                }),
                _ => None,
            }
        } else {
            None
        }
    })
}
pub(crate) fn descriptor_only_path(path: &str) -> Result<()> {
    let mut parts = path.split('/');
    let folder = parts.next().unwrap_or("");
    if ROOT_LOCATORS
        .iter()
        .any(|r| matches!(r.kind, Kind::Descriptor(_)) && r.directory == folder)
    {
        let name = parts.next().ok_or_else(|| error(Code::Path, path))?;
        if parts.next().is_some() || !name.ends_with(".xml") {
            return Err(error(Code::NestedUnsupported, path));
        }
        if !identifier(name.strip_suffix(".xml").unwrap()) {
            return Err(error(Code::Identifier, path));
        }
    }
    Ok(())
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub kind: String,
    pub path: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    pub kind: Kind,
    pub name: String,
    pub uuid: Option<String>,
    pub path: String,
    pub modules: Vec<Module>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unverified {
    pub path: String,
    pub selector: String,
}
#[derive(Debug)]
pub struct Xml221Subset {
    objects: Vec<Object>,
    unverified: Vec<Unverified>,
    unassigned: Vec<String>,
}
impl Xml221Subset {
    pub fn objects(&self) -> &[Object] {
        &self.objects
    }
    pub fn unverified(&self) -> &[Unverified] {
        &self.unverified
    }
    pub fn unassigned_modules(&self) -> &[String] {
        &self.unassigned
    }
    pub fn inspect(blobs: &BTreeMap<String, Vec<u8>>) -> Result<Self> {
        if blobs.len() > MAX_FILES
            || blobs
                .values()
                .try_fold(0usize, |n, b| n.checked_add(b.len()))
                .is_none_or(|n| n > MAX_TOTAL_BYTES)
        {
            return Err(error(Code::Limit, ""));
        }
        for path in blobs.keys() {
            path_check(path)?;
        }
        if !blobs.contains_key("Configuration.xml") {
            return Err(error(Code::Missing, "Configuration.xml"));
        }
        let mut docs = BTreeMap::new();
        for (path, raw) in blobs {
            if path.ends_with(".xml") {
                docs.insert(path.as_str(), parse(raw, path)?);
            } else if !path.ends_with(".bsl") {
                return Err(error(Code::Path, path));
            }
        }
        let mut state = State {
            blobs,
            docs: &docs,
            seen: BTreeSet::new(),
            uuids: BTreeSet::new(),
            objects: vec![],
            unverified: vec![],
            assigned: BTreeSet::new(),
            inline: false,
            declarations: vec![],
            shared: false,
            leaves: false,
            journals: false,
            services: false,
            charts: false,
            registers: false,
            reports: false,
            workflows: false,
            corpus: false,
            root_ranges: BTreeMap::new(),
            roots: false,
            common: false,
            companions: false,
            companion_refs: vec![],
        };
        state.read("Configuration.xml", Kind::Configuration, None)?;
        for path in docs.keys() {
            if !state.seen.contains(*path) {
                return Err(error(Code::OrphanXml, path));
            }
        }
        state.objects.sort_by(|a, b| a.path.cmp(&b.path));
        state
            .unverified
            .sort_by(|a, b| (&a.path, &a.selector).cmp(&(&b.path, &b.selector)));
        let unassigned = blobs
            .keys()
            .filter(|p| p.ends_with(".bsl") && !state.assigned.contains(*p))
            .cloned()
            .collect();
        Ok(Self {
            objects: state.objects,
            unverified: state.unverified,
            unassigned,
        })
    }
}
pub(crate) fn path_check(path: &str) -> Result<()> {
    if path.len() > 1024 {
        return Err(error(Code::Limit, path));
    }
    if path.is_empty()
        || path.contains(['\\', ':'])
        || path.chars().any(|c| c < ' ')
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(error(Code::Path, path));
    }
    Ok(())
}
fn parse<'a>(raw: &'a [u8], path: &str) -> Result<Document<'a>> {
    parse_mode(raw, path, false)
}
fn parse_mode<'a>(raw: &'a [u8], path: &str, configuration: bool) -> Result<Document<'a>> {
    if raw.len() > MAX_XML_BYTES {
        return Err(error(Code::Limit, path));
    }
    let text = std::str::from_utf8(raw).map_err(|_| error(Code::Utf8, path))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.starts_with('\u{feff}') {
        return Err(error(Code::Malformed, path));
    }
    let mut reader = Reader::from_str(text);
    let mut depth = 0usize;
    let mut count = 0usize;
    loop {
        let event = reader
            .read_event()
            .map_err(|_| error(Code::Malformed, path))?;
        count += 1;
        if count > MAX_EVENTS {
            return Err(error(Code::Limit, path));
        }
        match event {
            Event::DocType(_) => return Err(error(Code::Dtd, path)),
            Event::PI(_) => return Err(error(Code::ProcessingInstruction, path)),
            Event::Decl(d) => {
                if let Some(encoding) = d.encoding() {
                    let encoding = encoding.map_err(|_| error(Code::Malformed, path))?;
                    if !encoding.eq_ignore_ascii_case(b"utf-8") {
                        return Err(error(Code::Encoding, path));
                    }
                }
            }
            Event::Start(_) => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(error(Code::Limit, path));
                }
            }
            Event::Empty(_) => {
                if depth + 1 > MAX_DEPTH {
                    return Err(error(Code::Limit, path));
                }
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| error(Code::Malformed, path))?;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: if configuration && path == "Configuration.xml" {
                MAX_CONFIGURATION_NODES
            } else {
                MAX_NODES
            },
        },
    )
    .map_err(|e| {
        error(
            if matches!(e, roxmltree::Error::NodesLimitReached) {
                Code::Limit
            } else {
                Code::Malformed
            },
            path,
        )
    })
}
fn matches(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().namespace() == Some(NS) && node.tag_name().name() == name
}
fn elements<'a, 'i>(n: Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
    n.children().filter(|c| c.is_element()).collect()
}
fn no_text(n: Node<'_, '_>) -> bool {
    n.children()
        .filter(|c| c.is_text())
        .all(|c| c.text().unwrap_or("").trim().is_empty())
}
fn scalar(n: Node<'_, '_>) -> String {
    n.children()
        .filter(|c| c.is_text())
        .map(|c| c.text().unwrap_or(""))
        .collect()
}
fn identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c == '_' || c.is_alphabetic())
        && chars.all(|c| c == '_' || c.is_alphanumeric())
}
fn uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}
struct State<'a, 'b> {
    blobs: &'a BTreeMap<String, Vec<u8>>,
    docs: &'b BTreeMap<&'a str, Document<'a>>,
    seen: BTreeSet<String>,
    uuids: BTreeSet<String>,
    objects: Vec<Object>,
    unverified: Vec<Unverified>,
    assigned: BTreeSet<String>,
    inline: bool,
    declarations: Vec<Declaration>,
    companions: bool,
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
    root_ranges: BTreeMap<String, (usize, usize)>,
    companion_refs: Vec<CompanionRef>,
}
impl State<'_, '_> {
    fn read(&mut self, path: &str, kind: Kind, expected: Option<&str>) -> Result<()> {
        if !self.seen.insert(path.into()) {
            return Err(error(Code::Duplicate, path));
        }
        let doc = self
            .docs
            .get(path)
            .ok_or_else(|| error(Code::Missing, path))?;
        self.read_document(doc, path, kind, expected, false)?;
        Ok(())
    }
    fn read_document(
        &mut self,
        doc: &Document<'_>,
        path: &str,
        kind: Kind,
        expected: Option<&str>,
        collect_references: bool,
    ) -> Result<Vec<(String, Kind, String)>> {
        let mut references = Vec::new();
        let root = doc.root_element();
        if !matches(root, "MetaDataObject") {
            return Err(error(Code::Namespace, path));
        }
        if root.attribute("version") != Some("2.21") {
            return Err(error(Code::Version, path));
        }
        let children = elements(root);
        if children.len() != 1 || !matches(children[0], kind.name()) || !no_text(root) {
            return Err(error(Code::Shape, path));
        }
        let obj = children[0];
        if !no_text(obj) {
            return Err(error(Code::Shape, path));
        }
        let props: Vec<_> = elements(obj)
            .into_iter()
            .filter(|n| matches(*n, "Properties"))
            .collect();
        if props.len() != 1 {
            return Err(error(Code::Shape, path));
        }
        if !no_text(props[0]) {
            return Err(error(Code::Shape, path));
        }
        for attr in props[0].attributes() {
            self.unverified.push(Unverified {
                path: path.into(),
                selector: format!(
                    "/MetaDataObject/{}/Properties/@{{{}}}{}",
                    kind.name(),
                    attr.namespace().unwrap_or(""),
                    attr.name()
                ),
            });
        }
        let names: Vec<_> = elements(props[0])
            .into_iter()
            .filter(|n| matches(*n, "Name"))
            .collect();
        if names.len() != 1 || !elements(names[0]).is_empty() || names[0].attributes().len() != 0 {
            return Err(error(Code::Shape, path));
        }
        let name = scalar(names[0]);
        if !identifier(&name) {
            return Err(error(Code::Identifier, path));
        }
        if expected.is_some_and(|e| e != name) {
            return Err(error(Code::NameMismatch, path));
        }
        let uid = obj.attribute("uuid");
        if matches!(
            kind,
            Kind::Form
                | Kind::Template
                | Kind::CommonModule
                | Kind::CommonCommand
                | Kind::CommonForm
                | Kind::CommonTemplate
                | Kind::CommonPicture
                | Kind::Constant
                | Kind::Role
                | Kind::ScheduledJob
                | Kind::XDTOPackage
                | Kind::Descriptor(_)
                | Kind::Enum
                | Kind::DocumentJournal
                | Kind::HTTPService
                | Kind::WebService
                | Kind::IntegrationService
                | Kind::ChartOfAccounts
                | Kind::ChartOfCalculationTypes
                | Kind::ChartOfCharacteristicTypes
                | Kind::AccountingRegister
                | Kind::AccumulationRegister
                | Kind::CalculationRegister
                | Kind::Recalculation
                | Kind::DataProcessor
                | Kind::Report
                | Kind::ExchangePlan
                | Kind::BusinessProcess
                | Kind::Task
                | Kind::Sequence
                | Kind::SettingsStorage
                | Kind::Subsystem
        ) && (uid.is_none() || obj.attributes().len() != 1)
        {
            return Err(error(Code::Uuid, path));
        }
        if let Some(uid) = uid {
            if !uuid(uid) {
                return Err(error(Code::Uuid, path));
            }
            if !self.uuids.insert(uid.to_lowercase()) {
                return Err(error(Code::Duplicate, path));
            }
        }
        let base = if kind == Kind::Configuration {
            "Ext/".to_string()
        } else {
            format!("{}/Ext/", path.strip_suffix(".xml").unwrap())
        };
        let modules = kind
            .modules()
            .iter()
            .filter_map(|m| {
                let p = format!("{base}{m}.bsl");
                if self.blobs.contains_key(&p) {
                    self.assigned.insert(p.clone());
                    Some(Module {
                        kind: (*m).into(),
                        path: p,
                    })
                } else {
                    None
                }
            })
            .collect();
        self.objects.push(Object {
            kind,
            name: name.clone(),
            uuid: uid.map(str::to_string),
            path: path.into(),
            modules,
        });
        let selector = format!("/MetaDataObject/{}", kind.name());
        for (node, allowed, prefix) in [
            (root, "version", "/MetaDataObject"),
            (obj, "uuid", selector.as_str()),
        ] {
            for attr in node.attributes() {
                if attr.name() != allowed || attr.namespace().is_some() {
                    self.unverified.push(Unverified {
                        path: path.into(),
                        selector: format!(
                            "{prefix}/@{{{}}}{}",
                            attr.namespace().unwrap_or(""),
                            attr.name()
                        ),
                    });
                }
            }
        }
        for prop in elements(props[0]) {
            if !matches(prop, "Name") {
                self.unverified.push(Unverified {
                    path: path.into(),
                    selector: format!(
                        "{selector}/Properties/{{{}}}{}",
                        prop.tag_name().namespace().unwrap_or(""),
                        prop.tag_name().name()
                    ),
                });
            }
        }
        let empty_filter = self.corpus && kind == Kind::Descriptor(DescriptorKind::FilterCriterion);
        if matches!(
            kind,
            Kind::Descriptor(_)
                | Kind::CommonCommand
                | Kind::CommonForm
                | Kind::CommonTemplate
                | Kind::CommonPicture
                | Kind::Constant
                | Kind::Role
                | Kind::ScheduledJob
                | Kind::XDTOPackage
        ) {
            for child in elements(obj) {
                if !matches(child, "Properties")
                    && !matches(child, "InternalInfo")
                    && !(empty_filter && matches(child, "ChildObjects"))
                {
                    return Err(error(Code::NestedUnsupported, path));
                }
            }
        }
        if matches!(
            kind,
            Kind::Enum
                | Kind::DocumentJournal
                | Kind::ChartOfAccounts
                | Kind::ChartOfCalculationTypes
                | Kind::ChartOfCharacteristicTypes
                | Kind::AccountingRegister
                | Kind::AccumulationRegister
                | Kind::CalculationRegister
                | Kind::Recalculation
                | Kind::DataProcessor
                | Kind::Report
                | Kind::ExchangePlan
                | Kind::BusinessProcess
                | Kind::Task
                | Kind::Sequence
                | Kind::SettingsStorage
                | Kind::Subsystem
        ) {
            for child in elements(obj) {
                if !matches(child, "Properties")
                    && !matches(child, "InternalInfo")
                    && !matches(child, "ChildObjects")
                {
                    return Err(error(Code::NestedUnsupported, path));
                }
            }
        }
        if matches!(
            kind,
            Kind::HTTPService | Kind::WebService | Kind::IntegrationService
        ) {
            for child in elements(obj) {
                if !matches(child, "Properties")
                    && !matches(child, "ChildObjects")
                    && !(kind == Kind::IntegrationService && matches(child, "InternalInfo"))
                {
                    return Err(error(Code::NestedUnsupported, path));
                }
            }
        }
        let containers: Vec<_> = elements(obj)
            .into_iter()
            .filter(|n| matches(*n, "ChildObjects"))
            .collect();
        if matches!(
            kind,
            Kind::Form
                | Kind::Template
                | Kind::CommonModule
                | Kind::CommonCommand
                | Kind::CommonForm
                | Kind::CommonTemplate
                | Kind::CommonPicture
                | Kind::Constant
                | Kind::Role
                | Kind::ScheduledJob
                | Kind::XDTOPackage
                | Kind::Descriptor(_)
        ) && !containers.is_empty()
            && !empty_filter
        {
            return Err(error(Code::NestedUnsupported, path));
        }
        if empty_filter
            && containers
                .iter()
                .any(|n| n.attributes().len() != 0 || !no_text(*n) || !elements(*n).is_empty())
        {
            return Err(error(Code::NestedUnsupported, path));
        }
        if kind == Kind::Recalculation
            && (containers.len() != 1 || elements(containers[0]).is_empty())
        {
            return Err(error(Code::Shape, path));
        }
        if containers.len() > 1 {
            return Err(error(Code::Duplicate, path));
        }
        for other in elements(obj) {
            if !matches(other, "Properties") && !matches(other, "ChildObjects") {
                self.unverified.push(Unverified {
                    path: path.into(),
                    selector: format!(
                        "{selector}/{{{}}}{}",
                        other.tag_name().namespace().unwrap_or(""),
                        other.tag_name().name()
                    ),
                });
            }
        }
        if let Some(container) = containers.first() {
            if container.attributes().len() != 0 || !no_text(*container) {
                return Err(error(Code::Shape, path));
            }
            let refs = elements(*container);
            if kind != Kind::Configuration && !refs.is_empty() {
                if !self.inline {
                    return Err(error(Code::NestedUnsupported, path));
                }
                parse_inline_children(
                    &refs,
                    kind.name(),
                    None,
                    path,
                    &mut self.declarations,
                    &mut self.unverified,
                    self.companions,
                    &mut self.companion_refs,
                    matches!(
                        kind,
                        Kind::ChartOfAccounts
                            | Kind::ChartOfCalculationTypes
                            | Kind::ChartOfCharacteristicTypes
                    ),
                    self.registers,
                    (self.reports && matches!(kind, Kind::DataProcessor | Kind::Report))
                        || (self.workflows
                            && matches!(
                                kind,
                                Kind::ExchangePlan | Kind::BusinessProcess | Kind::Task
                            ))
                        || (self.corpus
                            && matches!(
                                kind,
                                Kind::Sequence | Kind::SettingsStorage | Kind::Subsystem
                            )),
                )?;
                return Ok(references);
            }
            let mut seen = BTreeSet::new();
            for child in refs {
                let child_kind = if self.roots {
                    if child.tag_name().namespace() != Some(NS) {
                        return Err(error(Code::Namespace, path));
                    }
                    root_locator_with_corpus(
                        child.tag_name().name(),
                        self.shared,
                        self.leaves,
                        self.journals,
                        self.services,
                        self.charts,
                        self.registers,
                        self.reports,
                        self.workflows,
                        self.corpus,
                    )
                    .ok_or_else(|| error(Code::UnsupportedType, path))?
                    .kind
                } else if matches(child, "Catalog") {
                    Kind::Catalog
                } else if matches(child, "Document") {
                    Kind::Document
                } else if self.common && matches(child, "CommonModule") {
                    Kind::CommonModule
                } else if matches(child, "InformationRegister") {
                    Kind::InformationRegister
                } else {
                    return Err(error(Code::UnsupportedType, path));
                };
                if child.attributes().len() != 0 || !elements(child).is_empty() {
                    return Err(error(Code::Shape, path));
                }
                let child_name = scalar(child);
                if !identifier(&child_name) {
                    return Err(error(Code::Identifier, path));
                }
                let target = if self.roots {
                    root_locator_with_corpus(
                        child_kind.name(),
                        self.shared,
                        self.leaves,
                        self.journals,
                        self.services,
                        self.charts,
                        self.registers,
                        self.reports,
                        self.workflows,
                        self.corpus,
                    )
                    .unwrap()
                    .path(&child_name)?
                } else {
                    format!("{}/{}.xml", child_kind.folder(), child_name)
                };
                if !seen.insert(target.clone()) {
                    return Err(error(Code::Duplicate, path));
                }
                if collect_references {
                    if self.shared {
                        let span = child.range();
                        self.root_ranges
                            .insert(target.clone(), (span.start, span.end));
                    }
                    references.push((target, child_kind, child_name));
                } else {
                    self.read(&target, child_kind, Some(&child_name))?;
                }
            }
        }
        Ok(references)
    }
}

/// One bounded file, same structural parser as legacy. Cross-file checks belong to caller.
#[cfg(feature = "experimental-snapshot-v5")]
pub(crate) struct ParsedFile {
    pub object: Object,
    pub unverified: Vec<Unverified>,
    pub references: Vec<(String, Kind, String)>,
    pub root_ranges: BTreeMap<String, (usize, usize)>,
    pub declarations: Vec<Declaration>,
    pub companion_refs: Vec<CompanionRef>,
}
#[cfg(feature = "experimental-snapshot-v5")]
pub(crate) fn inspect_file(
    path: &str,
    raw: &[u8],
    kind: Kind,
    expected: Option<&str>,
    modules: &[String],
) -> Result<ParsedFile> {
    inspect_file_mode(path, raw, kind, expected, modules, false)
}
#[cfg(feature = "experimental-snapshot-v5")]
pub(crate) fn inspect_file_mode(
    path: &str,
    raw: &[u8],
    kind: Kind,
    expected: Option<&str>,
    modules: &[String],
    inline: bool,
) -> Result<ParsedFile> {
    inspect_file_options(path, raw, kind, expected, modules, inline, false)
}
#[cfg(feature = "experimental-snapshot-v5")]
pub(crate) fn inspect_file_options(
    path: &str,
    raw: &[u8],
    kind: Kind,
    expected: Option<&str>,
    modules: &[String],
    inline: bool,
    companions: bool,
) -> Result<ParsedFile> {
    inspect_file_profile(
        path, raw, kind, expected, modules, inline, companions, false,
    )
}
#[cfg(feature = "experimental-snapshot-v5")]
pub(crate) fn inspect_file_profile(
    path: &str,
    raw: &[u8],
    kind: Kind,
    expected: Option<&str>,
    modules: &[String],
    inline: bool,
    companions: bool,
    common: bool,
) -> Result<ParsedFile> {
    inspect_root_profile(
        path, raw, kind, expected, modules, inline, companions, common, false,
    )
}
#[cfg(feature = "experimental-snapshot-v5")]
pub(crate) fn inspect_root_profile(
    path: &str,
    raw: &[u8],
    kind: Kind,
    expected: Option<&str>,
    modules: &[String],
    inline: bool,
    companions: bool,
    common: bool,
    roots: bool,
) -> Result<ParsedFile> {
    inspect_shared_profile(
        path, raw, kind, expected, modules, inline, companions, common, roots, false, false, false,
        false, false, false, false, false, false,
    )
}
#[cfg(feature = "experimental-snapshot-v5")]
pub(crate) fn inspect_shared_profile(
    path: &str,
    raw: &[u8],
    kind: Kind,
    expected: Option<&str>,
    modules: &[String],
    inline: bool,
    companions: bool,
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
) -> Result<ParsedFile> {
    inspect_shared_profile_configuration(
        path, raw, kind, expected, modules, inline, companions, common, roots, shared, leaves,
        journals, services, charts, registers, reports, workflows, corpus, false,
    )
}
#[cfg(feature = "experimental-snapshot-v5")]
pub(crate) fn inspect_shared_profile_configuration(
    path: &str,
    raw: &[u8],
    kind: Kind,
    expected: Option<&str>,
    modules: &[String],
    inline: bool,
    companions: bool,
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
    configuration: bool,
) -> Result<ParsedFile> {
    path_check(path)?;
    if modules.len() > 4 {
        return Err(error(Code::Limit, path));
    }
    let blobs = modules.iter().map(|p| (p.clone(), Vec::new())).collect();
    let docs = BTreeMap::new();
    let doc = parse_mode(raw, path, configuration && kind == Kind::Configuration)?;
    let mut state = State {
        blobs: &blobs,
        docs: &docs,
        seen: BTreeSet::new(),
        uuids: BTreeSet::new(),
        objects: vec![],
        unverified: vec![],
        assigned: BTreeSet::new(),
        inline: false,
        declarations: vec![],
        shared: false,
        leaves: false,
        journals: false,
        services: false,
        charts: false,
        registers: false,
        reports: false,
        workflows: false,
        corpus: false,
        root_ranges: BTreeMap::new(),
        roots: false,
        common: false,
        companions: false,
        companion_refs: vec![],
    };
    state.inline = inline;
    state.companions = companions;
    state.common = common;
    state.roots = roots;
    state.shared = shared;
    state.leaves = leaves;
    state.journals = journals;
    state.services = services;
    state.charts = charts;
    state.registers = registers;
    state.reports = reports;
    state.workflows = workflows;
    state.corpus = corpus;
    let references = state.read_document(&doc, path, kind, expected, true)?;
    if raw.starts_with(&[0xef, 0xbb, 0xbf]) {
        for (start, end) in state.root_ranges.values_mut() {
            *start += 3;
            *end += 3;
        }
        for r in &mut state.companion_refs {
            r.start += 3;
            r.end += 3;
        }
        for d in &mut state.declarations {
            d.start += 3;
            d.end += 3;
        }
    }
    Ok(ParsedFile {
        object: state.objects.remove(0),
        unverified: state.unverified,
        references,
        root_ranges: state.root_ranges,
        declarations: state.declarations,
        companion_refs: state.companion_refs,
    })
}

/// Opaque key is distinct from the actual XML path; offsets address original bytes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Declaration {
    pub key: String,
    pub parent: Option<String>,
    pub kind: String,
    pub name: String,
    pub uuid: String,
    pub path: String,
    pub start: usize,
    pub end: usize,
}
fn parse_inline_children(
    nodes: &[Node<'_, '_>],
    parent_kind: &str,
    parent: Option<&str>,
    path: &str,
    out: &mut Vec<Declaration>,
    unverified: &mut Vec<Unverified>,
    companions: bool,
    refs: &mut Vec<CompanionRef>,
    chart: bool,
    registers: bool,
    report: bool,
) -> Result<()> {
    let allowed: &[&str] = match parent_kind {
        "Sequence" => &["Dimension"],
        "SettingsStorage" | "Subsystem" => &[],
        "ExchangePlan" | "BusinessProcess" => &["Attribute", "TabularSection"],
        "Task" => &["AddressingAttribute", "Attribute", "Command"],
        "DataProcessor" | "Report" => &["Attribute", "Command", "TabularSection"],
        "Catalog" | "Document" => &["Attribute", "Command", "TabularSection"],
        "InformationRegister" => &["Attribute", "Command", "Dimension", "Resource"],
        "TabularSection" => &["Attribute"],
        "Enum" => &["EnumValue"],
        "DocumentJournal" => &["Column", "Command"],
        "HTTPService" => &["URLTemplate"],
        "URLTemplate" => &["Method"],
        "WebService" => &["Operation"],
        "Operation" => &["Parameter"],
        "IntegrationService" => &["IntegrationServiceChannel"],
        "ChartOfAccounts" => &["AccountingFlag", "ExtDimensionAccountingFlag", "Attribute"],
        "AccountingRegister" => &["Attribute", "Dimension", "Resource"],
        "AccumulationRegister" | "CalculationRegister" => &["Dimension", "Resource"],
        "Recalculation" => &["Dimension"],
        "ChartOfCalculationTypes" => &[],
        "ChartOfCharacteristicTypes" => &["Attribute", "TabularSection"],
        _ => return Err(error(Code::NestedUnsupported, path)),
    };
    let service = matches!(
        parent_kind,
        "HTTPService" | "URLTemplate" | "WebService" | "Operation" | "IntegrationService"
    );
    let register = registers
        && matches!(
            parent_kind,
            "AccountingRegister" | "AccumulationRegister" | "CalculationRegister" | "Recalculation"
        );
    let mut names = BTreeSet::new();
    for node in nodes {
        let kind = node.tag_name().name();
        if node.tag_name().namespace() != Some(NS) {
            return Err(error(Code::Namespace, path));
        }
        if kind == "Form"
            || kind == "Template"
            || (registers && kind == "Recalculation")
            || (report && kind == "Subsystem")
        {
            if !companions
                || parent.is_some()
                || parent_kind == "Enum"
                || service
                || (chart && kind == "Template")
                || (parent_kind == "Sequence")
                || (parent_kind == "SettingsStorage" && kind != "Form")
                || (parent_kind == "Subsystem" && kind != "Subsystem")
                || (kind == "Subsystem" && parent_kind != "Subsystem")
                || (kind == "Template" && ["BusinessProcess", "Task"].contains(&parent_kind))
                || (register && (kind == "Template" || parent_kind == "Recalculation"))
                || (kind == "Recalculation" && parent_kind != "CalculationRegister")
            {
                return Err(error(Code::NestedUnsupported, path));
            }
            if node.attributes().len() != 0 || !elements(*node).is_empty() {
                return Err(error(Code::Shape, path));
            }
            let name = scalar(*node);
            if !identifier(&name) || name.len() > 256 {
                return Err(error(Code::Identifier, path));
            }
            if !names.insert((kind.to_owned(), name.clone())) {
                return Err(error(Code::Duplicate, path));
            }
            let range = node.range();
            refs.push(CompanionRef {
                kind: if kind == "Form" {
                    Kind::Form
                } else if kind == "Subsystem" {
                    Kind::Subsystem
                } else if kind == "Recalculation" {
                    Kind::Recalculation
                } else {
                    Kind::Template
                },
                name,
                start: range.start,
                end: range.end,
            });
            continue;
        }
        if !allowed.contains(&kind) {
            return Err(error(Code::UnsupportedType, path));
        }
        if !no_text(*node) {
            return Err(error(Code::Shape, path));
        }
        if (report
            || register
            || chart
            || service
            || matches!(parent_kind, "Enum" | "DocumentJournal"))
            && (node.attributes().len() != 1
                || node
                    .attributes()
                    .any(|a| a.namespace().is_some() || a.name() != "uuid"))
        {
            return Err(error(Code::Uuid, path));
        }
        let uid = node
            .attribute("uuid")
            .filter(|s| uuid(s))
            .ok_or_else(|| error(Code::Uuid, path))?;
        let props: Vec<_> = elements(*node)
            .into_iter()
            .filter(|n| matches(*n, "Properties"))
            .collect();
        if props.len() != 1 || !no_text(props[0]) {
            return Err(error(Code::Shape, path));
        }
        let named: Vec<_> = elements(props[0])
            .into_iter()
            .filter(|n| matches(*n, "Name"))
            .collect();
        if named.len() != 1 || !elements(named[0]).is_empty() || named[0].attributes().len() != 0 {
            return Err(error(Code::Shape, path));
        }
        let name = scalar(named[0]);
        if name.trim().is_empty()
            || name.len() > 1024
            || ((report
                || register
                || chart
                || service
                || matches!(parent_kind, "Enum" | "DocumentJournal"))
                && !identifier(&name))
        {
            return Err(error(Code::Identifier, path));
        }
        if !names.insert((kind.to_owned(), name.clone())) {
            return Err(error(Code::Duplicate, path));
        }
        let key = format!("{path}#{}", uid.to_lowercase());
        let selector = format!("inline[{key}]");
        for attr in node.attributes() {
            if attr.name() != "uuid" || attr.namespace().is_some() {
                unverified.push(Unverified {
                    path: path.into(),
                    selector: format!(
                        "{selector}/@{{{}}}{}",
                        attr.namespace().unwrap_or(""),
                        attr.name()
                    ),
                });
            }
        }
        for attr in props[0].attributes() {
            unverified.push(Unverified {
                path: path.into(),
                selector: format!(
                    "{selector}/Properties/@{{{}}}{}",
                    attr.namespace().unwrap_or(""),
                    attr.name()
                ),
            });
        }
        for prop in elements(props[0]) {
            if !matches(prop, "Name") {
                unverified.push(Unverified {
                    path: path.into(),
                    selector: format!(
                        "{selector}/Properties/{{{}}}{}",
                        prop.tag_name().namespace().unwrap_or(""),
                        prop.tag_name().name()
                    ),
                });
            }
        }
        let children: Vec<_> = elements(*node)
            .into_iter()
            .filter(|n| matches(*n, "ChildObjects"))
            .collect();
        for other in elements(*node) {
            if !matches(other, "Properties") && !matches(other, "ChildObjects") {
                if register
                    || (report && !(kind == "TabularSection" && matches(other, "InternalInfo")))
                    || (chart && !(kind == "TabularSection" && matches(other, "InternalInfo")))
                    || matches!(parent_kind, "Enum" | "DocumentJournal")
                    || (service
                        && !(kind == "IntegrationServiceChannel" && matches(other, "InternalInfo")))
                {
                    return Err(error(Code::NestedUnsupported, path));
                }
                unverified.push(Unverified {
                    path: path.into(),
                    selector: format!(
                        "{selector}/{{{}}}{}",
                        other.tag_name().namespace().unwrap_or(""),
                        other.tag_name().name()
                    ),
                });
            }
        }
        let range = node.range();
        out.push(Declaration {
            key: key.clone(),
            parent: parent.map(str::to_owned),
            kind: kind.into(),
            name,
            uuid: uid.to_lowercase(),
            path: path.into(),
            start: range.start,
            end: range.end,
        });
        if ["TabularSection", "URLTemplate", "Operation"].contains(&kind) {
            if children.len() != 1
                || children[0].attributes().len() != 0
                || !no_text(children[0])
                || (kind != "Operation" && elements(children[0]).is_empty())
            {
                return Err(error(Code::Shape, path));
            }
            parse_inline_children(
                &elements(children[0]),
                kind,
                Some(&key),
                path,
                out,
                unverified,
                companions,
                refs,
                chart,
                registers,
                report,
            )?;
        } else if !children.is_empty() {
            return Err(error(Code::NestedUnsupported, path));
        }
    }
    Ok(())
}

#[cfg(all(test, feature = "experimental-snapshot-v5"))]
mod inline_tests {
    use super::*;
    fn parse_body(kind: Kind, body: &str) -> Result<ParsedFile> {
        let xml=format!("<MetaDataObject xmlns=\"{NS}\" version=\"2.21\"><{k} uuid=\"00000000-0000-0000-0000-000000000001\"><Properties><Name>Test</Name></Properties><ChildObjects>{body}</ChildObjects></{k}></MetaDataObject>",k=kind.name());
        inspect_file_mode("Catalogs/Test.xml", xml.as_bytes(), kind, None, &[], true)
    }
    fn leaf(kind: &str, id: u64) -> String {
        format!("<{kind} uuid=\"00000000-0000-0000-0000-{id:012}\"><Properties><Name>Name{id}</Name><UnknownProperty><Content/></UnknownProperty></Properties></{kind}>")
    }
    #[test]
    fn roles_and_extra_tabular_setting_are_bounded_structural_facts_only() {
        for kind in [Kind::Catalog, Kind::Document, Kind::InformationRegister] {
            let mut body = leaf("Attribute", 2) + &leaf("Command", 3);
            if kind == Kind::InformationRegister {
                body += &(leaf("Dimension", 4) + &leaf("Resource", 5));
            } else {
                body+=&format!("<TabularSection uuid=\"00000000-0000-0000-0000-000000000004\"><Properties><Name>Rows</Name></Properties><AdditionalSetting><Content/></AdditionalSetting><ChildObjects>{}</ChildObjects></TabularSection>",leaf("Attribute",5));
            }
            let a = parse_body(kind, &body).unwrap();
            let b = parse_body(kind, &body).unwrap();
            assert_eq!(a.declarations, b.declarations);
            assert_eq!(a.declarations.len(), 4);
            assert!(!a.unverified.is_empty());
            if kind != Kind::InformationRegister {
                assert_eq!(
                    a.declarations[3].parent.as_deref(),
                    Some(a.declarations[2].key.as_str())
                );
                assert!(a
                    .unverified
                    .iter()
                    .any(|u| u.selector.contains("AdditionalSetting")));
            }
        }
    }
    #[test]
    fn unknown_companions_roles_and_malformed_inline_fail_closed() {
        let valid = leaf("Attribute", 2);
        let cases=vec!["<Form>F</Form>".into(),"<Template>T</Template>".into(),"<Future/>".into(),leaf("Dimension",2),
            valid.replace("00000000-0000-0000-0000-000000000002","bad"),valid.replace("<Name>Name2</Name>","<Name/>"),valid.replace("<Name>Name2</Name>","<Name><Bad/></Name>"),valid.replace("</Properties>","</Properties><Properties><Name>Again</Name></Properties>"),
            valid.replace("<Attribute uuid", "<Attribute xmlns=\"other\" uuid"),valid.replace("</Attribute>","<ChildObjects/></Attribute>"),valid.clone()+&valid,
            "<TabularSection uuid=\"00000000-0000-0000-0000-000000000002\"><Properties><Name>T</Name></Properties></TabularSection>".into(),
            "<TabularSection uuid=\"00000000-0000-0000-0000-000000000002\"><Properties><Name>T</Name></Properties><ChildObjects>bad</ChildObjects></TabularSection>".into()];
        for body in cases {
            assert!(parse_body(Kind::Catalog, &body).is_err(), "{body}");
        }
        assert!(parse_body(Kind::InformationRegister,&format!("<TabularSection uuid=\"00000000-0000-0000-0000-000000000002\"><Properties><Name>T</Name></Properties><ChildObjects>{}</ChildObjects></TabularSection>",leaf("Attribute",3))).is_err());
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CompanionRef {
    pub kind: Kind,
    pub name: String,
    pub start: usize,
    pub end: usize,
}

#[cfg(test)]
mod root_locator_tests {
    use super::*;
    #[test]
    fn locator_registry_is_unique_and_exact() {
        let mut names = BTreeSet::new();
        let mut dirs = BTreeSet::new();
        let mut kinds = BTreeSet::new();
        for r in ROOT_LOCATORS {
            assert!(names.insert(r.xml_type));
            assert!(dirs.insert(r.directory));
            assert!(kinds.insert(r.kind));
            assert_eq!(r.kind.name(), r.xml_type);
            assert_eq!(r.kind.folder(), r.directory);
            assert_eq!(r.path("Имя").unwrap(), format!("{}/Имя.xml", r.directory));
            for bad in ["../Name", "a/b", "a.b", "", "a\\b"] {
                assert!(r.path(bad).is_err());
            }
        }
        assert_eq!(
            root_locator("FunctionalOptionsParameter")
                .unwrap()
                .directory,
            "FunctionalOptionsParameters"
        );
        assert_eq!(
            root_locator("FilterCriterion").unwrap().directory,
            "FilterCriteria"
        );
        for bad in [
            "FilterCriteria",
            "filtercriterion",
            "Unknown",
            "Form",
            "FunctionalOptionParameter",
        ] {
            assert!(root_locator(bad).is_none());
        }
    }
}

#[cfg(all(test, feature = "experimental-snapshot-v5"))]
mod configuration_dom_budget_tests {
    use super::*;
    fn nodes(n: usize) -> String {
        format!("<r>{}</r>", "<n/>".repeat(n - 2))
    }
    fn events(s: &str) -> usize {
        let mut r = Reader::from_str(s);
        let mut n = 0;
        loop {
            n += 1;
            if matches!(r.read_event().unwrap(), Event::Eof) {
                return n;
            }
        }
    }
    #[test]
    fn configuration_exact_dom_cap_and_other_documents_unchanged() {
        let old = nodes(10000);
        assert_eq!(
            parse(old.as_bytes(), "Configuration.xml")
                .unwrap()
                .descendants()
                .count(),
            10000
        );
        let extra = nodes(10001);
        assert_eq!(
            parse(extra.as_bytes(), "Configuration.xml")
                .unwrap_err()
                .code,
            Code::Limit
        );
        assert_eq!(
            parse_mode(extra.as_bytes(), "Languages/Other.xml", true)
                .unwrap_err()
                .code,
            Code::Limit
        );
        let exact = nodes(MAX_CONFIGURATION_NODES as usize);
        assert!(events(&exact) < MAX_EVENTS);
        assert_eq!(
            parse_mode(exact.as_bytes(), "Configuration.xml", true)
                .unwrap()
                .descendants()
                .count(),
            16384
        );
        let over = nodes(MAX_CONFIGURATION_NODES as usize + 1);
        assert!(events(&over) < MAX_EVENTS);
        assert_eq!(
            parse_mode(over.as_bytes(), "Configuration.xml", true)
                .unwrap_err()
                .code,
            Code::Limit
        );
        // Matching path alone cannot grant the exception to a different structural type.
        let e = inspect_shared_profile_configuration(
            "Configuration.xml",
            extra.as_bytes(),
            Kind::Catalog,
            None,
            &[],
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
            true,
        )
        .err()
        .expect("must reject wrong kind");
        assert_eq!(e.code, Code::Limit);
    }
    #[test]
    fn configuration_event_depth_byte_caps_remain_exact() {
        let exact = format!("<r>{}<!--x--></r>", "<n></n>".repeat(9998));
        assert_eq!(events(&exact), MAX_EVENTS);
        assert!(parse_mode(exact.as_bytes(), "Configuration.xml", true).is_ok());
        let over = exact.replace("</r>", "<!--x--></r>");
        assert_eq!(events(&over), MAX_EVENTS + 1);
        assert!(Document::parse(&over).unwrap().descendants().count() < 16384);
        assert_eq!(
            parse_mode(over.as_bytes(), "Configuration.xml", true)
                .unwrap_err()
                .code,
            Code::Limit
        );
        for n in [MAX_DEPTH, MAX_DEPTH + 1] {
            let s = format!("{}{}", "<n>".repeat(n), "</n>".repeat(n));
            assert_eq!(
                parse_mode(s.as_bytes(), "Configuration.xml", true).is_ok(),
                n == MAX_DEPTH
            );
        }
        let exact = format!("<r>{}</r>", "x".repeat(MAX_XML_BYTES - 7));
        assert_eq!(exact.len(), MAX_XML_BYTES);
        assert!(parse_mode(exact.as_bytes(), "Configuration.xml", true).is_ok());
        let over = exact.replace("</r>", "x</r>");
        assert_eq!(
            parse_mode(over.as_bytes(), "Configuration.xml", true)
                .unwrap_err()
                .code,
            Code::Limit
        );
    }
}
