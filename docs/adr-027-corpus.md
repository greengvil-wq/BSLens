# ADR-027: оставшиеся корни, Configuration и явная выборка исходников

Основа `820f29d795a60c8fd4353bfcd1030eb3541333e4`. Только новый opt-in
`build-corpus`, профиль `linux-v5-corpus1-xml221-r09`, Linux x86_64.
Наследуются 39 корневых видов workflow1 и добавляются три: всего 42 вида, не считая
Configuration. Старые профили, их DDL и `spikes/linux-v5/schema.sql` неизменны.
Это структурная грамматика, а не заявление приёмки реальной БСП.

## Новая грамматика

* Sequence: `Sequences/<Name>.xml`, плоские inline Dimension с UUID/Name,
  точными XML-диапазонами; собственные файлы не допускаются.
* SettingsStorage: `SettingsStorages/<Name>.xml`, scalar Form refs,
  обязательный `Ext/ManagerModule.bsl`. Каждая форма требует дескриптор,
  `Ext/Form.xml` и `Ext/Form/Module.bsl`. Справка формы — полная пара
  `Ext/Help.xml` + `Ext/Help/ru.html` либо отсутствует.
* Subsystem: `Subsystems/<Name>.xml`, только scalar Subsystem refs. Следующий
  дескриптор — `<owner>/Subsystems/<Child>.xml`. Допустимы три уровня включая
  корневой; четвёртый вызывает отказ. У каждого уровня независимо допустимы
  `Ext/CommandInterface.xml` и полная пара справки. Payload непрозрачен.
  Отсутствующий дескриптор, сирота, дубли, неверный тип/Name/UUID, неизвестные
  структуры и пути вызывают отказ. Число обработанных ссылок и очереди ограничено
  source_count; действуют также XML depth/events/nodes, facts/time бюджеты.

У дескрипторов MDClasses/MetaDataObject 2.21, unqualified UUID и один direct
Properties/Name. InternalInfo и остальные семантические свойства не объявляются
разобранными. У Subsystem сохраняется непосредственный parent_id, scalar-ссылка
с её фактическим XML path/byte range и отдельный полный дескриптор. Собственные
payload не присваиваются предкам. Нет виртуальных путей и распаковки ZIP.

Только новый профиль допускает пустой ChildObjects FilterCriterion: без атрибутов,
текста и элементов, не более одного. Самозакрывающаяся и парная пустая запись
эквивалентны. Непустой/неизвестный wrapper и wrapper других descriptor-only видов
по-прежнему отвергаются; предыдущие профили не расширены.

Configuration владеет четырьмя точными необязательными дорожками:
`Ext/ManagedApplicationModule.bsl`, `Ext/OrdinaryApplicationModule.bsl`,
`Ext/SessionModule.bsl`, `Ext/ExternalConnectionModule.bsl`.
MCP отдельно показывает accepted/unparsed/absent для каждой. Unparsed сохраняет
точные bytes и причину; вымышленных методов нет. Exact lookup и Recalculation
с непосредственным родителем и XML остаются доступны.

## Точная выборка и независимая сверка

Последний обязательный аргумент CLI `GLOBAL_OPAQUE_JSON` — JSON-массив точных
относительных путей. Например, **только для синтетики**:
`["Splash.png","Ext/First.bin","Ext/Second.bin","Ext/Archive.zip"]`.
Имена реальных четырёх файлов не предоставлены и здесь не угаданы. Список должен
быть получен локально на NucBox. `[]` означает явную пустую выборку, не автопоиск.

Правила selector (регистр чувствителен, пути безопасные repo-relative):

1. Все файлы с суффиксом `.xml` или `.bsl` выбираются, независимо от каталога.
   Это не означает, что любой XML layout принимается: неизвестный дескриптор остаётся причиной отказа. В известных семействах модули-сироты
   отвергаются; унаследованные BSL вне подтверждённых локаторов сохраняют
   явный unconfirmed owner и не приписываются Configuration.
2. Наследуется выборка всех файлов под `<Catalogs|Documents|InformationRegisters>/<Name>/<Forms|Templates>/`.
   Также выбираются все файлы в известных семействах CommonCommands/CommonForms/
   CommonTemplates, CommonPictures/Constants/Roles/ScheduledJobs/XDTOPackages,
   Enums/DocumentJournals, HTTPServices/WebServices/IntegrationServices,
   ChartsOfAccounts/ChartsOfCalculationTypes/ChartsOfCharacteristicTypes,
   AccountingRegisters/AccumulationRegisters/CalculationRegisters, DataProcessors/
   Reports, ExchangePlans/BusinessProcesses/Tasks и Sequences/SettingsStorages/Subsystems.
   Их допустимые пути проверяются соответствующими ADR-018–027; неизвестные
   дорожки в этих семействах не исключаются молча, а отвергаются.
