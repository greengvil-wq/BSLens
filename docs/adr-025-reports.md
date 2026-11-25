# ADR-025: DataProcessor и Report

Основа `aa464ac79b9152693de819b76eab56b3c7ece55f`. Новый opt-in `build-reports`,
профиль `linux-v5-report1-xml221-r09`, Linux x86_64. Предыдущие профили не расширяются.
`report_guard=processor-report-assets-v1` и нормализованные бюджеты входят в identity.

## Структура и исходники

Явные локаторы: DataProcessor → `DataProcessors/<Name>.xml`,
Report → `Reports/<Name>.xml`. Дескрипторы MDClasses/MetaDataObject 2.21 имеют
правильный прямой тип, unqualified UUID и scalar Properties/Name. Корневые
ChildObjects допускают Attribute, Command, TabularSection и scalar Form/Template.
Пустой ChildObjects допустим. TabularSection содержит один непустой ChildObjects
только с Attribute; листья не имеют ChildObjects. Неизвестные структурные теги,
роль, глубина, namespace, UUID/Name или дубли вызывают отказ.

InternalInfo у корня/TabularSection и Properties помимо Name остаются unverified.
Нет анализа назначения команды, полей отчёта, обработчиков, СКД или иных свойств.
Каждый inline-узел сохраняет реальный XML path, UUID, диапазон исходных bytes с
BOM, owner_id корня и parent_key непосредственной табличной части. Ссылки
Configuration и Form/Template также имеют точные диапазоны в исходном XML.

ManagerModule и ObjectModule корня независимо необязательны. Для каждой inline
Command обязателен точный `Commands/<Name>/Ext/CommandModule.bsl`. Модуль связан
с Command через inline_module_owners; modules.owner_id остаётся ID контейнера.
Сирота или другая дорожка модуля отвергается. BSL-методы не приписываются команде
по совпадению имени. При unparsed сохраняются причина и исходные bytes без методов.

Form ref → `Forms/<Name>.xml` + обязательный `Forms/<Name>/Ext/Form.xml`;
`Forms/<Name>/Ext/Form/Module.bsl` необязателен. Справка корня и формы допускается
только полной парой `Ext/Help.xml` + `Ext/Help/ru.html`, либо отсутствует целиком.
Template ref → `Templates/<Name>.xml` + ровно один
`Templates/<Name>/Ext/Template.{xml,bin,txt}`. Ноль/несколько payload — отказ.
Только у DataProcessor допускается дополнительный `Ext/Template/ru.html`,
который требует Template.xml того же макета. XML payload не проверяется как
MDClasses/2.21: СКД, spreadsheet, Form и Help могут иметь иные схемы.

Только внутри формы DataProcessor разрешены дополнительные дорожки:

- `Forms/<Form>/Ext/Form/Items/<Item>/<File>.{zip,png,gif,svg}` — ровно два
  непустых сегмента после Items, точный регистр расширений;
- `Forms/<Form>/Ext/Help/<Catalog>/<File>.png` — ровно два сегмента после Help,
  с полной парой справки этой формы.

Второй путь трактуется в контексте формы из постановки задачи; его корневой аналог
и дополнительные ресурсы Report не включены в грамматику. Иные глубины,
расширения и владельцы отвергаются. Имена Item/Catalog не доказывают семантическую
связь с UI. Все assets сохраняются как payload с владельцем-формой; ZIP не
распаковывается, SVG/HTML не исполняются, бинарные и текстовые bytes выдаются
Base64 без lossy. Наличие ресурсов не означает, что содержимое проанализировано.

Основание — переданные обезличенные агрегаты pinned БСП: 154 обработки
(17 пустых ChildObjects; 442 Attribute, 78 Command, 20 TabularSection/168 Attribute,
422 Form, 89 Template), 52 отчёта (7 Attribute, 3 Command, 1 TabularSection/7 Attribute,
13 Form, 70 Template). BSL форм — 418/422 и 12/13; пары справки корней — 70/40,
форм — 131/1. Наблюдались 17 ZIP, 6 PNG, 1 GIF, 1 SVG в Items, 1 PNG справки и
22 HTML макетов обработок. Это внешние наблюдения, не выполненный здесь прогон БСП;
количества не зафиксированы как универсальные ограничения схемы.

## Совместимость и пределы

Новых таблиц, зависимостей и расширений kind CHECK нет. DDL register1 получает
новое точное значение профиля; schema.sql и схемы прежних профилей неизменны.
Канонический формат фактов journal1 сохранён; профиль/guard отделяют identity.
Все предшествующие виды наследуются, включая Recalculation. Reader разрешает
его descriptor-ветку в обоих register1/report1; прежние профили не расширены.

Open не повторяет BSL-разбор. Offline verify использует единый producer из
сохранённых sources, сверяет OID/SHA, объекты/родителей/владельцев, методы и строки
без Git. Это не подтверждение Git provenance. Подмена владельца asset с корректно
пересчитанными хешами может пройти open, но должна отвергаться offline verify.

`report_structure=complete_known_grammar`, `report_semantics=not_analyzed`;
companion_contents и relations not_analyzed. Сохранены file/total/count/Git/facts/
coverage/SQLite/time бюджеты, owner-only staging, лимиты reader, дерева и полного
JSON-RPC 65536 bytes. Точный lookup объектов/BSL-методов ADR-021 прежний.
Reader предназначен для доверенных артефактов с независимым expected SHA, без
гарантии безопасности произвольного SQLite. Поздние отказы откатывают факты,
удаляют собственную временную БД и оставляют output без опубликованного снимка.

## Проверка

```sh
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
cargo +1.85.1 test --locked --features experimental-snapshot-mcp --test mcp_snapshot reports_cli
```

Реальный CLI строит смешанный снимок, затем удаляется только созданный тестом Git.
Offline verify/open и настоящий stdio initialize/metadata/tree/source выполняются
при пустом PATH. Проверены точные BSL-методы/полные исходники/XML/assets, unparsed,
все сочетания наличия корневых модулей, формы без кода, пустая обработка,
parent_key табличной части и владелец команды, три формата макетов, четыре формата
Items и PNG справки. ZIP намеренно содержит непрозрачные bytes, не архив для распаковки.

Регрессия в том же новом профиле проходит от CalculationRegister к Recalculation,
проверяет parent_id, собственника Dimension, точные XML и scalar-ссылку.
29 отрицательных вариантов проверяют отсутствующие файлы, сирот, неполные пары,
неизвестные дорожки/типы/глубину, неверные UUID/Name, дубли и payload ambiguity;
output остаётся пустым. Проверены deterministic reuse, parity прежних фактов,
отказ старого профиля от новых корней и offline обнаружение неверного владельца.

[Перенос и команды](nucbox-report-diagnostic.md). Реальные данные и NucBox не
затрагивались; исходники передаются без БСП/ERP, бинарников и секретов. Полная БСП
ещё не принята: оставшиеся виды/структуры и реальная локальная приёмка — отдельная работа.
