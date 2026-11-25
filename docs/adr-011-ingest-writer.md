# Узкий срез: bounded Git ingest → writer v5

Основа: draft #8 `a0dea54a9d139d766b425bda3d23da8c7365c209`.
Переиспользован ingest из #5 `853f4720879d2f682f10617604deb295486d5a94`.
Только облачный Linux x86_64 и синтетические входы. Это кандидат для независимого
ревью; не приёмка БСП/ERP. #5/#6 и main не изменяются.

## Что изменилось

Явная команда `build-ingest` и API `snapshot_v5::ingest_writer::build` записывают
выбранные Git blobs по одному в sources приватной SQLite-транзакции. Legacy
`build`, R09, XML/лексический парсеры и их пределы не переключены на другой алгоритм.
`experimental-snapshot-v5` теперь включает библиотечный `experimental-v5-ingest`,
но чтение Git запускается только при явном вызове builder. Reader/MCP Git не вызывают.
Новых зависимостей нет.

**Большим может быть inventory Git, но не выбранный XML/BSL-корпус.** Новый режим
проходит дерево более чем с 4096 файлами и с более 32 MiB невыбранных тел. Он
инвентаризирует невыбранные записи, не читает и не проверяет их blob-тела.
В input_manifest это явно `NotXmlOrBslNotRead`. Исходный OID дерева проверен;
это не fsck всех объектов и не доказательство Git-происхождения при offline verify.

Общий `derive_facts` остаётся прежним: максимум 4096 выбранных XML/BSL и 32 MiB
их суммарных тел, с материализацией ограниченного выбранного корпуса в памяти.
Эти потолки не повышены. XML 2.21 по-прежнему может отказать с NestedUnsupported.
Полноценный пофайловый producer фактов и поддержка вложенного XML — следующие
самостоятельные шаги. Успех на большом inventory не доказывает работу полной БСП.

## Профиль, схема и идентичность

Новый профиль: `linux-v5-ingest1-r09-facts`. Исходный `spikes/linux-v5/schema.sql`
сохранён побайтно. Новый точный вариант DDL получается заменой только CHECK имени
профиля. User_version остаётся 5; таблицы, индексы и колонки те же. Это отдельный
известный вариант схемы, не миграция существующих снимков. Старый reader закономерно
не принимает его; обновлённый reader принимает оба точных варианта и требует
соответствующее значение profile. Лишние views/triggers/DDL по-прежнему отвергаются
ограниченным валидатором. Сначала проверяется SHA/standalone header, как в S4.

Канонизация snapshot_id/facts_sha256 не изменена и не дублируется. Новый profile и
нормализованные build_parameters входят в прежний snapshot_id: четыре writer-лимита,
шесть Git-лимитов (deadline в целых ms), fixed depth/path/layout, выбор расширений,
нечтение excluded, версии guards, прежние XML/лексические потолки и временное покрытие.
Изменение параметров меняет ID; повтор тех же параметров даёт reuse при тех же фактах.
Reader отвергает неизвестные/лишние параметры, неканоническую сериализацию,
запредельные бюджеты и несогласованный limits_draft. Пределы источников проверяет
через count/length, без загрузки BLOB-тел. Reader сообщает фактический profile.

Offline verify использует прежний общий derive_facts и пересчитывает SHA/OID
сохранённых источников. Структура и producer фактов здесь одинаковы у обоих
профилей, поэтому отдельного парсера или второго алгоритма verify нет. Git ему
не нужен; `git_provenance=not_verified`, `relations=not_analyzed` сохраняются.
Сохранённые бюджеты не заменяют доверенные лимиты reader/verify/MCP.

## Лимиты, запуск и rollback

Все параметры обязательны, defaults отсутствуют:

```sh
cargo build --locked --features experimental-snapshot-mcp --bin bslens-v5 --bin bslens-mcp
./target/debug/bslens-v5 build-ingest \
  /absolute/synthetic-repo FULL_COMMIT EXPECTED_TREE synthetic:demo /absolute/output \
  100 1000000 4000000 4000000 \
  5000 100 5000 1000000 4000000 30000
```

