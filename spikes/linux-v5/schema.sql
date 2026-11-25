-- S2: экспериментальный Linux-профиль, не production-снимок.
-- foreign_keys необходимо включать на КАЖДОМ пишущем соединении вне транзакции.
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = DELETE;
PRAGMA user_version = 5;
BEGIN;
CREATE TABLE metadata (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    schema_version INTEGER NOT NULL CHECK (schema_version = 5),
    profile TEXT NOT NULL CHECK (profile = 'linux-v5-spike-xml221-bsl-r09'),
    snapshot_id TEXT NOT NULL CHECK (snapshot_id GLOB 'bslens-v5-?*'),
    built_at_utc TEXT NOT NULL CHECK (
        length(built_at_utc) = 20 AND
        built_at_utc GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z' AND
        strftime('%Y-%m-%dT%H:%M:%SZ', built_at_utc) IS NOT NULL AND
        strftime('%Y-%m-%dT%H:%M:%SZ', built_at_utc) = built_at_utc),
    facts_sha256 TEXT CHECK (facts_sha256 IS NULL OR
        (length(facts_sha256) = 64 AND facts_sha256 NOT GLOB '*[^0-9a-f]*')),
    source_id TEXT NOT NULL CHECK (length(source_id) > 0),
    commit_oid TEXT NOT NULL CHECK (length(commit_oid) IN (40,64) AND commit_oid NOT GLOB '*[^0-9a-f]*'),
    tree_oid TEXT NOT NULL CHECK (length(tree_oid) IN (40,64) AND tree_oid NOT GLOB '*[^0-9a-f]*'),
    core_version TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    xml_version TEXT NOT NULL,
    lexical_version TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    build_parameters TEXT NOT NULL CHECK (json_valid(build_parameters)),
    input_manifest TEXT NOT NULL CHECK (json_valid(input_manifest)),
    coverage_draft TEXT NOT NULL CHECK (json_valid(coverage_draft)),
    limits_draft TEXT NOT NULL CHECK (json_valid(limits_draft)),
    relations TEXT NOT NULL CHECK (relations = 'not_analyzed')
) STRICT;
CREATE TABLE sources (
    path TEXT PRIMARY KEY CHECK (length(path) > 0 AND substr(path,1,1) <> '/'
        AND instr(path, char(92)) = 0 AND instr(path, char(0)) = 0
        AND instr('/' || path || '/', '/../') = 0
        AND instr('/' || path || '/', '/./') = 0 AND instr(path,'//') = 0
        AND substr(path,-1) <> '/'),
    kind TEXT NOT NULL CHECK (kind IN ('xml','bsl')),
    mode TEXT NOT NULL CHECK (mode IN ('100644','100755')),
    blob_oid TEXT NOT NULL CHECK (length(blob_oid) IN (40,64) AND blob_oid NOT GLOB '*[^0-9a-f]*'),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    content BLOB NOT NULL,
    UNIQUE(path,kind)
) STRICT;
CREATE TABLE objects (
    object_id INTEGER PRIMARY KEY,
    parent_id INTEGER REFERENCES objects(object_id),
    kind TEXT NOT NULL CHECK (length(kind) > 0),
    name TEXT NOT NULL CHECK (length(name) > 0),
    xml_path TEXT NOT NULL UNIQUE,
    source_kind TEXT NOT NULL DEFAULT 'xml' CHECK (source_kind = 'xml'),
    CHECK ((kind = 'Configuration' AND parent_id IS NULL) OR
           (kind <> 'Configuration' AND parent_id IS NOT NULL)),
    CHECK (parent_id IS NULL OR parent_id <> object_id),
    FOREIGN KEY(xml_path,source_kind) REFERENCES sources(path,kind),
    UNIQUE(parent_id,kind,name)
) STRICT;
CREATE UNIQUE INDEX one_configuration ON objects(kind) WHERE kind = 'Configuration';
CREATE INDEX object_branch ON objects(parent_id,kind,name,object_id);
CREATE TABLE modules (
    path TEXT PRIMARY KEY,
    source_kind TEXT NOT NULL DEFAULT 'bsl' CHECK (source_kind = 'bsl'),
    owner_id INTEGER REFERENCES objects(object_id),
    parse_status TEXT NOT NULL CHECK (parse_status IN ('accepted','unparsed')),
    reason_code TEXT CHECK (reason_code IN (
        'Limit','Utf8','UnsupportedCharacter','InvalidControl','ReservedName',
        'MultilineString','DirectiveUnsupported','RegionHeader','RegionInsideMethod',
        'RegionUnmatchedEnd','RegionMissingEnd','AnnotationUnsupported','AsyncUnsupported',
        'UnsupportedHeader','AmbiguousBoundary','Delimiters','MissingEnd','UnmatchedEnd')),
    CHECK ((parse_status = 'accepted' AND reason_code IS NULL) OR
           (parse_status = 'unparsed' AND reason_code IS NOT NULL AND length(reason_code) > 0)),
    FOREIGN KEY(path,source_kind) REFERENCES sources(path,kind),
    UNIQUE(path,parse_status)
) STRICT;
CREATE INDEX module_branch ON modules(owner_id,path);
CREATE TABLE source_lines (
    path TEXT NOT NULL,
    source_kind TEXT NOT NULL DEFAULT 'bsl' CHECK (source_kind = 'bsl'),
    line_number INTEGER NOT NULL CHECK (line_number >= 1),
    byte_start INTEGER NOT NULL CHECK (byte_start >= 0),
    byte_end INTEGER NOT NULL CHECK (byte_end > byte_start),
    PRIMARY KEY(path,line_number),
    FOREIGN KEY(path,source_kind) REFERENCES sources(path,kind)
) STRICT;
CREATE TABLE methods (
    method_id INTEGER PRIMARY KEY,
    module_path TEXT NOT NULL,
    parse_status TEXT NOT NULL DEFAULT 'accepted' CHECK (parse_status = 'accepted'),
    kind TEXT NOT NULL CHECK (kind IN ('procedure','function')),
    name TEXT NOT NULL CHECK (length(name) > 0),
    exported INTEGER NOT NULL CHECK (exported IN (0,1)),
    start_line INTEGER NOT NULL CHECK (start_line >= 1),
    end_line INTEGER NOT NULL CHECK (end_line >= start_line),
    byte_start INTEGER NOT NULL CHECK (byte_start >= 0),
    byte_end INTEGER NOT NULL CHECK (byte_end > byte_start),
    FOREIGN KEY(module_path,parse_status) REFERENCES modules(path,parse_status),
    FOREIGN KEY(module_path,start_line) REFERENCES source_lines(path,line_number),
    FOREIGN KEY(module_path,end_line) REFERENCES source_lines(path,line_number),
    UNIQUE(module_path,start_line)
) STRICT;
CREATE INDEX method_branch ON methods(module_path,start_line,method_id);
COMMIT;
