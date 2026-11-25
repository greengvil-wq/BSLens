# ADR-031: полные XML diagnostics отдельными строками

Основа `12260e2cc4c0ee3f3d8cc2a88c37d3b58e5bc938`. Новый явный
`build-diagnostics`, профиль `linux-v5-diagnostics1-xml221-r09` наследует
[corpus-config1](adr-030-configuration-dom.md). Старые профили, их DDL, массив
`xml_unverified` и прежний coverage budget остаются неизменными.

Переданная локальная диагностика БСП показала: уже 42703 unverified-записи только
корневых Properties занимают 8035108 bytes компактного JSON. Поэтому повышение
внешнего envelope или точный подсчёт прежнего массива в 1 MiB не решают задачу.
Реальные данные не переносились в облако; этот документ не объявляет приёмку БСП.

## DDL и точный учёт

В новом профиле добавлены только таблица и индекс:

```sql
CREATE TABLE xml_diagnostics (
  diagnostic_id INTEGER PRIMARY KEY,
  path TEXT NOT NULL REFERENCES sources(path),
  selector TEXT NOT NULL
) STRICT;
CREATE INDEX xml_diagnostics_order
  ON xml_diagnostics(path COLLATE BINARY, selector COLLATE BINARY);
```

`path` — фактический сохранённый XML, `selector` — прежняя полная диагностика
непроверенного свойства/атрибута. Семантика свойства не утверждается. Ничего не
усекается; одинаковые записи сохраняются с кратностью. diagnostic_id — локальный
surrogate, не часть логической identity или evidence digest.

Каноническая строка — компактный UTF-8 JSON `{"path":...,"selector":...}` с точным
escaping serde_json закреплённого Cargo.lock. Порядок — BINARY(path, selector).
Дубликаты дают повторные одинаковые строки. `bytes` — **сумма длин JSON-строк**,
без внешнего массива/запятых и без framing digest. Это не физический размер SQLite.

Жёсткие пределы:

* строка: 16384 bytes канонического JSON; предварительно ограничен и сырой текст;
* сумма: обязательный `MAX_DIAGNOSTICS_BYTES`, от 1 до 67108864 (64 MiB);
* каждая диагностика входит в общий MAX_FACTS, hard cap 2000000;
* полная coverage metadata: не более 8192 bytes и прежнего MAX_COVERAGE_BYTES
  (последний по-прежнему максимум 1048576);
* build_parameters: прежние 16384 bytes; полный wire JSON-RPC: прежние 65536.

Все sources/Git/XML/BSL, BIN, SQLite page cap, scratch, count и cooperative time
limits сохранены. Ошибки Limit(diagnostic_row_bytes), Limit(diagnostics_bytes),
Limit(facts), coverage_summary_bytes или SQLite page cap вызывают откат и отсутствие
публикации. Превышение строкового предела не скрывается пропуском диагностики.

Build/verify держат XML и его ограниченный DOM/диагностики **одного файла**,
записывая строки сразу в SQLite. Общий Vec всех diagnostics нового профиля пуст;
общий JSON-массив не создаётся. Индекс задаёт порядок без сортировки всего корпуса
в Rust; digest читает одну строку за раз. На строку до проверки escaping может
потребоваться до примерно 6 × 16384 bytes плюс JSON overhead; это фиксированная
граница, не зависящая от полного корпуса. SQLite страницы, индекс и остальные
структуры producer также расходуют память/диск: 64 MiB diagnostics не означают
64 MiB RSS или 64 MiB SQLite. Требуется отдельно достаточная явная SQLite квота.
Время проверяется кооперативно между ограниченными операциями; это не hard kill.

## Metadata, identity, evidence и verify

`build_parameters.diagnostics` содержит `format=xml-diagnostics-json-v1`, явный
`bytes`, фиксированный `row_bytes=16384` и domain. Эти нормализованные параметры и
новый profile входят в snapshot_id. Исходный selector/scope не расширяется.

В `coverage_draft` вместо xml_unverified хранится `xml_diagnostics` со строго
проверяемыми format/domain/count/bytes/sha256/details/semantics. Остальные малые
счётчики покрытия сохраняются. Domain `BSLens-v5/xml-diagnostics/json1`, затем
один NUL; для каждой отсортированной строки SHA-256 получает big-endian u64 её
длины и точные JSON bytes. Count/bytes и digest проверяются на open. Данные не
передаются как непроверенный произвольный metadata массив.

Канонические facts нового профиля имеют отдельный domain
`BSLens-v5/facts/diagnostics1\0` и framed summary диагностик, затем прежние
канонические факты. Таким образом evidence входит в facts digest; старые hash
алгоритмы не меняются. Offline verify из сохранённых sources заново строит
диагностики в scratch SQLite и сравнивает metadata и facts digest. Удаление или
подмена строки обнаруживаются даже при пересчитанных summary/facts/file digest.
Это не проверка Git provenance.

