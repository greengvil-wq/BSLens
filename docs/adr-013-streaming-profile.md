# Stream-профиль: CLI build → open/verify → MCP без Git

Продолжение локального слоя `e3176f4ce109964ba574032c136a49d0b6f19e68`
поверх #9 `bf97d5e21ece847eda4b51cb0087e13eaef0935c`. Кандидат локальный:
никакие новые коммиты/PR не публикуются, #8/#9/main не изменяются.

## Режим и совместимость

Явная команда `bslens-v5 build-stream` и API `streaming_writer::build` используют
профиль **linux-v5-stream1-xml221-r09**. `build` и `build-ingest` сохраняют прежние
профили, параметры и пределы. Исходный schema.sql не изменён. Третий известный
вариант DDL отличается только CHECK имени профиля; reader проверяет точный каталог
и соответствующее profile, без миграции старых снимков или принятия произвольной схемы.
Старые reader без этого изменения закономерно отвергают новый профиль.

Весь новый путь доступен под существующим opt-in `experimental-snapshot-v5`;
`experimental-streaming-facts` остаётся совместимым feature-алиасом. MCP включает
snapshot-v5 своим обычным `experimental-snapshot-mcp`. Не нужно включать новый
алгоритм молча в старых командах: выбор делает имя команды/профиля.

Builder читает blobs по одному через ingest, коммитит sources **только в приватной
временной SQLite**, затем вызывает общий `streaming_facts::derive`. Этот же producer
использует новый offline verify. Ни build, ни verify не восстанавливают весь корпус
в памяти. Канонизация snapshot_id/facts_sha256 остаётся общей и неизменённой.

Новый profile, версия алгоритма, effective source/Git/facts/coverage/time limits,
fixed guards и выбор расширений входят в нормализованный build_parameters и затем
в прежний snapshot_id. Время сборки и фактическая длительность в ID не входят.
Повтор параметров даёт reuse; изменение фактического бюджета меняет ID. Reader
отвергает лишние/неизвестные параметры, неканоническую сериализацию, завышенные
потолки, несовпадение limits_draft и превышение сохранённых бюджетов фактов/источников.
Open не разбирает XML/BSL; SQL count/length не читает весь BLOB-корпус в Rust-память.

## Запуск

```sh
cargo build --locked --features experimental-snapshot-mcp --bin bslens-v5 --bin bslens-mcp
# 4 бюджета источников/SQLite, 6 Git, затем facts / coverage bytes / facts time ms:
./target/debug/bslens-v5 build-stream \
  /absolute/synthetic-repo FULL_COMMIT EXPECTED_TREE synthetic:demo /absolute/output \
  5000 8388608 67108864 134217728 \
  10000 100 12000 8388608 70000000 30000 \
  100000 1048576 30000

# Репозиторий после build не нужен. SHA получают независимо доверенным способом.
./target/debug/bslens-v5 open /absolute/output/ID.SHA.sqlite \
  --staging-dir /absolute/private/staging --max-sqlite-bytes 134217728 --expected-sha256 SHA
./target/debug/bslens-v5 verify /absolute/output/ID.SHA.sqlite \
  --staging-dir /absolute/private/staging --max-sqlite-bytes 134217728 --expected-sha256 SHA \
  --max-sources 5000 --max-source-bytes 8388608 --max-total-bytes 67108864 \
  --max-scratch-bytes 134217728 --max-facts 100000 --max-coverage-bytes 1048576 --facts-time-ms 30000
./target/debug/bslens-mcp --snapshot /absolute/output/ID.SHA.sqlite \
  --staging-dir /absolute/private/staging --max-sqlite-bytes 134217728 --expected-sha256 SHA
```

