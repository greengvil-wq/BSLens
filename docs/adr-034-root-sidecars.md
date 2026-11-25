# ADR-034: корневые XML sidecar как непрозрачные источники

Основа `67828a09b19ecbc79ed56726f54d503e56caa92c`. Новый явный
`build-root-sidecars`, профиль `linux-v5-root-sidecar1-xml221-r09` наследует
optional-form1. Старые профили и правила классификации остаются прежними.

Переданная локальная сверка закреплённой БСП: 5212 источников классифицировались
как xml, но реальных descriptors 5078. Разница 134: Catalog Help.xml 73,
Document Help.xml 19, InformationRegister Help.xml 28 и Catalog Predefined.xml 14.
Это свидетельство из NucBox, не облачный прогон БСП. Реальные имена/данные сюда
не переносились. Заявленные 12496 выбранных источников не пересчитывались в облаке.

## Только четыре формы пути

```text
Catalogs/<Owner>/Ext/Help.xml
Documents/<Owner>/Ext/Help.xml
InformationRegisters/<Owner>/Ext/Help.xml
Catalogs/<Owner>/Ext/Predefined.xml
```

Ровно четыре сегмента, буквальный Ext в третьей позиции, точный регистр имени
файла, допустимый непустой Owner. Имя владельца Ext разрешено. Общие безопасные
пути Git (без symlink, пустых/точечных сегментов и обхода родителей) сохраняются.
Никаких дополнительных tails: Document/InformationRegister Predefined.xml,
Other.xml, другая глубина и иные неизвестные XML не получают исключения.

Селектор **не меняется**: эти файлы уже выбраны как .xml. Только их kind в sources
становится payload. Все другие пути проходят прежний classifier. Общая функция
используется при build и offline verify. Layout XML не разбирается как MDClasses,
Help/Predefined не считается семантически понятым; даже невалидный XML остаётся
точным непрозрачным BLOB. Source/object/SQLite/time quotas не увеличиваются.

Каждый файл получает companion_payloads(path, object_id, role='additional'), где
object_id — подтверждённый реальный корень Catalog/Document/InformationRegister.
Это не форма и не Configuration. Отсутствующий в Configuration/неверный descriptor
не создаёт фиктивного владельца; orphan payload отвергается общей проверкой.
Файлы необязательны: отсутствие не заменяется пустым источником. Нет тихого
пропуска: все выбранные payload должны получить владельца, а число xml должно
по-прежнему точно равняться числу выведенных objects. Строгий
companion_descriptor_inventory сохранён.

## HTML, counts и identity

Корневые `<Owner>/Ext/Help/ru.html` этих трёх семейств остаются вне согласованного
scope: не читаются, не сохраняются и не обещаются как покрытые. По переданной
диагностике БСП их 120. Полная пара Help XML/HTML здесь **не требуется**, поскольку
HTML намеренно не выбран. Это не меняет прежние правила справки форм, Reports,
DataProcessors и других ранее поддержанных семейств.

`input_manifest.selected/excluded` сохраняет прежний список path/mode/blob_oid;
для HTML причина OutsideSelectionNotRead. В coverage_draft добавлена ограниченная
root_xml_sidecars со count/contents=not_analyzed/help_html=outside_scope_not_read.
Count проверяется по сохранённым companion_payloads при open и воспроизводится
при verify. MCP показывает root_xml_sidecars_count и
sidecar_help_html=outside_scope_not_read. Этот маркер относится только к scope
sidecar данного среза, не ко всей ранее поддержанной справке.

DDL меняет только profile discriminator. В build_parameters включён
root_sidecars_guard=catalog-document-register-root-xml-opaque-v1; guard и profile
участвуют в snapshot_id. Точные bytes/OID/SHA остаются в sources; owner/role —
в канонических facts. Подмена owner с пересчитанными facts/file SHA может пройти
формальный open, но отвергается пересчётом offline verify. Git provenance не
проверяется. Diagnostics SQLite storage, строковые/суммарные квоты, ограниченная
metadata и wire budget сохраняются.

## Запуск и синтетическая проверка

Команда — как в [ADR-032](adr-032-optional-form.md), заменить только
build-optional-form на **build-root-sidecars**. Все 21 аргумент после имени команды
прежние, включая MAX_OPAQUE_BIN_BYTES и MAX_DIAGNOSTICS_BYTES. Verify и stdio MCP
по [ADR-031](adr-031-diagnostics.md), с независимым expected SHA и trusted staging.
Рабочий Git/БСП на NucBox не удалять; реальный повтор нужен отдельно.

Тест root_sidecars_cli_offline_stdio_owner_inventory_html_scope_and_tamper:
реальный CLI → удаление собственного синтетического Git → offline verify → stdio
с пустым PATH. 12 selected, 3 excluded HTML, 7 descriptors/objects, 5 payload
(4 новых sidecar и layout optional-формы). Проверены owner с именем Ext, точные
BOM/CRLF XML payload bytes по диапазону, additional role/payload_presence,
сохранённое отсутствие BSL optional-формы, детерминизм, старый отказ, неверный
вид/путь/глубина, сирота, rollback и подмена owner с согласованными digest.

```sh
cargo +1.85.1 test --locked --features experimental-snapshot-mcp root_sidecars_cli -- --nocapture
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
```

Это синтетический кандидат Linux x86_64, не приёмка полной БСП. Main/GitHub и
реальные данные не затронуты; запреты неизвестных структур не ослаблены.