MCP сохраняет прежнюю ограниченную allowlist summary, `details_omitted=true`,
`xml_unverified_count` теперь берётся из проверенного count. Дополнительные малые
поля — `xml_diagnostics_bytes` и `xml_diagnostics_sha256`. `coverage_sha256` —
хеш компактной coverage metadata, **не** digest полного набора строк. Полные
строки доступны в SQLite; отдельный MCP-инструмент их выгрузки не добавлен.
Tree/source и точные байтовые диапазоны не меняются.

## Команды для NucBox

Источники БСП и pinned OID остаются на NucBox. Не удалять рабочий репозиторий.
Значения всех квот выбирает вызывающий; новый последний аргумент не заменяет
старый MAX_COVERAGE_BYTES. Ниже MAX_DIAGNOSTICS_BYTES может быть 67108864;
это bounded budget, а не обещание достаточности для любой БСП.

```sh
cargo +1.85.1 build --locked --features experimental-snapshot-mcp --bins
./target/debug/bslens-v5 build-diagnostics "$BSP_REPO" "$BSP_COMMIT" "$BSP_TREE" "$SOURCE_ID" "$OUTPUT" \
 "$MAX_SOURCES" "$MAX_SOURCE_BYTES" "$MAX_TOTAL_BYTES" "$MAX_SQLITE_BYTES" \
 "$GIT_ENTRIES" "$GIT_TREES" "$GIT_READS" "$GIT_OBJECT_BYTES" "$GIT_TOTAL_BYTES" "$GIT_TIME_MS" \
 "$MAX_FACTS" "$MAX_COVERAGE_BYTES" "$FACTS_TIME_MS" "$GLOBAL_OPAQUE_JSON" \
 "$MAX_OPAQUE_BIN_BYTES" "$MAX_DIAGNOSTICS_BYTES"
./target/debug/bslens-v5 verify "$SNAPSHOT" --staging-dir "$STAGING" \
 --expected-sha256 "$EXPECTED_SHA256" --max-sqlite-bytes "$MAX_SQLITE_BYTES" \
 --max-sources "$MAX_SOURCES" --max-source-bytes "$MAX_SOURCE_BYTES" \
 --max-total-bytes "$MAX_TOTAL_BYTES" --max-scratch-bytes "$MAX_SQLITE_BYTES" \
 --max-facts "$MAX_FACTS" --max-coverage-bytes "$MAX_COVERAGE_BYTES" \
 --facts-time-ms "$FACTS_TIME_MS" --max-opaque-bin-bytes "$MAX_OPAQUE_BIN_BYTES" \
 --max-diagnostics-bytes "$MAX_DIAGNOSTICS_BYTES"
```

Минимальная сессия MCP (JSON-RPC по одной строке, протокол закреплён в коде):

```sh
PATH= ./target/debug/bslens-mcp --snapshot "$SNAPSHOT" --staging-dir "$STAGING" \
 --max-sqlite-bytes "$MAX_SQLITE_BYTES" --expected-sha256 "$EXPECTED_SHA256" <<'JSONRPC'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"local-check","version":"1"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"metadata","arguments":{}}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"tree","arguments":{}}}
JSONRPC
```

MCP запускается по прежнему snapshot/stdIO контракту. Trusted staging — абсолютный
Linux owner-only 0700 с проверенными родителями. Reader предназначен для доверенных
артефактов с независимым SHA; общей защиты от произвольных SQLite не заявлено.
Для аудита: `SELECT path,selector FROM xml_diagnostics ORDER BY path COLLATE BINARY,
selector COLLATE BINARY`. Таблицы sources(path,kind,mode,blob_oid,sha256,content) и
их selector остаются по ADR-027/028/029/030; никакие исходники не исключены.

## Синтетическая проверка

600 Language descriptors × 120 unverified Properties = 72000 записей,
9576000 bytes компактных строк, metadata <8192. Реальный CLI build → удаление
только синтетического Git → offline verify → настоящий stdio handshake/tree/source
с пустым PATH, lookup последнего имени и точные XML/BSL bytes. Проверены повторяемая
identity, точный total и total−1, строковый cap/escaping, общий facts cap, SQLite
quota и rollback; старый профиль отвергает плотную фикстуру. На малой общей фикстуре
новые строки равны прежнему массиву. Удаление/подмена при согласованных внешних
хешах проходит формальный open, но отвергается verify.

```sh
cargo +1.85.1 test --locked --features experimental-snapshot-mcp diagnostics_ -- --nocapture
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
```

Реальная БСП требует отдельного локального повтора. GitHub/main не затрагиваются.
