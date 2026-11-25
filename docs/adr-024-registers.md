# ADR-024: регистры и отдельный Recalculation

Основа `7fd780382afd77169aefa0d1534ec696ed519c45`. Новый opt-in
`build-registers`, профиль `linux-v5-register1-xml221-r09`, Linux x86_64.
`register_guard=three-registers-recalculation-v1` и нормализованные бюджеты входят
в identity. Старые профили и команды не расширены.

## Грамматика и адреса

| Корень → каталог | Inline-дети | Scalar-ссылки |
|---|---|---|
| AccountingRegister → AccountingRegisters | Attribute, Dimension, Resource | Form |
| AccumulationRegister → AccumulationRegisters | Dimension, Resource | Form |
| CalculationRegister → CalculationRegisters | Dimension, Resource | Form, Recalculation |

Корневой дескриптор — `<каталог>/<Name>.xml`, MDClasses/MetaDataObject 2.21,
один прямой элемент правильного типа с unqualified UUID, Properties/Name и
допустимыми InternalInfo/ChildObjects. Inline-дети плоские, имеют UUID и scalar
Name; собственные ChildObjects и неизвестные структурные оболочки запрещены.
Properties помимо Name и содержимое InternalInfo остаются unverified.

Recalculation разрешается только у CalculationRegister в
`CalculationRegisters/<Owner>/Recalculations/<Name>.xml`. Это самостоятельный
дескриптор MDClasses 2.21 с UUID/Name, InternalInfo и ровно одним непустым
ChildObjects, содержащим только плоские Dimension. Его parent_id — ID регистра;
owner_id Dimension — ID Recalculation, не регистра. Ключи inline содержат реальный
XML path и UUID; диапазоны байтов учитывают BOM. В companions сохраняется диапазон
scalar-ссылки в XML родителя. Вложенные ссылки, собственные BSL/payload/справка
пересчёта не поддерживаются и отвергаются. Recalculation не является корневым видом
Configuration и не разрешается по пути другого родителя.

Form разрешается по существующему пути `Forms/<Name>.xml`; обязательны
`Ext/Form.xml` и `Ext/Form/Module.bsl`. Справка формы — необязательная полная пара
`Ext/Help.xml` + `Ext/Help/ru.html`. Неизвестные дополнительные файлы отвергаются.

AccountingRegister требует `Ext/RecordSetModule.bsl`; иных корневых BSL здесь нет.
Для AccumulationRegister известны необязательные ManagerModule и RecordSetModule,
а также необязательный `Ext/Aggregates.xml`. CalculationRegister не имеет
поддержанных корневых BSL. Справка корней допускается полной парой Help.xml/ru.html;
отсутствие всей пары не означает неизвестный файл. Aggregates.xml, справка и
Form.xml сохраняются как непрозрачные payload, без проверки MDClasses или
семантического разбора. Их точные bytes выдаются Base64. BSL сохраняет исходные
строки, BOM/CRLF/последнюю строку без LF; при unparsed методы не выдумываются.

Основание — обезличенная локальная сверка из постановки: два бухгалтерских
регистра (6 Attribute/4 Dimension/6 Resource/2 Form), два накопления
(4 Dimension/2 Resource/1 Form), один расчёта (2 Dimension/1 Resource/2 Form и
Recalculation с двумя Dimension). Все пять имеют справку; наблюдались два
RecordSetModule бухгалтерских регистров, оба модуля у одного накопления и ни
одного у второго, один Aggregates.xml. Реальные данные здесь не читались.
Грамматика не фиксирует наблюдённые количества узлов как универсальную норму.

## Контракт и пределы

Новых таблиц и kind CHECK не требуется: используются objects.parent_id,
inline_declarations и companions. Схема chart1 отличается только точным профилем;
канонический формат journal1 и исходный schema.sql неизменны. Новый профиль/guard
отделяют identity. Reader/verify используют единый producer для нового профиля;
open не повторяет BSL-разбор. Offline verify сверяет sources OID/SHA, XML-факты,
родителей, владельцев, методы и индекс строк без Git, не подтверждая Git provenance.

MCP: корень → вид → объект → companions → Recalculation → inline Dimension.
Источник Recalculation — его полный дескриптор, источник reference — точный диапазон
в родителе. Точный lookup объектов/BSL-методов ADR-021 сохранён. Лимиты inline-веток,
узлов/байтов, полного JSON-RPC 65536 bytes, reader и bounded producer не снимаются.
`register_structure=complete_known_grammar`, `register_semantics=not_analyzed`;
relations not_analyzed. Движения, вычисление агрегатов, алгоритмы расчёта и
обработчики не интерпретируются и не выполняются.

Неизвестные теги/пути, неверный UUID/Name/namespace, сироты, дубли и отсутствующий
обязательный файл вызывают отказ. Поздний отказ откатывает факты и удаляет свою
временную БД, output остаётся пустым. Сохраняются file/total/count/Git/facts/coverage/
SQLite/time бюджеты. Reader предназначен для доверенных артефактов с независимым
expected SHA; не обещает безопасность произвольного SQLite.

## Синтетическая проверка

```sh
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
cargo +1.85.1 test --locked --features experimental-snapshot-mcp --test mcp_snapshot registers_cli
```

Реальный CLI строит снимок, затем удаляется только созданный тестом Git. Offline
verify/open и stdio initialize/metadata/tree/source выполняются с пустым PATH.
Сверяются точные методы/исходники, XML-диапазоны, payload, формы, владельцы,
unparsed и отсутствие BSL, Recalculation parent_id и его Dimension. Проверяются
детерминизм/reuse, parity прежних фактов и отказ предыдущего профиля от регистров.
27 отрицательных вариантов покрывают обязательные файлы, справку, неизвестные
пути/типы/вложения, неверные роли и ссылки, UUID/Name/namespace, дубли и сироты.
Подмена родителя Recalculation с пересчитанными хешами проходит open, но должна
быть отвергнута offline verify. Прежние тесты и точный lookup проверяются вместе.

[Перенос и команды](nucbox-register-diagnostic.md). Архив содержит исходники и
синтетические тесты, без БСП/ERP/бинарников/секретов. NucBox не запускался.
Полная БСП не принята: оставшиеся неподдержанные виды и структуры — отдельные срезы.
