# ADR-023: три вида планов

Основа `1d5d5d3143e30f8072a8514868a09a792f0e706e`. Новый opt-in
`build-charts`, профиль `linux-v5-chart1-xml221-r09`, Linux x86_64.
Предыдущие профили не расширяются. `chart_guard=three-charts-inline-form-png-v1`
и нормализованные бюджеты входят в identity снимка.

## Явная грамматика

| Корень → каталог | Inline-дети |
|---|---|
| ChartOfAccounts → ChartsOfAccounts | AccountingFlag, ExtDimensionAccountingFlag, Attribute |
| ChartOfCalculationTypes → ChartsOfCalculationTypes | Нет |
| ChartOfCharacteristicTypes → ChartsOfCharacteristicTypes | Attribute, TabularSection → Attribute |

Все три корня допускают scalar Form refs и InternalInfo. Дескриптор
`<каталог>/<Name>.xml`: MDClasses/MetaDataObject 2.21, один правильный тип,
unqualified uuid, прямой Properties с единственным scalar Name. Inline-узлы
также имеют uuid/Name; TabularSection имеет ровно один непустой ChildObjects
только с Attribute и допускает InternalInfo. У листьев ChildObjects запрещён.
Иные inline-теги, Template, неверные роли, UUID/namespace/Name, неизвестные
структурные оболочки и лишняя глубина отвергаются. Properties помимо Name и
содержимое InternalInfo отмечаются unverified, не интерпретируются семантически.

Каждое объявление сохраняет действительный XML path, диапазон исходных байтов
с учётом BOM, UUID, корневого owner_id и parent_key непосредственного родителя.
Синтетических файлов для inline-узлов нет. Ссылка из Configuration.xml также
сохраняется как точный диапазон.

У каждого корня обязателен `Ext/ManagerModule.bsl`, необязателен
`Ext/ObjectModule.bsl`. `Ext/Predefined.xml` необязателен и непрозрачен.
Справка — необязательная полная пара `Ext/Help.xml` + `Ext/Help/ru.html`.
Form ref разрешается в `Forms/<Name>.xml`; обязательны `Forms/<Name>/Ext/Form.xml`
и `Forms/<Name>/Ext/Form/Module.bsl`. Справка формы также принимается полной парой.
BSL принадлежит реальному корню либо форме; unparsed сохраняет bytes и причину,
не создаёт методов.

Только для ChartsOfAccounts разрешён дополнительный путь
`Forms/<Form>/Ext/Form/Items/<Item>/<File>.png`, с непустыми Item/File и точной
глубиной. PNG сохраняется непрозрачно с владельцем-формой. Его наличие необязательно;
число ограничено общими бюджетами. Имя Item не доказывает связь с элементом UI.
Иные расширения, глубина, формы-сироты и пути в других видах вызывают отказ.
Payload не разбирается как MDClasses, BSL или изображение; XML может иметь другую
схему, бинарные bytes сохраняются без преобразований. Source payload — Base64.

Основание — переданные агрегаты локальной сверки закреплённой БСП: семь корней,
семь ManagerModule, шесть ObjectModule, пять Predefined.xml, шесть пар справки;
два AccountingFlag, три ExtDimensionAccountingFlag, 46 корневых Attribute,
три TabularSection с 12 Attribute, 19 Form refs и один PNG. Здесь реальные данные
не читались и NucBox не запускался.

## Совместимость и пределы

Новая схема наследует service1 и расширяет CHECK inline kind двумя флагами;
таблицы и канонический формат фактов journal1 прежние. Значения kind/parent_key
уже входят в хеш. Профиль и guard отделяют новую identity. Старые схемы,
schema.sql, зависимости, команды и профили не изменены.

Build ограниченно читает по одному сохранённому source. Offline verify использует
тот же producer, сверяет OID/SHA, владельцев, структуру, методы и индексы без Git.
Open BSL не разбирает и Git-происхождение не подтверждает. Подмена владельца PNG
с пересчитанными хешами может пройти open, но должна быть отвергнута verify.
Поздний отказ откатывает факты и удаляет собственную временную БД без публикации.

Сохранены лимиты file/total/count/Git/facts/coverage/SQLite/time, private staging,
reader и полного JSON-RPC 65536 bytes. Reader предназначен для доверенных
артефактов с независимым expected SHA, без гарантии безопасности произвольного
SQLite. Точный lookup ADR-021 для объектов и BSL-методов сохранён; inline-ветки
по-прежнему ограничены числом узлов/байтов и не имеют нового фильтра.
`chart_structure=complete_known_grammar`, `chart_contents=not_analyzed`;
relations not_analyzed. Это не семантика учёта и не полнота всей БСП.

## Проверка

```sh
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
cargo +1.85.1 test --locked --features experimental-snapshot-mcp --test mcp_snapshot charts_cli
```

Тест вызывает фактический build-charts, удаляет только свой синтетический Git,
выполняет offline verify/open и настоящий stdio initialize/metadata/tree/source
при пустом PATH. Сверяются владельцы, parent_key, диапазоны XML/BOM, BSL и PNG
byte-exact, unparsed, отсутствие необязательного содержимого, lookup,
детерминизм и parity прежних фактов. 25 отрицательных вариантов проверяют
обязательные файлы, справку, неизвестные пути/вложения, UUID/Name, дубли и сироты;
output остаётся пустым. Проверяется семантически неверный владелец с корректными
формальными хешами. Прежний тест lookup охватывает 804 CommonModule и 261 метод.

[Перенос исходников и команды](nucbox-chart-diagnostic.md). Реальная полная БСП
не принята; оставшиеся корневые виды и структуры требуют отдельных срезов.
