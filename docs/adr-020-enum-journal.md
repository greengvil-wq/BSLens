# ADR-020: Enum и DocumentJournal

Основа 705098ac1b69d8ca630f76fa81acc15f42f2da52. Новый opt-in `build-journals`,
профиль `linux-v5-journal1-xml221-r09`, только Linux x86_64. Старые профили и команды
не расширяются. Нормализованные бюджеты прежние; в snapshot_id добавлен
`journal_guard=enum-journal-inline-command-v1`. Реальная БСП в облаке не запускалась.

## Поддержанная структура

Явные локаторы: Enum → `Enums/<Name>.xml`, DocumentJournal →
`DocumentJournals/<Name>.xml`. Дескрипторы MDClasses/MetaDataObject 2.21, точные
Name/type, валидный уникальный unqualified UUID. Корневые ссылки и их диапазоны
берутся из Configuration.xml; родитель — Configuration. Неизвестные структурные
оболочки и корневые виды не пропускаются.

Enum/ChildObjects содержит только плоские EnumValue с UUID и Properties/Name.
DocumentJournal/ChildObjects содержит плоские Column и Command с UUID/Name и
скалярные ссылки Form/Template. Вложенный ChildObjects у EnumValue/Column/Command
запрещён. Объявление хранит реальный XML path, байтовый диапазон, ключ UUID,
непосредственный корневой owner и parent_key=NULL. Виртуальные XML-файлы не создаются.

| Путь относительно корня объекта | Правило |
|---|---|
| Enum `Ext/ManagerModule.bsl` | необязательный |
| Journal `Ext/ManagerModule.bsl` | необязательный |
| Journal `Commands/<Command>/Ext/CommandModule.bsl` | обязателен для каждого объявленного Command; сироты запрещены |
| Journal `Ext/Help.xml` + `Ext/Help/ru.html` | необязательны только полной парой |
| Journal `Forms/<Form>.xml` | обязательный дескриптор для каждой ссылки |
| Journal `Forms/<Form>/Ext/Form.xml` и `Ext/Form/Module.bsl` | оба обязательны в этом узком профиле |
| Journal `Forms/<Form>/Ext/Help.xml` + `Ext/Help/ru.html` | необязательная полная пара |
| Journal `Templates/<Template>.xml` и `Templates/<Template>/Ext/Template.xml` | обязательны для каждой ссылки |

Form/Template имеют свои реальные дескрипторы без ChildObjects и владельца Journal.
Модуль формы принадлежит Form. Правила форм прежних Catalog/Document/Register и
CommonForm не изменены. В этом срезе альтернативные payload макета журнала и
неизвестные дополнительные файлы вызывают отказ; они не исчезают из excluded.
Все файлы Enums/DocumentJournals выбираются под прежними явными лимитами.

Содержимое Form.xml, Help.xml, HTML и Template.xml непрозрачно. DCS без версии
принимается как payload, не как MDClasses-дескриптор. Семантика Synonym, Comment,
Color, DefaultForm, AuxiliaryForm, RegisteredDocuments и других Properties не
выводится: сохранённый XML и xml_unverified явно фиксируют границу разбора.
`journal_structure=complete_known_grammar`, `journal_contents=not_analyzed`,
relations=not_analyzed. Это не утверждение полноты всей конфигурации или Навигатора.

Грамматика опирается на переданные агрегаты локальной замороженной БСП: 135 Enum,
572 EnumValue, 13 ManagerModule; 4 Journal, 27 Column, 15 Command с модулями,
9 Form, 4 Template, 5 пар справки форм, ManagerModule и справка у 3 из 4 Journal.
Эти числа не являются результатом облачной приёмки реального корпуса.

## Версионированное хранение владельца команды

Новый профиль расширяет CHECK inline_declarations на EnumValue/Column и добавляет
`inline_module_owners(module_path, declaration_key, owner_id)`. FK связывает модуль
с существующим modules и declaration_key+owner_id; путь модуля и ключ объявления
уникальны. Индекс обеспечивает поиск по объявлению. Таблица расходует facts budget.
Исходный schema.sql и схемы прежних профилей не меняются.

`modules.owner_id` остаётся корневым объектом-контейнером для совместимого SQL-дерева.
Непосредственный владелец CommandModule определяется только explicit
inline_module_owners, не угадывается по имени. MCP показывает `owner_declaration`
и `container_object_id` и в списке модулей, и в ветке модуля. Inline Command.children
переходит прямо к модулю. EnumValue/Column не имеют выдуманных методов или детей.

Канонизация нового профиля имеет домен `BSLens-v5/facts/journal1` и включает новую
таблицу по стабильным ключам. Старые домены и identity неизменны. Reader распознаёт
точную новую схему; open не парсит BSL повторно. Offline verify из сохранённых
sources пересчитывает всю структуру и явную связь команды тем же producer;
проверяет OID/SHA, но не Git provenance. Подмена declaration_key при корректных
facts/file hashes формально проходит open, но не проходит verify.

MCP также показывает manager_presence=accepted/unparsed/absent. Отказ BSL сохраняет
unparsed и исходные байты без методов. Payload выдаётся Base64; валидный BSL — UTF-8,
невалидный — Base64 без lossy. Пользоваться возвращёнными children/source, включая
реальные XML-диапазоны с учётом BOM. Сохраняется лимит полного JSON-RPC 65536 bytes;
новые строки владельцев имеют отдельную проверку длины и ограниченное число строк.

## Отказ и проверки

Пропущенный CommandModule, дескриптор, обязательный payload, половина пары справки,
неизвестный путь/тег, вложенность, дубли, неверный UUID/Name или превышение бюджета
вызывают отказ всей сборки. Savepoint откатывает рассчитанные факты, временная БД
удаляется, снимок не публикуется. Исходники читаются по одному, а не собираются в
Manifest с телами всего корпуса. Сохранены file/total/count/Git/facts/coverage/SQLite/time
лимиты и строгий private staging. Reader рассчитан на доверенные артефакты с
независимым expected SHA; безопасность произвольного SQLite не заявляется.

```sh
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
cargo +1.85.1 test --locked --features experimental-snapshot-mcp --test mcp_snapshot enum_journal_cli
```

При одном target запускать последовательно. Последний тест выполняет реальный
build, удаляет только созданный тестом Git, затем open/verify и stdio MCP с пустым
PATH. Проверены точный XML/BOM, сортировка, одинаковые имена команд у разных
журналов, immediate owner, формы/макеты, BSL CRLF/last-no-LF, unparsed и бинарные
HTML bytes; отказ при поздней ошибке, пустой output, детерминизм и parity старых
фактов. Пересчитанная неправильная связь команды отвергается offline verify.

[Перенос и команды NucBox](nucbox-journal-diagnostic.md). Полная приёмка pinned БСП
и остальные неподдержанные корневые виды/вложения остаются за пределами этого шага.
Ни рабочие данные, ни NucBox здесь не использовались; публикация и merge не выполняются.
