# ADR-030: ограниченный DOM корневой Configuration

Основа `346a2e2e2799d1703229b24cb8c79274981b9f8a`. Явная команда
`build-corpus-config`, профиль `linux-v5-corpus-config1-xml221-r09` наследуют
[corpus-help1](adr-029-root-help.md). Единственное расширение: для точного пути
`Configuration.xml` и ожидаемого типа `Configuration` предел roxmltree составляет
16384 DOM-узла вместо 10000. Считаются также document, текст и комментарии.
Все другие структурные XML сохраняют 10000; старые команды и профили неизменны.
Пределы 2 MiB XML, 20000 событий quick_xml (включая EOF), глубины 64, прочие
source/Git/facts/SQLite/time budgets, BIN policy и MCP budgets не повышаются.

Основание — переданная локальная диагностика закреплённой БСП: 3607 ссылок,
11490 DOM-узлов при 345959 bytes, 15300 событиях и глубине 6. Это не облачный
прогон БСП и не подтверждение её полной приёмки. Никаких реальных данных в тестах.

В канонических build_parameters сохраняется `xml_limits.nodes=10000`, добавлено
`configuration_xml={path:"Configuration.xml",kind:"Configuration",nodes:16384,
guard:"root-dom16384-v1"}`. Оно вместе с profile участвует в identity. Предел
канонических параметров 16384 bytes остаётся прежним. DDL меняет только profile,
таблицы, source selector, OID, точные bytes и канонизация фактов прежние.
Build и offline verify вызывают общий расчёт с этим фиксированным правилом;
reader/MCP распознают новый профиль. Старые profile grammars не расширены.

## Воспроизведение

Использовать команду из ADR-029 с заменой **только** `build-corpus-help` на
`build-corpus-config`; все 21 аргумент и явные квоты сохраняются. Verify требует
прежний явный `--max-opaque-bin-bytes`, MCP — snapshot/trusted staging/SQLite cap
и независимый expected SHA. Источники БСП остаются на NucBox; удалять рабочий Git
нельзя. Удаление репозитория ниже выполняет только синтетический тест.

```sh
cargo +1.85.1 test --locked --features experimental-snapshot-mcp configuration_ -- --nocapture
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
```

Сквозной тест `configuration_dom_cli_3607_refs_offline_verify_stdio_and_limits`:
реальные CLI build → удаление собственного Git → offline verify → stdio MCP
с пустым PATH. Configuration: 3607 ссылок, 97627 bytes, 10829 DOM, 14441 событий,
глубина 4. Lookup последнего имени и точные XML/BSL bytes, deterministic reuse,
сохранённые metadata и лимит ветки 128. Старый профиль отвергает этот XML.
Отдельные CLI случаи: ровно 16384 DOM проходит и offline проверяется; 16385,
события >20000 и другой структурный XML с 10001 DOM отвергаются без публикации
снимка. Parser probes отдельно проверяют точные границы DOM/event/depth/bytes
и недопустимость исключения для другого ожидаемого типа.

Это узкая синтетическая проверка. Неподдержанное содержимое по-прежнему даёт
строгий отказ; семантика непрозрачных payload и Git provenance не заявляются.
