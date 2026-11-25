<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/bslens-dark.svg">
  <img alt="BSLens: только нужный контекст для ИИ-агента" src="assets/bslens-light.svg" width="440">
</picture>

# BSLens

BSLens — проект навигатора по конфигурациям 1С для разработчиков и ревьюеров.
Он помогает находить объекты, модули и методы в конкретном коммите Git и
открывать нужные строки через локальный MCP. Ответы не зависят от текущих
файлов в рабочей копии.

## Отдельный Git-режим R09

Экспериментальный MCP-сервер R09 на Linux предоставляет два инструмента:

- `tree` раскрывает дерево постранично: конфигурация → вид объектов → объект →
  модуль → процедура или функция.
- `source` по ссылке из дерева возвращает физические строки исходного файла
  вместе с координатами и идентификатором Git-объекта.

Неизвестные конструкции дают явный отказ. Модуль с неподтверждённым владельцем
показывается отдельно; одноимённые методы имеют разные идентификаторы.

## Как запустить R09

Для сборки используется Rust 1.85.1. На входе нужен локальный Git-репозиторий
с поддерживаемой выгрузкой 1С. Передайте абсолютный путь и полный SHA коммита.

```sh
cargo build --release --locked --features experimental-router --bin bslens-mcp
./target/release/bslens-mcp --repo /absolute/path/to/repository --commit FULL_COMMIT_SHA
```

Укажите эту команду в настройках MCP-клиента как сервер с транспортом stdio.
Сервер не открывает сетевые порты, не меняет репозиторий и не читает рабочую копию.
При ошибке запуска процесс завершается с ненулевым кодом.

## Ограничения

Выгрузка XML 2.21: корневая конфигурация, справочники, документы и регистры
сведений; вложенные и другие неподдержанные сущности дают отказ. Поддержаны
однострочные объявления процедур и функций без параметров на русском и английском.
Именованные сбалансированные области поддержаны только вне методов
([точная граница](docs/adr-009-regions.md)).
Условная компиляция, асинхронные объявления и некоторые другие конструкции пока
не разбираются. BSLens не проверяет семантику тела и не строит граф вызовов.

R09 ограничен 4 096 отслеживаемыми файлами и 32 MiB тел выбранных Git-объектов;
полная выгрузка БСП превышает эти лимиты. Проверки проведены на синтетических
данных в Linux. Работа на ERP УХ и Windows не подтверждена.
Автономного снимка v5 в R09 нет: локальный репозиторий Git нужен и с кешем.

Для тестов с `--all-features` нужны Git и `/usr/bin/python3` на Linux.

```sh
cargo test --locked
cargo test --locked --all-features
```

[Подключение и проверка MCP](docs/adr-006c-executable.md) ·
[Контракт методов и исходника](docs/adr-005b-methods.md) ·
[Проверка Git и построение дерева](docs/adr-005-pipeline.md)

## Необязательный кеш повторного запуска

Для Linux доступен opt-in кеш выбранных XML/BSL исходников. Он включается feature
`experimental-cache` и параметром `--cache-dir`; обычный запуск не меняется.
Повторный запуск не перечитывает тела Git-объектов из репозитория, но проверяет
коммит и дерево Git и заново строит структуру из кешированных исходников.
[Подключение, ограничения и явное перестроение](docs/adr-008b-cache-cli.md).

## Автономный MCP по SQLite v5: S5–S6

Кандидат интеграции v5 из PR #1–#4; формат снимка остаётся SQLite.
Экспериментальный режим только Linux x86_64, без Git после построения снимка.
Это отдельный opt-in режим, команды R09 выше не меняются:

Builder v5 наряду с четырьмя явными лимитами пока наследует ограничения
upstream Git: 4096 файлов и 32 MiB суммарных Git-объектов. Увеличение явных
лимитов v5 не снимает эти ограничения; полный перечень — в [контракте S3](docs/adr-010-s3.md).

