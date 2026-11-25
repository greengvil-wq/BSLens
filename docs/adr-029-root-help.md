# ADR-029: PNG-ресурс корневой справки DataProcessor

Основа `9402a75802bfc4c74725b9f78dc7b74266e76d88`. Новый явный режим
`build-corpus-help`, профиль `linux-v5-corpus-help1-xml221-r09`.
Он наследует corpus-bin1 и добавляет ровно одну форму пути:

```text
DataProcessors/<Owner>/Ext/Help/<ResourceDir>/<File>.png
```

Шесть сегментов: DataProcessors, непустой Owner, буквальные Ext/Help,
один непустой ResourceDir и имя файла с непустой основой и точным lowercase `.png`.
Действуют общие безопасные repo-relative пути: без пустых сегментов, `.`/`..`,
NUL, абсолютных путей и symlink. Глубже/мельче, другой suffix или Reports — отказ.
Имя владельца `Ext` допустимо и покрыто тестом. PNG не распаковывается, не
декодируется и не интерпретируется как BSL или семантика справки.

Основание — переданная read-only диагностика закреплённой БСП: из 2288 путей
DataProcessors и 412 Reports ровно одна такая дорожка не проходила прежний
validate_path; у неё есть владелец и обе части справки. Реальные имена и bytes
в облако не переносились. Этот факт не расширяет грамматику Reports или других
семейств и не означает локальную приёмку БСП.

## Владение и строгая полнота

Для каждого ресурса должны существовать:

* корневой дескриптор `DataProcessors/<Owner>.xml` из Configuration;
* `DataProcessors/<Owner>/Ext/Help.xml`;
* `DataProcessors/<Owner>/Ext/Help/ru.html`.

Ресурс записывается в `sources` как payload, а в `companion_payloads` — с
`object_id` **самого корня DataProcessor** и `role=additional`. Пути не выдумываются,
байты не преобразуются. Обе части справки остаются непрозрачными источниками.
Пара другой обработки, отчёта или формы не удовлетворяет требованию корня.
Одна отсутствующая часть даёт `report_help_incomplete`; при отсутствии обеих
с наличным PNG — `root_help_pair_required`. Сирота/неверная структура и иные пути
не скрываются исключением из scope. Поздний отказ удаляет свою временную SQLite,
output остаётся без опубликованного снимка.

Общий пофайловый producer проверяет эту же грамматику и пару как при build,
так и при offline verify. Неправильный, но существующий owner с корректными
файловым и facts digest может пройти формальный open; пересчёт verify обнаруживает
несоответствие (`VerifyFacts`). Git provenance этой проверкой не подтверждается.

## Совместимость и пределы

Старые `report1`, `corpus1`, `corpus-bin1` и остальные профили сохраняют прежние
правила: новый путь в них по-прежнему запрещён. В новом DDL меняется только
точный profile; таблицы, столбцы и канонизация фактов не изменены.
В `build_parameters` добавлен фиксированный
`root_help_guard=dataprocessor-root-help-png-pair-v1`; он и profile входят в identity.
Общий предел канонических параметров 16384 bytes сохранён.

BIN policy до 24 MiB наследуется без расширения грамматики: **PNG не получает
BIN cap**. Для него остаются обычные Git object/source caps; все total/count,
SQLite/scratch, facts/coverage/time и parser limits прежние. MCP работает read-only,
одна проверенная копия на сессию; byte range / Base64 восстанавливают точные bytes.
128 узлов ветки, reader byte cap и полный JSON-RPC 65536 bytes не повышены.
Сохраняются lookup, владельцы ранее поддержанных видов и Recalculation.

## Воспроизведение

Команда имеет те же аргументы, что build-corpus-bin, но явное новое имя:

```sh
cargo +1.85.1 build --locked --features experimental-snapshot-mcp --bins
./target/debug/bslens-v5 build-corpus-help "$BSP_REPO" "$BSP_COMMIT" "$BSP_TREE" "$SOURCE_ID" "$OUTPUT" \
 "$MAX_SOURCES" "$MAX_SOURCE_BYTES" "$MAX_TOTAL_BYTES" "$MAX_SQLITE_BYTES" \
 "$GIT_ENTRIES" "$GIT_TREES" "$GIT_READS" "$GIT_OBJECT_BYTES" "$GIT_TOTAL_BYTES" "$GIT_TIME_MS" \
 "$MAX_FACTS" "$MAX_COVERAGE_BYTES" "$FACTS_TIME_MS" "$GLOBAL_OPAQUE_JSON" "$MAX_OPAQUE_BIN_BYTES"
```

Verify по [ADR-028](adr-028-corpus-bin.md) с явным `--max-opaque-bin-bytes`,
MCP — `--snapshot`, trusted staging, max-sqlite-bytes и независимый expected SHA.
Для root PNG: Configuration → DataProcessor → точный name_exact → companions;
`source` принимает возвращённый saved_path и `[start_byte,end_byte)`.
Для длинного файла читать малыми диапазонами, Base64 декодировать без lossy.
Реальные BSP repo/полные pinned OID/bytes остаются на NucBox; рабочий Git не удалять.
[Перенос source-only комплекта](nucbox-corpus-diagnostic.md).

```sh
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
cargo +1.85.1 test --locked --features experimental-snapshot-mcp --test mcp_snapshot root_help_cli
```

Реальный синтетический CLI → удаление только созданного тестом Git → offline
verify → stdio handshake/tree/source с пустым PATH: точные диапазоны, owner корня,
payload kind, deterministic reuse, сохранённые guard/BIN budget. Отрицательные:
каждая отсутствующая часть и вся пара (при существующей чужой паре), Reports,
неверная глубина/суффикс/пустая основа, PNG 8 MiB+1 при обычном cap 8 MiB и BIN cap
24 MiB, поздний facts limit, старый профиль и подмена owner с пересчитанными digest.
Все отказы build проверяют отсутствие опубликованного снимка; verify закрывает
соединения раньше удаления собственной staging-копии.

Это только синтетическая проверка узкой грамматики; полная БСП не принята.
GitHub/main и реальные данные не затронуты.
