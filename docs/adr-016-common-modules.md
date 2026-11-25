# ADR-016: CommonModule в отдельном Linux v5 профиле

Основа — локальный companion-кандидат 02af451f59378a82f3d59e4c3e36de9e9edcbc52.
Новая явная команда `build-common-modules`, профиль `linux-v5-common1-xml221-r09`.
Старые команды и профили не принимают CommonModule; их параметры, ограничения,
DDL и канонизация фактов сохранены. Новый профиль использует таблицы companion1,
меняя только точный CHECK профиля в metadata. Формат SQLite остаётся v5. Новые
таблицы и зависимости не нужны. В identity входят профиль, нормализованные лимиты
и `common_module_guard=xml221-required-module-v1`; смысл старого snapshot_id прежний.

## Поддерживаемый контракт

Подтверждение от локальной read-only проверки закреплённой БСП: 804 пары
`CommonModules/<Name>.xml` и `CommonModules/<Name>/Ext/Module.bsl`, без сирот и
пропусков; XML 2.21, без ChildObjects. Реальные имена и тела сюда не переносились.
Синтетический тест использует тот же структурный контракт; настоящая БСП здесь
не запускалась и не объявляется принятой.

Configuration/ChildObjects содержит скалярную MDClasses-ссылку CommonModule.
Дескриптор — MetaDataObject MDClasses v2.21, один CommonModule с валидным
unqualified uuid, одно прямое Properties с одним скалярным Name, совпадающим со
ссылкой. ChildObjects запрещён, даже пустой. Уникальность UUID проверяется вместе
с остальными объектами. Прочие семантические свойства, в том числе серверные и
клиентские флаги, остаются unverified: никаких выводов о контексте исполнения.

Пара обязательна именно в этом профиле: отсутствие дескриптора или Module.bsl,
неразрешённый CommonModules XML, сиротский/дополнительный BSL в CommonModules,
неверное имя/тип/версия/namespace/UUID, повторная ссылка — строгий отказ. Legacy
поведение неподтверждённых модулей вне этой области не меняется. Формы и макеты
сохраняют ранее согласованные правила: BSL формы и payload не обязательны для
структурной навигации, содержимое payload не анализируется.

Объект CommonModule — непосредственный ребёнок Configuration. Владелец его
`Ext/Module.bsl` — этот объект, не Configuration и не другой общий модуль.
Исходники XML/BSL, OID/SHA и физические диапазоны сохраняются без преобразования.
Лексический отказ не является отсутствием исходника: status=unparsed, reason
сохраняется, методов нет, source выдаёт точные физические строки. UTF-8 возвращается
без преобразования, невалидный UTF-8 — явно Base64. accepted с нулём методов остаётся
отдельным состоянием. Семантических связей и Git provenance offline verify не даёт.

## Ограничения и атомарность

Writer и offline verify используют общий пофайловый producer из SQLite sources.
Старые бюджеты файлов, суммарного ввода, фактов, coverage, SQLite и кооперативного
времени не увеличены. При позднем отказе итоговый снимок не публикуется. Reader
не разбирает BSL при open; MCP использует одно immutable соединение на сессию.
Остаются private Linux staging, независимый expected SHA, bounded tree/source и
полный JSON-RPC budget 65536 bytes. Reader предназначен для доверенных артефактов,
не является sandbox произвольного SQLite. Проверенная платформа — Linux x86_64.

## Команды и проверки

```sh
cargo +1.85.1 test --locked
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --all-features --test mcp_snapshot common_modules_cli
cargo +1.85.1 fmt --check
git diff --check
```

Не запускать default/all-features одновременно в одном CARGO_TARGET_DIR: общий
CLI-путь может быть заменён другой сборкой во время integration test.

Новая команда принимает те же 18 аргументов, что build-companions:

```sh
bslens-v5 build-common-modules "$REPO" "$COMMIT" "$TREE" "$SOURCE_ID" "$OUTPUT" \
 "$MAX_SOURCES" "$MAX_SOURCE_BYTES" "$MAX_TOTAL_BYTES" "$MAX_SQLITE_BYTES" \
 "$GIT_ENTRIES" "$GIT_TREES" "$GIT_READS" "$GIT_OBJECT_BYTES" "$GIT_TOTAL_BYTES" "$GIT_TIME_MS" \
 "$MAX_FACTS" "$MAX_COVERAGE_BYTES" "$FACTS_TIME_MS"
```

Порядок локального verify/MCP и требования переноса — в
[руководстве NucBox](nucbox-common-diagnostic.md). Рабочий репозиторий не удалять.
Только синтетический integration test удаляет созданный им Git, затем запускает
настоящие verify/open и stdio MCP с пустым PATH. Он проверяет дерево/владельца,
точные bytes метода, BOM/CRLF/последнюю строку без LF, невалидный UTF-8 и диапазон
unparsed, parity старых фактов по реальным адресам, детерминизм, строгие отказы и
rollback. Формально целый снимок с неверным владельцем и пересчитанными hashes
open принимает, offline verify отвергает.

Это очередной структурный срез, не полная приёмка Навигатора. Остальные корневые
типы БСП и содержимое форм/макетов не реализованы этим шагом. Остальные семь
разрезов также не относятся к нему. Штатные CI GitHub здесь не запускались.