Первые четыре числа: выбранные sources count / bytes одного / total bytes / SQLite
bytes. Их потолки: 4096 / 16 MiB / 32 MiB / 1 GiB. Последний — ограничение
эксперимента, не обещание пригодности для промышленного корпуса. XML имеет свой
предел 2 MiB; lexical — 1 MiB (большой BSL сохраняется с `unparsed`). SQLite
ограничивается полными страницами 4096 bytes; меньший бюджет даёт отказ.

Последние шесть: Git entries / trees / reads / object bytes / total read bytes /
deadline ms. Потолки [точно те же, что у #5](adr-011-s8-ingest.md#лимиты-и-ошибки).
Сумма Git включает commit/tree и выбранные blobs, повторяющийся OID учитывается
за каждый путь. Deadline ограничивает ingest, но не последующий расчёт фактов.
Произвольные Git/SQLite и hostile filesystem не становятся доверенными от этих
проверок; [граница reader из S4](adr-010-s4.md#граница-защиты-sqlite) сохраняется.

Ни один снимок не публикуется до успешного Receipt, расчёта фактов, проверки
контракта и закрытия SQLite. Ошибка позднего blob, count/bytes, producer или SQLite
откатывает транзакцию и удаляет собственный временный файл. Для SQLITE_FULL сохраняется
ошибка «диск или лимит страниц», без ложного различения ENOSPC и page cap.
Публикация использует прежний S3 atomic publish/reuse и его отдельный PublishedSync
для случая уже выполненного rename с последующим отказом fsync.

После build используйте обычные `verify` и `bslens-mcp --snapshot` из README с
независимо переданным expected SHA и приватным staging. Репозиторий после build
не требуется; MCP сохраняет прежний wire budget 65536 и точные UTF-8/Base64 bytes.

## Воспроизводимая синтетическая проверка

```sh
cargo test --locked --all-features --test mcp_snapshot ingest_
cargo test --locked
cargo test --locked --all-features
cargo fmt --all -- --check
git diff --check
```

В `actual_ingest_writer_large_inventory_build_verify_mcp_and_rollback` добавляются
пять невыбранных blobs по 7 MiB (старый Git отказывает ещё до увеличения числа
путей), затем 4200 невыбранных файлов. Новый CLI проходит. Выбранные источники
малы: supported XML, обычный/непринятый BSL, invalid UTF-8, BOM/CRLF/последняя
строка без LF. После удаления только созданного тестом Git-репозитория реальный
CLI verify и stdio MCP работают с пустым PATH. Проверены handshake, порядок/форма
дерева относительно малого R09-эталона, unparsed/accepted-zero, exact source bytes.

Проверены reuse и изменение ID при изменении бюджета, поздний отказ count после
первой вставки, отказ Git на последнем выбранном blob, SQLite page cap и пустые
каталоги отказавших сборок. Второй тест отвергает лишние/запредельные параметры,
limits_draft mismatch и несоответствие profile точному DDL с корректным file SHA.
Этот новый сценарий — сохранённый интеграционный тест, в отличие от ручного smoke
при подготовке #8. Workflow из main не меняется: PR в ветку #8 автоматически не
попадает под его фильтр `pull_request: main`.

Не проверены здесь: полный выбранный корпус сверх 4096/32 MiB, реальные БСП/ERP,
промышленные RSS/CPU/диск, Windows/ARM, новый XML producer. #6 с замерами не включён.

Результат облачного прогона Rust 1.85.1: default — 1 passed, all-features — 124
passed, fmt/diff-check — успешно. После добавления явного отказа MAX_SOURCES=4097
оба ingest-интеграционных теста повторены: 2 passed. Staging-проверки владельцев
выполнялись вне UID-remapping sandbox в том же облачном Linux, без их ослабления.
