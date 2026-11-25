# Inline XML: первый структурный срез, не полная приёмка БСП

Основа неизменна: `ed25bac0a0a3f680c3c01849d3464f8ec49ddb77`.
Работа ведётся в отдельном локальном кандидате без публикации и live-прогона.
Источник знания о форме — переданная агрегированная read-only диагностика на
закреплённой БСП (Configuration и 400 дескрипторов); реальные значения/файлы сюда
не переносились. Все новые фикстуры синтетические. Из восьми разрезов BSLens этот
шаг относится только к первому, структурному; поэтапный анализ не реализуется.

## Поддержанное подмножество

| Непосредственный родитель | Принимаемые inline-объявления в ChildObjects |
|---|---|
| Catalog, Document | Attribute, Command, TabularSection |
| InformationRegister | Attribute, Command, Dimension, Resource |
| TabularSection внутри Catalog/Document | только Attribute |

MDClasses namespace, корректный unqualified uuid, один прямой Properties,
один скалярный непустой Name обязательны. Name сохраняется без нормализации;
лимит 1024 UTF-8 bytes, элементы/атрибуты внутри Name отвергаются. Вложенность
глубже реквизита табличной части не принимается. TabularSection требует один
непустой ChildObjects без атрибутов/постороннего текста. Дубликаты UUID, точных имён
одного вида у одного непосредственного родителя и неверная форма дают отказ.

У наблюдавшихся TabularSection есть дополнительный прямой элемент кроме
Properties/ChildObjects. Его семантика **не известна**. Дополнительные свойства,
атрибуты и такие элементы сохраняются в исходнике, отмечаются в xml_unverified;
они не становятся разобранными фактами. Узел означает только структурное
объявление. В coverage и MCP остаётся `semantics/inline_semantics=not_analyzed`.

Form и Template в ChildObjects — ссылки на companions, а не inline-объявления.
В этом срезе они дают строгий NestedUnsupported. Неизвестные дочерние теги и теги
в неподходящей роли дают UnsupportedType/Namespace/Shape. Не подменяются пропуском.
Остальные корневые типы не добавлены: в Configuration поддержаны только Catalog,
Document, InformationRegister. Любые иные, например CommonModule, Enum, Constant,
DataProcessor, Report, ChartOfAccounts, AccumulationRegister, AccountingRegister,
CalculationRegister, остаются вне профиля. Это перечень ограничений реализации,
а не утверждение об обследованных значениях конкретного корпуса.

## Профиль и адрес доказательства

Новая команда `build-inline`, профиль **linux-v5-inline1-xml221-r09**. Старые
build/build-ingest/build-stream, их DDL, fingerprints и поведение сохранены.
Базовый schema.sql не изменён. Новый точный DDL добавляет таблицу
`inline_declarations`, индексы ветви/уникальности и отдельный CHECK профиля.
Schema version остаётся 5; профиль точно определяет вариант схемы. Старые readers
его не принимают. Обратной миграции или подмены старого профиля нет.

Поля: opaque declaration_key, owner_id корневого объекта, parent_key непосредственного
inline-родителя (NULL означает сам корневой объект), kind/name/uuid, **настоящий**
xml_path, byte_start/byte_end. Адрес `path#uuid` используется только как opaque key,
никогда как файл. FK xml_path/source_kind указывает на исходный XML в sources;
composite FK parent_key/owner_id не даёт пересадить ребёнка к другому владельцу.
Диапазон — исходные bytes, 0-based/end-exclusive, с учётом трёх bytes UTF-8 BOM.
Источники сохраняются целиком; owner BSL остаётся корневым объектом. Новые пути
модулей команд не угадываются: вне прежнего набора путей BSL остаётся unconfirmed,
но его bytes не теряются. complete_branch означает полноту выбранной SQL-ветви,
а не полноту знаний о БСП.

Канонизация старых фактов не меняется. Для таблицы inline используется отдельный
домен `BSLens-v5/facts/inline1`, включающий kind/name/uuid, реальный XML, parent key,
диапазон и стабильный XML-ключ владельца вместо surrogate ID. Build и verify используют
один producer. Новые guards/companion refusal входят в normalized parameters и ID.
Смена родителя при формально корректных SHA проходит open, но отвергается verify.
Open проверяет структуру/хеши, не повторяет XML-разбор и не утверждает происхождение Git.

## Навигация и ограничения

CLI build-inline принимает те же обязательные бюджеты, что build-stream.
CLI open/verify автоматически распознают профиль; verify требует source/facts/
coverage/time limits вызывающего. Пример команд и перенос — в
[инструкции NucBox](nucbox-inline-diagnostic.md).

MCP остаётся stdio read-only с тремя инструментами. Для нового профиля:

- object-ветвь содержит группу «Встроенные объявления»;
- tree `level=inline`, `parent_id=<owner>` возвращает только непосредственные
  объявления; `declaration_parent=<opaque key>` раскрывает реквизиты табличной части;
- source `{ "declaration_key": "<opaque key>" }` возвращает точные XML bytes узла,
  реальный path, start_byte/end_byte, encoding и SHA. UTF-8/Base64 не lossy.

Row/text preflight ограничивает ветвь до загрузки строк; полный JSON-RPC cap 65536
сохраняется. XML-узел больше source budget даёт отказ без частичного payload; можно
выбрать меньший дочерний узел. Старые профили не рекламируют новые аргументы и
отвергают их. Методы/модули BSL продолжают использовать прежние физические строки.
Необработанные semantic properties не попадают в wire metadata; передаются счётчик
inline, временная сводка и hash покрытия. Нет заявления о полноте семантики.

Все лимиты stream producer сохраняются. Каждое inline-объявление и замечание
coverage учитываются в facts budget. XML bytes/nodes/events/depth ограничены;
время кооперативное, не hard CPU/RSS sandbox. При неизвестной структуре, лимите или
SQLite ошибке приватная сборка удаляется до публикации. Требование доверенного
SQLite и независимого expected SHA остаётся в силе.

## Синтетические проверки

```sh
cargo test --locked --all-features inline_ -- --nocapture
cargo test --locked
cargo test --locked --all-features
cargo fmt --all -- --check
git diff --check
```

Unit matrix: все поддержанные роли, реквизит TabularSection, дополнительный
семантический элемент как unverified, детерминированные records, неизвестные теги,
Form/Template, неверный namespace/uuid/Name/Properties, дубликаты имён, неправильный
родитель и запрещённая глубина.

Фактические CLI/MCP: 12 объявлений в трёх типах, повторная сборка/reuse, старый
stream-профиль по-прежнему отказывает, parity старых root/module/method/line фактов,
удаление только синтетического Git → verify/open → stdio handshake/tree/source.
BOM и Unicode XML slice сверен с сохранёнными bytes; BSL byte slice не меняется.
Form/Template/неизвестный/неверный XML оставляют пустые output-каталоги. Проверены
бюджет ветви, ошибочный непосредственный parent с пересчитанными SHA и отказ verify.
Полная БСП здесь не запускалась и не принималась.

Итог облачной синтетики, Linux x86_64 / Rust 1.85.1: locked default — **1 passed**,
locked all-features — **135 passed**, fmt/diff-check и сборка documented CLI/MCP
успешны, предупреждений нет. Регрессия классификации schema_field_budget исправлена
и проверена неизменённым старым тестом; каталоговые лимиты не ослаблялись. Полный
набор со staging выполнялся в том же облачном Linux вне UID-remapping sandbox,
без ослабления проверок путей/владельцев. Проверка NucBox/БСП не выполнялась.
