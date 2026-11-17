<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/bslens-dark.svg">
  <img alt="BSLens: только нужный контекст для ИИ-агента" src="assets/bslens-light.svg" width="440">
</picture>

# BSLens

BSLens — проект навигатора по конфигурациям 1С для разработчиков и ревьюеров.
Его задача — находить объекты, модули и методы в конкретном коммите Git и
открывать нужные фрагменты исходника через локальный MCP, не читая всю выгрузку.

## Целевая архитектура

`Точный коммит Git → адаптер выгрузки 1С → индексатор Rust → снимок SQLite → CLI и MCP`

Первое дерево структуры: конфигурация → тип объекта → объект → модуль → процедура
или функция. Дерево вызовов и сравнение коммитов не входят в этот шаг.

## Что работает сейчас

Проект на ранней стадии. В `main` можно собрать ядро на Rust и запустить
тесты. Экспериментальный режим `synthetic-dev` читает метаданные и физические
строки из синтетического снимка SQLite схемы v4. По умолчанию этот режим
отключён, а его ответы помечены `synthetic-partial-NOT-PRODUCTION`.

**Рабочие снимки пока не поддерживаются:** полная проверка данных, сборка
снимков, дерево и MCP ещё не доступны в основной ветке.

## Сборка и проверки

Rust 1.85.1, зависимости закреплены в Cargo.toml и Cargo.lock:

```sh
cargo test --locked
cargo test --locked --features synthetic-dev
cargo build --locked
```

Если у вас уже есть синтетический файл `fixture.sqlite`, передайте CLI его
точный идентификатор `SNAPSHOT_ID`:

```sh
cargo run --locked --features synthetic-dev -- synthetic-dev fixture.sqlite SNAPSHOT_ID metadata
cargo run --locked --features synthetic-dev -- synthetic-dev fixture.sqlite SNAPSHOT_ID source Module.bsl 1 2
```

CLI не создаёт снимки и не читает исходники из рабочей папки Git. Вывод JSON пригоден для
автоматических проверок. В штатном CLI `metadata fixture.sqlite SNAPSHOT_ID` выдаёт отказ
до реализации полной проверки.

[Архитектура и ограничения](docs/adr-001.md) · [Проверки R01](docs/testing-r01.md)