Числа — синтетические бюджеты, не рекомендуемый промышленный профиль. Все обязательны.
Сохраняются [потолки producer](adr-012-streaming-facts-layer.md#вход-атомарность-и-ограничения)
и [ingest](adr-011-s8-ingest.md#лимиты-и-ошибки): источники до 100000 / 16 MiB одного /
512 MiB суммарно; SQLite до 1 GiB; facts до 2000000; coverage до 1 MiB; время фактов
до 300 секунд. Проверяются целые milliseconds без потери точности сериализации.
XML по-прежнему имеет 2 MiB/event/node/depth пределы, BSL lexer — 1 MiB и свои
ограничения. Поэтому 4 MiB BSL сохраняется как unparsed/Limit с точными source_lines,
а не объявляется разобранным. **NestedUnsupported остаётся строгим отказом.**

Git deadline и время расчёта фактов — отдельные фазы. Facts deadline кооперативный,
не прерывает отдельный SQL-вызов или bounded разбор; он не является общим hard
wall-clock timeout публикации, хеширования или копирования verify. Ограничения
процесса RSS/CPU и промышленная производительность не заявляются.

## Проверка и отказ

`verify_streaming` требует **доверенные лимиты вызывающего**; обычный `verify(Limits)`
для нового профиля возвращает отказ о необходимых бюджетах. CLI требует три новых
параметра только для stream-профиля, а на legacy отвергает их как неприменимые.
Бюджеты снимка не подменяют политику verify. После проверки SHA/blob OID по saved
sources scratch-транзакция коммитится, общий producer воспроизводит факты и coverage,
которые сравниваются с сохранёнными. Git provenance остаётся not_verified.

После позднего отказа ingest, source/facts/coverage/SQLite budget или строгого XML
вся собственная временная SQLite удаляется; публичного снимка нет. Промежуточный
commit sources внутри приватного файла не означает публикацию. Публикация/reuse
используют прежний атомарный S3 код и его отдельную ошибку PublishedSync после rename.
Verify закрывает scratch раньше удаления, обе сессии reader остаются независимыми.
Staging, SHA до SQLite, standalone header, read-only/query_only/trusted_schema,
точные UTF-8/Base64 и общий MCP frame 65536 сохраняются. Артефакты должны быть
доверенными; общей защиты от произвольного враждебного SQLite не заявляется.

## Фактически проверяемая приёмка

```sh
cargo test --locked --all-features --test mcp_snapshot stream_cli -- --nocapture
cargo test --locked
cargo test --locked --all-features
cargo fmt --all -- --check
git diff --check
```

Сохранённый тест `stream_cli_selected_count_and_bytes_offline_verify_real_mcp`:

- На малой общей фикстуре реальный CLI build-stream получает тот же facts_sha256,
  что старый builder; profile и snapshot_id различаются.
- Count: добавлены 4200 **выбранных** пустых BSL (вместе с базовыми — 4209 источников).
  Bytes: отдельно 9 **выбранных** BSL по 4 MiB; источников мало, общая сумма >32 MiB.
  Старый CLI build отказывает; build-stream успешно публикует оба снимка.
- Для обоих: удаление только одноразового Git-репозитория → CLI open/verify с пустым
  PATH → настоящий stdio MCP handshake, metadata/tree/source. Побайтово сверяются
  BOM/CRLF/последняя строка без LF и invalid UTF-8 через Base64; проверены
  accepted-zero и unparsed/Limit.
- Повторный build даёт reuse; смена facts-budget меняет identity. Отказы count,
  bytes, facts, SQLite и NestedUnsupported не оставляют опубликованных/временных файлов.
- Verify отказывает без новых бюджетов и при слишком малом facts-budget. Формально
  целый изменённый метод с пересчитанными facts/file SHA проходит open и отвергается
  verify. Неизвестные/завышенные параметры и DDL/profile mismatch отвергаются на open.

Это синтетическая сквозная проверка, не приёмка 188 MB БСП. Общий лимит выбранного
корпуса 4096/32 MiB преодолён в новом режиме; структурный блокер NestedUnsupported
не устранён. Рабочие базы, репозиторий пользователя, БСП/ERP и NucBox не использовались.

Результат облачного Linux x86_64 / Rust 1.85.1: locked default — 1 passed,
locked all-features — 132 passed; fmt/diff-check — успешно, предупреждений нет.
Документированная сборка только с experimental-snapshot-mcp прошла. После добавления
независимых SQL assertions count/total и проверки числа модулей через MCP сквозной
тест повторён: 1 passed (53.69 s). Полные проверки со staging выполнялись в том же
облачном Linux вне UID-remapping sandbox, без ослабления проверок путей/владельцев.
