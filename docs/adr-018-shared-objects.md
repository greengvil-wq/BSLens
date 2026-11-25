# ADR-018: CommonCommand, CommonForm, CommonTemplate

Основа f9affa67da0036a8fb8003c325879897f0061ae0. Новый opt-in `build-shared`, профиль
`linux-v5-shared1-xml221-r09`. Старые профили не расширяются. Схема companion1
с новым CHECK профиля; SQLite v5, без новых таблиц и зависимостей. Нормализованные
бюджеты и `shared_guard=common-command-form-template-known-files-v1` входят в
snapshot_id. Канонизация facts сохраняет прежний домен и охватывает новые строки.

## Подтверждённая форма и поддержанная грамматика

Переданный результат локальной проверки замороженной БСП: 96 CommonCommand с
CommandModule.bsl; 121 CommonForm с Form.xml, 120 с Module.bsl, 65 с парой справки;
23 CommonTemplate, каждый с одним Template.xml/bin/txt (10/7/6). Три дополнительных
дорожки макетов подтверждены как Ext/Template/ru.html при extrnprops/Help XML.
Это входные агрегаты задачи, а не выполненный здесь прогон реальных источников.

Общие объекты — непосредственные дети Configuration. Локаторы явные:
CommonCommand→CommonCommands, CommonForm→CommonForms, CommonTemplate→CommonTemplates.
Дескрипторы `<каталог>/<Name>.xml`: MDClasses/MetaDataObject 2.21, точные type/Name,
валидный уникальный unqualified UUID, Properties; ChildObjects запрещён.
Возможный InternalInfo остаётся unverified. Общий корневой локатор включается
только в новом профиле; build-roots и предыдущие команды отвергают эти три вида.

| Вид | Допустимые файлы под `<каталог>/<Name>/` |
|---|---|
| CommonCommand | обязательный `Ext/CommandModule.bsl` |
| CommonForm | обязательный `Ext/Form.xml`; необязательный `Ext/Form/Module.bsl`; справка только парой `Ext/Help.xml` + `Ext/Help/ru.html` |
| CommonTemplate | ровно один `Ext/Template.xml`, `.bin` или `.txt`; HTML `Ext/Template/ru.html` только вместе с Help XML |

Владелец CommandModule — реальный CommonCommand, владелец Form/Module — реальный
CommonForm. Отсутствие BSL у формы допустимо и не порождает фиктивного модуля.
Отказ лексического разбора сохраняется как unparsed с причиной, без методов;
исходник и физические диапазоны доступны. Отсутствие обязательного файла,
неизвестный путь, неоднозначный payload, неверный дескриптор, сироты и неполная
пара справки вызывают отказ всей сборки, без опубликованного снимка.

## Непрозрачные payload и пары XML/HTML

Все файлы трёх каталогов входят в выборку, включая binary/HTML. Они читаются
ограниченно через Git, сохраняются byte-exact в sources с OID/SHA и расходуют
обычные бюджеты. Неизвестные дополнительные файлы вызывают отказ, а не теряются
в excluded. Payload не становится дескриптором или BSL. CommonForm Form.xml,
Help.xml, Template.bin/txt и HTML не анализируются семантически.

Для Template.xml нужен один узкий структурный признак: является ли корневой
элемент `Help` в namespace `http://v8.1c.ru/8.3/xcf/extrnprops`. Заголовок читается
из SQLite максимум 65536 байт и 128 XML-событий, только до первого элемента.
При таком Help XML обязателен Template/ru.html; HTML без Help XML отвергается.
Нераспознаваемый/превышающий бюджет заголовок — явный unsupported/limit.
Оба фиксированных предела входят в параметры identity. Тело XML не разбирается,
не требуется MDClasses и не проверяется версия 2.21 у payload. Spreadsheet
`document` и DataCompositionSchema без версии проверены синтетически и принимаются
как непрозрачные тела без HTML. Это не проверка полной XML-корректности содержимого.
Справка формы проверяется по фиксированной паре имён, без анализа тела Help.xml.

Покрытие `shared_structure=complete_known_grammar` означает только сохранение
всех файлов трёх видов в объявленной грамматике. `shared_contents=not_analyzed`.
Никаких семантических связей, контекстов исполнения или полноты всего Навигатора
из этого не следует. Неизвестная грамматика не даёт успешного частичного снимка.

## Навигация и offline verify

Используются прежние таблицы objects, modules, companions, companion_payloads.
Для каждого общего объекта сохранён реальный диапазон ссылки в Configuration.xml
(с учётом BOM), непосредственный родитель и путь дескриптора. MCP: корень → вид →
объект → модуль/методы или companions. `branch.source` возвращает сам дескриптор;
companions содержит XML-доказательство ссылки, payload и пары справки. XML/HTML/bin/txt
payload возвращается явно Base64, без lossy и без отображения как BSL. Пользоваться
возвращёнными source/children; не конструировать ID или пути догадкой.

Open не разбирает BSL повторно, сессия MCP использует один immutable reader. Verify
проверяет сохранённые OID/SHA и тем же producer пересчитывает структуру/владельцев,
диапазоны и индекс. Подмена владельца с пересчитанными facts/file checksum формально
проходит open, но отвергается offline verify. Git provenance не подтверждается.
Сохранены прежние доверенные source/facts/SQLite/time лимиты, private staging,
полный wire-budget 65536 bytes и атомарный rollback. Проверено только Linux x86_64.
Reader предназначен для доверенных SQLite-артефактов, не для произвольных атакующих БД.

## Проверки и команды

```sh
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
cargo +1.85.1 test --locked --all-features --test mcp_snapshot shared_objects_cli
```

Сборки запускать последовательно в одном target. Последний тест использует реальный
builder, удаляет только созданный тестом Git, затем выполняет verify/open и stdio
MCP с пустым PATH. Проверены три вида вместе с прежними, владельцы, форма без BSL,
unparsed, точный метод/BOM/CRLF/последняя строка, все пары и варианты payload,
spreadsheets/DCS без версии, строгие отказы и rollback, parity прежних фактов,
детерминизм, подмена владельца при корректных хешах. Реальная БСП в облаке не запускалась.

`build-shared` принимает те же 18 аргументов, что build-roots, без повышения лимитов.
[Перенос и команды NucBox](nucbox-shared-diagnostic.md). Полная приёмка pinned БСП,
остальные корневые виды/вложения и остальные разрезы Навигатора остаются впереди.