```sh
cargo build --locked --features experimental-snapshot-mcp --bin bslens-v5 --bin bslens-mcp
# Предварительная сборка на одноразовом синтетическом Git-репозитории:
./target/debug/bslens-v5 build /absolute/synthetic-repo FULL_COMMIT synthetic:demo /absolute/output 100 1000000 4000000 4000000
# После сборки Git не нужен. Используйте имя ID.SHA.sqlite, выданное сборщиком:
./target/debug/bslens-v5 verify /absolute/output/ID.SHA.sqlite --staging-dir /absolute/private/staging --max-sqlite-bytes 4000000 --expected-sha256 SHA --max-sources 100 --max-source-bytes 1000000 --max-total-bytes 4000000 --max-scratch-bytes 4000000
```

Затем запуск MCP:

```sh
cargo build --locked --features experimental-snapshot-mcp --bin bslens-mcp
./target/debug/bslens-mcp --snapshot /absolute/ID.SHA.sqlite \
  --staging-dir /absolute/private/staging --max-sqlite-bytes 4000000 \
  --expected-sha256 SHA
```

Staging заранее создаётся с правами 0700 и доверенными родителями; число в примере —
только синтетический бюджет. Параметры `--repo`/`--commit` с `--snapshot` запрещены.
Linux reader предназначен для доверенно полученных артефактов. Значение
`--expected-sha256` следует получать из независимой доверенной записи:
сумма в имени файла сама по себе не подтверждает его происхождение.
Безопасность обработки произвольных, в том числе враждебных, SQLite-файлов
не гарантируется; [граница защиты reader описана в S4](docs/adr-010-s4.md#граница-защиты-sqlite).
Один reader обслуживает всю сессию: read-only `metadata`, `tree`, `source`.
В `tools/call` отсутствие `arguments` равно `{}` (metadata и корневой tree);
явные неправильные типы отвергаются. `snapshot.coverage` — ограниченная сводка
`allowlist-v1`: счётчики, число XML-замечаний и SHA-256 нормализованного полного
покрытия, с `details_omitted=true`. Селекторы, пути и неизвестные поля покрытия
не передаются. `scope_id` остаётся точным; общий бюджет контекста учитывает escaping.
Большой coverage_draft сам по себе не блокирует metadata/tree/короткий source.

Байты `source` находятся в `result.structuredContent.data`: при `encoding=utf-8`
закодируйте поле `data` как UTF-8; при `encoding=base64` декодируйте стандартный
Base64 с padding. Проверяйте `byte_length`/`sha256`; сохраняйте BOM, CRLF и
отсутствие последнего LF. Lossy-декодирования нет. 65536 байт — предел **всего**
JSON-RPC body, включая escaping/конверт и обе формы MCP-ответа. При превышении
возвращается -32001 без частичного payload; запросите меньший диапазон строк.

[Запуск, handshake, схемы и машинные коды S5–S6](docs/adr-010-s5-s6.md).
Синтетическая приёмка фактического бинаря: `cargo test --locked --all-features --test mcp_snapshot`.
Совмещённый smoke `CLI build → удаление одноразового репозитория → offline verify → MCP`
выполнен отдельно вручную; это не единый тест в CI. CI запускает форматирование
и существующие тестовые наборы default/all-features.
Проверки БСП/ERP, замеры S8 и production-приёмка этим не заявляются.


## Узкий opt-in build-ingest

Команда `bslens-v5 build-ingest` подключает пофайловый Git ingest из #5 к writer
с отдельным профилем `linux-v5-ingest1-r09-facts`. Она позволяет большой Git inventory
с невыбранными файлами, но сохраняет пределы выбранных XML/BSL: 4096 / 32 MiB.
Legacy `build` не меняется. [Контракт, лимиты, команда и сквозной тест](docs/adr-011-ingest-writer.md).
Полная БСП этим не разблокирована: пофайловый producer фактов и вложенный XML остаются
следующими шагами. Новый профиль читается обновлёнными reader/verify/MCP.

## Явный stream-профиль

`bslens-v5 build-stream` использует профиль `linux-v5-stream1-xml221-r09` и
пофайловый расчёт из SQLite sources. Через реальные CLI проверены отдельно
более 4096 выбранных файлов и более 32 MiB выбранных тел, затем offline verify
и MCP без Git. Старые `build`/`build-ingest` сохраняют свои пределы.
[Команды, обязательные бюджеты и сквозные тесты](docs/adr-013-streaming-profile.md).
Структурный `NestedUnsupported` остаётся отказом; полная БСП не принята.

## Узкий inline-профиль структуры

Команда `build-inline` добавляет inline Attribute/Command/TabularSection/Dimension/Resource
только в подтверждённых ролях Catalog/Document/InformationRegister, включая реквизит
табличной части. Хранятся непосредственный родитель, реальный XML и точные bytes;
неизвестные свойства помечаются непроверенными. Form/Template и остальные корневые
типы остаются отказами. [Контракт](docs/adr-014-inline-structure.md) и
[комплект для локальной диагностики NucBox](docs/nucbox-inline-diagnostic.md).
Это первый структурный разрез, не полная приёмка БСП и не модуль анализа.

## Структура форм и макетов

`build-companions` — отдельный профиль `linux-v5-companion1-xml221-r09` поверх
inline-среза. Разрешает MDClasses Form/Template ссылки у трёх поддержанных
родителей, сохраняет реальный дескриптор и XML-диапазон ссылки, привязывает BSL
формы к самой форме. Отсутствие кода формы или payload макета допустимо для
структуры; содержимое payload и дополнительных файлов остаётся `not_analyzed`.
Бинарники возвращаются только явно отмеченным Base64, не как BSL.
[Контракт и тесты](docs/adr-015-companion-structure.md),
[перенос и локальная диагностика](docs/nucbox-companion-diagnostic.md).
Общие модули и остальные корневые типы в этот шаг не входят; полная БСП и
Навигатор ещё не приняты. Старые команды и профили остаются прежними.

## Общие модули

`build-common-modules` — новый профиль `linux-v5-common1-xml221-r09` с обязательными
парами CommonModule XML + Ext/Module.bsl. Общий модуль получает собственного
владельца; unparsed сохраняет точный исходник и причину отказа без методов.
Старые профили остаются прежними. [Контракт и проверки](docs/adr-016-common-modules.md),
[перенос на NucBox](docs/nucbox-common-diagnostic.md). Это структурный срез;
остальные корневые типы и полная приёмка БСП ещё впереди.

## Корневые локаторы без вложений

`build-roots` — отдельный профиль `linux-v5-roots1-xml221-r09`, добавляющий только
11 descriptor-only видов по явной таблице XML-типов и каталогов. Неизвестные виды,
вложения и companions отвергаются, включая невыбранные бинарные файлы. Старые
профили не расширяются. MCP возвращает реальный XML через `branch.source`;
семантика свойств остаётся unverified. [Контракт и таблица](docs/adr-017-root-locators.md),
[локальная диагностика](docs/nucbox-roots-diagnostic.md). Полной приёмки БСП нет.

## Общие команды, формы и макеты

`build-shared`, профиль `linux-v5-shared1-xml221-r09`, добавляет CommonCommand,
CommonForm и CommonTemplate. Владельцы BSL проверяются; BSL формы может отсутствовать.
Известные payload и пары XML/HTML сохраняются точно и остаются `not_analyzed`.
Неизвестные файлы, недостающие обязательные файлы и неполные пары отвергаются.
[Контракт и ограничения](docs/adr-018-shared-objects.md),
[команды для NucBox](docs/nucbox-shared-diagnostic.md). Старые профили неизменны;
полная БСП и Навигатор ещё не приняты.

## Что следует за интеграционным кандидатом

Базовый draft #8 объединяет синтетический срез v5 (PR #1–#4) с main
и сохраняет его CI. Текущий кандидат добавляет только описанный выше узкий ingest-срез. Слияние и приёмка требуют отдельного решения.
Полная настоящая БСП пока не принята: остаётся `NestedUnsupported`; приватный
диагностический коммит `0ebf1fd` не подтверждает поддержку и не включён.

Ingest из [PR #5](https://github.com/greengvil-wq/BSLens/pull/5) (`853f472`) теперь
подключён к writer только в новом режиме; пофайловый расчёт теперь подключён отдельной командой `build-stream`.
Следующий шаг — строгая поддержка нужной вложенной XML-структуры и отдельная приёмка. [PR #6](https://github.com/greengvil-wq/BSLens/pull/6)
(`7321cbc`) содержит только замеры: их следует применять к согласованной сквозной
реализации после подключения ingest. Эти PR не включены механически; их наличие
не означает готовность полного корпуса. Изменения рабочих баз и данных не нужны
для этого синтетического интеграционного шага.

## Пять дополнительных корневых видов без вложений (opt-in)

`build-leaves`, профиль `linux-v5-leaf1-xml221-r09`, добавляет CommonPicture,
Constant, Role, ScheduledJob и XDTOPackage. Все известные бинарные/XML payload
сохраняются точно и остаются непрозрачными; оба модуля Constant независимо
необязательны, расписание ScheduledJob может отсутствовать. Неизвестные пути и
неполные обязательные наборы вызывают строгий отказ без публикации.
[Контракт и синтетические проверки](docs/adr-019-leaf-objects.md),
[команды для NucBox](docs/nucbox-leaf-diagnostic.md). Старые профили не расширены.
Это ограниченный структурный срез, не приёмка полной БСП.

## Enum и DocumentJournal (opt-in)

`build-journals`, профиль `linux-v5-journal1-xml221-r09`, добавляет плоские
EnumValue/Column/Command, модули команд и формы/макеты журналов. Inline Command
связан с модулем явной таблицей владельцев и точным XML-диапазоном; старые профили
не расширяются. ManagerModule необязателен, справка принимается полной парой.
Неизвестная структура и неполные обязательные файлы вызывают rollback.
[Контракт и проверки](docs/adr-020-enum-journal.md),
[команды для NucBox](docs/nucbox-journal-diagnostic.md). Payload и свойства вне
Name/структуры остаются непроанализированными; полная БСП ещё не принята.

## Точный поиск по видимому имени в snapshot MCP

В `tree` добавлен необязательный `name_exact`: объект внутри вида/родителя или
метод внутри модуля. Фильтр выполняется до проверки бюджета; ветки с 804 объектами
можно искать без загрузки целиком. ID/пути берутся из children предыдущих ответов.
Сравнение точное и чувствительное к регистру; видимые имена методов прежнего R09
уже в нижнем регистре. Дубли возвращаются как ambiguous, unparsed не означает
отсутствие метода. Старые запросы и лимиты не меняются.
[Контракт, примеры последовательности и проверка](docs/adr-021-exact-name.md).

## HTTPService, WebService, IntegrationService (opt-in)

`build-services`, профиль `linux-v5-service1-xml221-r09`, добавляет структуру
URLTemplate → Method, Operation → Parameter и IntegrationServiceChannel.
Обязательный Module.bsl принадлежит корню; inline-узлы имеют точные XML-диапазоны
и непосредственного родителя. Пустая Operation допустима, пустой URLTemplate — нет.
Семантика обработчиков/параметров не анализируется. Старые профили и точный lookup
сохранены; неизвестная структура вызывает rollback.
[Контракт и тесты](docs/adr-022-services.md), [команды NucBox](docs/nucbox-service-diagnostic.md).
Полная БСП ещё не принята.

## Три вида планов (opt-in)

`build-charts`, профиль `linux-v5-chart1-xml221-r09`, добавляет ChartOfAccounts,
ChartOfCalculationTypes и ChartOfCharacteristicTypes, inline-флаги/реквизиты,
табличные части и формы с точными XML-диапазонами. ManagerModule обязателен;
ObjectModule и Predefined.xml необязательны, справка принимается полной парой.
PNG внутри формы плана счетов сохраняется непрозрачно с владельцем-формой.
Старые профили неизменны; неизвестная структура вызывает rollback.
[Контракт и тесты](docs/adr-023-charts.md), [команды NucBox](docs/nucbox-chart-diagnostic.md).
Семантика учёта и содержимое payload не анализируются. Полная БСП ещё не принята.

## AccountingRegister, AccumulationRegister, CalculationRegister (opt-in)

`build-registers`, профиль `linux-v5-register1-xml221-r09`, добавляет три вида
регистров, плоские inline Attribute/Dimension/Resource по явной грамматике и формы.
Recalculation разрешается в отдельный дескриптор с родителем CalculationRegister;
его Dimension имеют собственного владельца и точные XML-диапазоны. Aggregates.xml
и справка непрозрачны. Неизвестные пути/структура вызывают rollback.
[Контракт и тесты](docs/adr-024-registers.md), [команды NucBox](docs/nucbox-register-diagnostic.md).
Старые профили сохранены. Семантика движений/агрегатов/расчётов не анализируется;
полная БСП ещё не принята.

## DataProcessor и Report (opt-in)

`build-reports`, профиль `linux-v5-report1-xml221-r09`, добавляет обработки и
отчёты: Attribute/Command/TabularSection, точных владельцев модулей команд,
формы с необязательным BSL и макеты с одним payload. Известные ресурсы форм
обработок сохраняются непрозрачно, ZIP не распаковывается. Неизвестный путь или
структура вызывают rollback; старые профили и точный lookup сохранены.
[Контракт и проверки](docs/adr-025-reports.md), [команды NucBox](docs/nucbox-report-diagnostic.md).
В смешанном снимке новый профиль сохраняет навигацию к Recalculation.
Полная БСП ещё не принята; содержимое payload и бизнес-семантика не анализируются.

## ExchangePlan, BusinessProcess и Task (opt-in)

`build-workflows`, профиль `linux-v5-workflow1-xml221-r09`, добавляет структуру
трёх видов, AddressingAttribute задачи, табличные части и точных владельцев команд.
Manager/Object обязательны; Content.xml и Flowchart.xml сохраняются непрозрачно.
Формы ExchangePlan допускают отсутствие BSL; макеты этого среза ограничены TXT.
[Контракт и проверки](docs/adr-026-workflows.md), [команды NucBox](docs/nucbox-workflow-diagnostic.md).
Старые профили, lookup и Recalculation сохранены. Состав обмена и маршрут не
анализируются и не выполняются. Полная БСП ещё не принята.

## Интеграционный профиль 42 видов и Configuration (opt-in)

`build-corpus`, профиль `linux-v5-corpus1-xml221-r09`, добавляет Sequence,
SettingsStorage и ограниченные рекурсивные Subsystem; только здесь исправлен
пустой ChildObjects FilterCriterion. Четыре модуля Configuration имеют явные
accepted/unparsed/absent. Глобальные XML сохраняются без разбора; PNG/BIN/ZIP
требуют явного JSON-списка путей, включённого в identity. Неизвестные/отсутствующие
дорожки и глубина вызывают отказ с rollback. Старые профили, exact lookup и
Recalculation сохранены. [Контракт, selector и SQL path/mode/OID](docs/adr-027-corpus.md),
[команды переноса](docs/nucbox-corpus-diagnostic.md). Реальная БСП ещё не принята;
opaque содержимое, свойства и связи не объявляются семантически разобранными.

У `build-corpus` предел JSON-аргумента opaque (65536 bytes) не заменяет общий
предел канонического `build_parameters`: **16384 UTF-8 bytes вместе с guards
и бюджетами**. Превышение вызывает отказ без публикации. Имя объекта `Ext`
допустимо; классификация descriptor/payload учитывает роль сегмента пути.

## Крупные непрозрачные BIN (отдельный opt-in)

`build-corpus-bin` / `linux-v5-corpus-bin1-xml221-r09` добавляет отдельный явный
per-file бюджет BIN до **24 MiB** только для точных Template.bin, XDTO Package.bin
и перечисленных глобальных BIN. Полные тела сохраняются; обычные XML/BSL,
commit/tree, старые профили и MCP limits не повышены. Offline verify требует
`--max-opaque-bin-bytes`; MCP выдаёт небольшие byte ranges. [Контракт, команды,
лимиты и измерение памяти](docs/adr-028-corpus-bin.md). Реальная БСП ещё не принята.

## Ресурс корневой справки DataProcessor (отдельный opt-in)

`build-corpus-help` / `linux-v5-corpus-help1-xml221-r09` наследует BIN-профиль
и добавляет только `DataProcessors/<Owner>/Ext/Help/<ResourceDir>/<File>.png`.
Владелец — корень; обязательны его Help.xml и Help/ru.html. Reports и старые
профили не расширены. PNG остаётся opaque и использует обычные caps, не BIN cap.
[Точный контракт, команды и проверки](docs/adr-029-root-help.md).

Корневая `Configuration.xml` с большим количеством ссылок: явный
`build-corpus-config` / `linux-v5-corpus-config1-xml221-r09` разрешает до 16384
DOM-узлов только у этого документа. Остальные XML — 10000, byte/event/depth
и все остальные квоты прежние; старые профили неизменны.
[Контракт, identity и синтетические проверки](docs/adr-030-configuration-dom.md).

Полные XML diagnostics большого корпуса: `build-diagnostics` /
`linux-v5-diagnostics1-xml221-r09` сохраняет их отдельными bounded строками SQLite,
а metadata/MCP — только малую сводку и digest. Явная общая квота до 64 MiB,
строка до 16 KiB, общий facts/SQLite budget сохраняется; старые профили неизменны.
[DDL, точный учёт, identity и команды NucBox](docs/adr-031-diagnostics.md).

Подтверждённые формы без кода: `build-optional-form` /
`linux-v5-optional-form1-xml221-r09` разрешает 0/1 Module.bsl только формам
AccumulationRegister и ChartOfCharacteristicTypes при полном layout.
MCP различает `absent` и `unparsed`; старые профили и diagnostics bounds сохранены.
[Грамматика, статусы и проверка](docs/adr-032-optional-form.md).

Исправлен общий producer: потомки вставляются сразу с фактическим родителем.
Одноимённые подсистемы в разных ветках больше не конфликтуют временно под
Configuration; настоящий UNIQUE(parent_id, kind, name) сохраняется.
[Совместимость, ID и регрессия](docs/adr-033-final-parent.md).

Корневые Help.xml Catalog/Document/InformationRegister и Catalog Predefined.xml:
`build-root-sidecars` / `linux-v5-root-sidecar1-xml221-r09` сохраняет их как opaque
payload с реальным владельцем. Селектор прежний; HTML этих sidecar не прочитан.
[Точные пути, counts, scope и проверки](docs/adr-034-root-sidecars.md).

### Версионированные контекстные аннотации (синтетический кандидат)

`build-context` / `linux-v5-context1-xml221-r09` добавляет только точные
`&НаКлиенте`, `&НаСервере`, `&НаСервереБезКонтекста` непосредственно перед
прежним допустимым заголовком метода. Старые профили неизменны; `#If` и расширенные
заголовки по-прежнему дают `unparsed`. Исходные байты сохраняются, компиляторная
семантика не проверяется. [Контракт, ограничения и команды — ADR-035](docs/adr-035-context-annotations.md).

### Однострочные параметры без defaults

`build-parameters` / `linux-v5-parameters1-xml221-r09` сохраняет контекстные
аннотации и добавляет ограниченный список `Identifier` / `Val Identifier` /
`Знач Identifier`. Defaults, multiline и `#If` остаются `unparsed`; старые guards
не расширяются. [Грамматика, caps и проверка — ADR-036](docs/adr-036-parameter-headers.md).

### Физические условные ветки (без выбора контекста)

`build-conditionals` / `linux-v5-conditionals1-xml221-r09` добавляет ограниченную
RU Boolean grammar только для трёх подтверждённых atoms. Методы сохраняют путь
объявления по arms; одноимённые варианты остаются ambiguous. Внешнее состояние
метода/областей не может пересекать arms. Successor также сохраняет прежние длинные
имена методов, не меняя parameters1. [Контракт и DDL — ADR-037](docs/adr-037-conditional-branches.md).

### Непрозрачные строки с продолжением `|`

`build-strings` / `linux-v5-strings1-xml221-r09` добавляет opaque string state для
документированного pipe-формата, сохраняя bytes и условные пути. Defaults и
multiline headers не расширяются. Имена в 152 UTF-8 байта остаются доступны;
отдельный предел MCP exact-name — 1024 UTF-8 байта.
[Контракт и проверка — ADR-038](docs/adr-038-pipe-strings.md).