3. Глобальные opaque выбираются **только** по `GLOBAL_OPAQUE_JSON`: файлы в корне
   или под `Ext/`, суффиксы `.png`, `.bin`, `.zip`. Не более 64, каждый путь ≤1024
   bytes, JSON ≤65536 bytes; повтор/небезопасный путь/другой тип — отказ. Порядок
   нормализуется binary sort. **65536 bytes — только внешний envelope аргумента.**
   Полный канонический `metadata.build_parameters` (UTF-8 bytes компактного JSON,
   включая все guards, бюджеты, policy и список) ограничен **16384 bytes**.
   Поэтому допустимые по отдельности 64 пути / 1024 bytes / envelope не гарантируют
   допустимость их суммы. Ровно 16384 принимается, 16385 даёт
   `Contract("metadata_field_budget")` с rollback и без опубликованного снимка.
   Лимит общий с reader и не повышен; не следует обходить отказ увеличением
   пользовательских бюджетов. Найденный глобальный PNG/BIN/ZIP вне списка и
   отсутствующий перечисленный файл вызывают отказ без публикации. Unlisted
   opaque не читается. Остальные файлы вне selector отражаются как
   `OutsideSelectionNotRead`, их содержимое не прочитано.
4. Глобальные XML — `.xml` непосредственно в корне (кроме Configuration.xml)
   либо под `Ext/` — сохраняются как `payload`, **без XML-разбора**. Глобальные
   XML/opaque принадлежат Configuration. Эта явная политика не распространяется
   на неизвестные каталоги корневых дескрипторов.

`metadata.build_parameters.global_scope` содержит policy
`root-or-ext-xml-opaque-allowlist-v1` и нормализованный список opaque. Политика,
профиль, guard и эффективные бюджеты входят в snapshot_id. Reader проверяет список
против sources, offline verify использует сохранённые источники без Git.

Для независимой сверки exact path/mode/OID:

```sql
SELECT path, mode, blob_oid, kind, length(content) AS bytes, sha256
FROM sources ORDER BY path COLLATE BINARY;
SELECT input_manifest, build_parameters FROM metadata WHERE singleton=1;
```

`sources.path` — точный Git path, `mode` — текст `100644`/`100755`, `blob_oid` —
Git blob OID, `sha256` — SHA-256 bytes, `content` — BLOB без преобразования.
`kind` — xml/bsl/payload. В `input_manifest.selected[]` те же path/mode/blob_oid;
`excluded[].input` содержит непрочитанные entries и reason. Локальная сверка с
`git ls-tree -r -z <полный commit>` должна учитывать **все** selected и excluded,
а не сравнивать только количество XML. Сохранённый OID и offline verify сами по
себе не доказывают Git provenance. Синтетический тест сверяет все три поля с
независимым ls-tree, включая executable mode, затем удаляет только свой Git.

## SQLite, ограничения и тесты

Новый schema profile наследует workflow1. Единственное изменение FK:
`companion_payloads.object_id REFERENCES objects(object_id)` вместо companions,
чтобы Configuration мог владеть глобальными файлами без вымышленной scalar-ссылки
на себя. Таблицы и столбцы не добавляются. Канонические факты уже включают
payload path/owner XML path/role; вычисление не дублируется. Старые схемы сохраняют
старый FK. Все ссылки и владельцы пересчитываются offline verify.

Остаются явные count/file/total/SQLite/Git/facts/coverage/time бюджеты, атомарный
rollback и отсутствие публикации при позднем отказе. Reader — для доверенных
артефактов с независимым expected SHA; гарантии защиты от произвольного SQLite
нет. MCP не увеличивает лимиты: 128 узлов ветки, отдельные reader byte limits,
полный сериализованный JSON-RPC ≤65536. Большие диапазоны читаются частями;
UTF-8 и Base64 восстанавливают точные bytes, без lossy. Содержимое payload,
CommandInterface, глобальных XML и relations: not_analyzed.

```sh
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
cargo +1.85.1 test --locked --features experimental-snapshot-mcp --test mcp_snapshot corpus_cli
```

Реальный build → удаление собственного synthetic Git → verify/open → stdio MCP
с пустым PATH: все 42 вида, SettingsStorage 13 forms, три уровня Subsystem,
точные inline/scalar XML/BSL/binary bytes, owner/parent, Configuration unparsed и
отдельное отсутствие optional module. Проверяются reuse, parity, Recalculation,
старый профиль, scope bounds и поздние ошибки с пустым output. Формально целый
снимок с ложным владельцем CommandInterface принимается open, но verify отвергает.
Наследованные lookup/transport/staging/ENOSPC/budget тесты выполняются целиком.

[Команды переноса](nucbox-corpus-diagnostic.md). Здесь только синтетика, без БСП,
NucBox и GitHub. Реальная приёмка остаётся отдельной: 42 известных вида не означают
проверку всех их возможных layout или семантическую полноту.

## Исправление классификации имени Ext

Имя объекта `Ext` не зарезервировано. Дескрипторы SettingsStorage определяются
как `<SettingsStorages>/<Name>.xml` или `<SettingsStorages>/<Name>/Forms/<Form>.xml`;
Subsystem — как последовательность пар `Subsystems/<Name>` с финальным `.xml`.
Наличие имени `Ext` в позиции Name не превращает дескриптор в payload. Настоящие
`Ext/Form.xml`, `Ext/CommandInterface.xml` и справка сохраняют kind=payload.
Формат профиля, DDL, identity и лимиты не изменены.

Регрессия строит SettingsStorage Ext с формами и Subsystem Ext с потомками,
удаляет только собственный Git, затем проверяет offline verify, stdio tree/source,
родителей, точные XML/BSL/payload и `sources.kind`. Для канонических параметров
проверены 16384 (build, offline verify, MCP) и 16385 (точный код отказа, пустой output),
при соблюдении более широких внешних ограничений списка и каждого пути.
