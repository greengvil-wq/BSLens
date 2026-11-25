# ADR-028: отдельный ограниченный бюджет непрозрачного BIN

Основа `8ba424b08f0406cb65c68d6ebed95f3d53d088e9`. Новый явный режим
`build-corpus-bin`, профиль `linux-v5-corpus-bin1-xml221-r09`, Linux x86_64.
Старые команды и профили, включая corpus1, сохраняют прежние пределы.
Схема наследует corpus1 с новым точным profile; таблицы и столбцы прежние.
Новых зависимостей нет. Профиль наследует 42 вида, Ext-locator, lookup и Recalculation.

Причина: переданная локальная диагностика БСП сообщила три выбранных BIN размером
9538006, 16405794 и 21561550 bytes, два последних — макеты. Это сведения постановки,
а не прочитанные в облаке реальные файлы. Синтетика воспроизводит эти размеры.
Полные тела остаются в `sources.content`, исключения из scope нет.

## Политика и владение

Последний аргумент `MAX_OPAQUE_BIN_BYTES` обязателен, диапазон 1…25165824 bytes
(24 MiB). Отдельный cap заменяет обычный per-file cap только для следующих путей:

* `CommonTemplates/<Name>/Ext/Template.bin`;
* `<Catalogs|Documents|InformationRegisters|DataProcessors|Reports>/<Owner>/Templates/<Name>/Ext/Template.bin`;
* `XDTOPackages/<Name>/Ext/Package.bin`;
* точный путь `.bin` из нормализованного `GLOBAL_OPAQUE_JSON`, только в корне
  или под `Ext/` согласно ADR-027.

Проверяется вся форма пути, не один суффикс. До чтения тела это кандидат по пути;
после сохранения общий producer проверяет реальный дескриптор, scalar-ссылку,
UUID/Name, обязательные файлы и владельца. Template.bin принадлежит Template/
CommonTemplate, Package.bin — XDTOPackage, глобальный BIN — Configuration.
Отсутствующий/неверный дескриптор и orphan не становятся успехом. Для нового
профиля выбранный `.bin` вне этой грамматики явно отвергается; прежние профили
не получают такого изменения. Payload непрозрачен, не XML и не BSL, не распаковывается.

## Все границы пути

1. Git ingest получает отдельный cap только для выбранного blob по этой грамматике.
   `cat-file --batch` проверяет OID, **тип blob**, размер и framing до выделения
   буфера тела. Cap+1 отвергается по заголовку. Commit/tree всегда используют
   обычный `GIT_OBJECT_BYTES`; его прежний потолок 16 MiB не повышен. Суммарный
   `GIT_TOTAL_BYTES`, entries/trees/reads, deadline и контроль фактического OID
   действуют для всех объектов. Хеш Git header+body теперь считается инкрементально,
   без второго Vec размером с blob; результат и identity старых профилей прежние.
2. Writer дополнительно проверяет source kind и cap, source_count/total/SQLite.
   Обычный `MAX_SOURCE_BYTES` (потолок 16 MiB) сохраняется для XML/BSL и иных payload.
   Структурные XML/BSL имеют также прежние parser/lexical пределы. Никакого общего
   повышения до 24 MiB нет. Данные вставляются по одному файлу, затем producer
   вычисляет факты из SQLite; BIN не перечитывается целиком для разбора.
3. `build_parameters.opaque_bin={"bytes":N,"grammar":"owned-template-xdto-global-bin-v1"}`
   входит в snapshot_id вместе с профилем, global_scope и нормализованными бюджетами.
   `limits_draft.source_bytes` остаётся обычным cap. Metadata validator проверяет
   отдельный cap и lengths всех sources по тем же правилам. Общий предел
   канонического build_parameters **16384 bytes** не повышен.
4. Offline verify требует **явный доверенный** `--max-opaque-bin-bytes N`:
   метаданные не назначают бюджет вызывающему. Без аргумента новый профиль отвергается;
   старый профиль не принимает этот параметр. Для BIN применяется cap вызывающего,
   для остальных — его обычный source cap. OID/SHA проверяются по сохранённым
   полным байтам, scratch заполняется incremental BLOB блоками 64 KiB. Далее
   единый producer пересчитывает владельцев, XML/BSL-факты и canonical facts.
   Наличие Git и исходного репозитория не требуется; Git provenance не утверждается.
5. Open и MCP поддерживают новый точный профиль. Вся БД ограничена доверенным
   max-sqlite-bytes и проходит прежние checksum/header/schema/facts проверки.
   MCP читает payload по byte ranges через incremental BLOB. Reader limits,
   128 узлов и полный JSON-RPC **65536 bytes** прежние. Base64 expansion/envelope
   учитываются; слишком большой ответ отвергается целиком, без частичных bytes.

Все отказавшие build удаляют собственную временную БД и не публикуют снимок,
включая поздний отказ фактов/дескриптора и ENOSPC/SQLite page cap. Ошибку нельзя
обходить исключением больших BIN из списка.

## Команды

