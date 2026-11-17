<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/bslens-dark.svg">
  <img alt="BSLens: только нужный контекст для ИИ-агента" src="assets/bslens-light.svg" width="440">
</picture>

# BSLens

BSLens помогает разработчику и ревьюеру ориентироваться в конфигурации 1С.
Цель: находить объекты, модули и методы в конкретном Git-коммите и открывать
проверяемые фрагменты исходника через локальный MCP, не читая всю выгрузку.

## Архитектура

`Точный коммит Git → адаптер выгрузки 1С → индексатор Rust → снимок SQLite → CLI и MCP`

Первое дерево структуры: конфигурация → тип объекта → объект → модуль → процедура
или функция. Дерево вызовов и сравнение коммитов не входят в этот шаг.

## Что работает сейчас

Это новый самостоятельный проект, а не продолжение истории старого Python-репозитория.
Согласованное направление: библиотека ядра Rust, поверх неё отдельный MCP-адаптер.
В R01 реализован только экспериментальный вертикальный срез чтения metadata и физических
строк из синтетического снимка schema v4. Деревья, MCP, создание снимков и полная проверка
фактов ещё не перенесены.

**Рабочие снимки пока читать нельзя.** `ValidatedSnapshot::open` всегда отказывает.
Экспериментальная реализация отсутствует в сборке по умолчанию. Для авторских тестовых
данных существует отдельная возможность `synthetic-dev`; её ответы явно помечены
`synthetic-partial-NOT-PRODUCTION`. Это не флаг отключения production-проверки: тип
`ValidatedSnapshot` через этот путь получить невозможно.

## Сборка и проверки

Rust 1.85.1, зависимости закреплены в Cargo.toml и Cargo.lock:

```sh
cargo test --locked
cargo test --locked --features synthetic-dev
cargo build --locked
```

Экспериментальный CLI требует точный ожидаемый идентификатор снимка:

```sh
cargo run --locked --features synthetic-dev -- synthetic-dev fixture.sqlite SNAPSHOT_ID metadata
cargo run --locked --features synthetic-dev -- synthetic-dev fixture.sqlite SNAPSHOT_ID source Module.bsl 1 2
```

CLI не создаёт снимки и не читает исходники из рабочей папки Git. Вывод JSON пригоден для
автоматических проверок. В штатном CLI `metadata fixture.sqlite SNAPSHOT_ID` выдаёт отказ
до реализации полной проверки. Python не нужен для сборки, тестов или выполнения Rust.

[Архитектура и ограничения](docs/adr-001.md) · [Проверки R01](docs/testing-r01.md)

## R02: отдельный экспериментальный XML-слой

Возможность `experimental-xml` добавляет библиотечный `xml221::Xml221Subset::inspect`:
ограниченные bytes XML 2.21 → поддержанные объекты и точные привязки модулей.
Она не включает MCP/CLI/XML-импорт и не превращает результат в проверенный Git-снимок.
Неподдержанные классы и вложенные сущности дают явный отказ. Отдельный код не меняет
закрытую production-границу R01.

```sh
cargo test --locked --features experimental-xml
cargo test --locked --all-features
```

[Решение R02](docs/adr-002.md) · [Проверки R02](docs/testing-r02.md)