```sh
cargo +1.85.1 build --locked --features experimental-snapshot-mcp --bins
./target/debug/bslens-v5 build-corpus-bin "$BSP_REPO" "$BSP_COMMIT" "$BSP_TREE" "$SOURCE_ID" "$OUTPUT" \
 "$MAX_SOURCES" "$MAX_SOURCE_BYTES" "$MAX_TOTAL_BYTES" "$MAX_SQLITE_BYTES" \
 "$GIT_ENTRIES" "$GIT_TREES" "$GIT_READS" "$GIT_OBJECT_BYTES" "$GIT_TOTAL_BYTES" "$GIT_TIME_MS" \
 "$MAX_FACTS" "$MAX_COVERAGE_BYTES" "$FACTS_TIME_MS" "$GLOBAL_OPAQUE_JSON" 25165824
V5="$PWD/target/debug/bslens-v5"
MCP="$PWD/target/debug/bslens-mcp"
env PATH= "$V5" verify "$SNAPSHOT" --staging-dir "$STAGING" --max-sqlite-bytes "$MAX_SQLITE_BYTES" --expected-sha256 "$EXPECTED_SHA" \
 --max-sources "$MAX_SOURCES" --max-source-bytes "$MAX_SOURCE_BYTES" --max-total-bytes "$MAX_TOTAL_BYTES" --max-scratch-bytes "$MAX_SCRATCH_BYTES" \
 --max-facts "$MAX_FACTS" --max-coverage-bytes "$MAX_COVERAGE_BYTES" --facts-time-ms "$FACTS_TIME_MS" --max-opaque-bin-bytes 25165824
env PATH= "$MCP" --snapshot "$SNAPSHOT" --staging-dir "$STAGING" --max-sqlite-bytes "$MAX_SQLITE_BYTES" --expected-sha256 "$EXPECTED_SHA"
```

Все переменные, полные pinned commit/tree, приватные output/staging и независимый
expected SHA согласуются локально; здесь команды БСП не выполнялись.
Общие квоты не следует копировать из синтетики как производственный профиль.
[Перенос исходников и stdio handshake](nucbox-corpus-diagnostic.md).
Для payload использовать возвращённый saved_path и небольшие `[start_byte,end_byte)`;
декодировать Base64 без преобразования bytes. Рабочий Git БСП не удалять.

## Проверки и память

Синтетический тест `corpus_bin_cli_large_owned_payload_caps_offline_verify_stdio`
создаёт два Template.bin и глобальный BIN указанных размеров, малый Package.bin и
BSL. Обычные Git/source caps — 8 MiB, BIN — 24 MiB, source total — 64 MiB,
Git total/SQLite/scratch — 128 MiB. Проверяются полный length, hashes, reuse,
изменение identity при изменении BIN cap, одинаковые факты при допустимых caps,
реальный CLI → удаление собственного synthetic Git → offline verify → stdio
metadata/tree/source, владельцы и точные head/tail bytes, BSL и отсутствие Git.

Отказы: старый профиль, обычный commit cap, попытка поднять обычные caps до 24 MiB,
BIN policy cap+1, тело ровно 24 MiB / 24 MiB+1, неподдержанный путь/дескриптор,
большой Template.xml (не получает BIN cap), недостаточные total/facts/SQLite квоты,
отсутствующий/малый verify BIN budget и слишком длинный MCP ответ. Наследованный
полный набор продолжает проверять legacy, Ext, 16 KiB parameters, ENOSPC и lookup.

Точный **лимит тела** одного специального файла — 25165824 bytes, не сумма BIN.
Rust ingest держит одно тело плюс batch header/framing до 129 bytes; Git OID и
SHA не создают второй body-sized Vec. SQLite может копировать связанный BLOB и
создавать свои buffers/cache, Git — свои буферы, поэтому 24 MiB **не является
лимитом RSS процесса**. Общие file/total/page/time квоты не заменяют OS memory limit.
Verify использует один собственный буфер 65536 bytes для переноса/хешей BIN,
плюс SQLite cache; payload не разбирается и не материализуется суммарно.

Пиковый RSS каждого CLI измеряется отдельно внешним harness через Linux
`resource.getrusage(RUSAGE_CHILDREN).ru_maxrss`, KiB. Внешний wrapper необязателен
и не входит в продукт: тест получает его через `BSLENS_TEST_RSS_WRAPPER`; без
переменной запускается обычный Rust CLI. На финальном полном прогоне: обычная положительная сборка 71312 KiB,
сборка тела ровно 24 MiB — 82000 KiB, verify — 10660/10816 KiB
(граничный/основной снимок). До чтения cap+1 отказал при 8192 KiB.
Итоговые logs передаются с exact SHA. Это наблюдения на облачном Linux x86_64, не гарантия RSS для любого
SQLite/allocator/репозитория. Source-only архив не содержит сгенерированные
большие BIN и не растёт на их суммарный размер.

```sh
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
cargo +1.85.1 test --locked --features experimental-snapshot-mcp --test mcp_snapshot corpus_bin_cli
```

Реальная БСП, NucBox, main и GitHub не затронуты. Синтетический успех не является
приёмкой БСП и не означает интерпретацию содержимого BIN.
