# ADR-032: подтверждённая форма без BSL

Основа `8bd6403d09d37a6d1f2d9af1f7fb4c6c6d53e7af`. Новый явный режим
`build-optional-form`, профиль `linux-v5-optional-form1-xml221-r09` наследует
[diagnostics1](adr-031-diagnostics.md). Старые профили и команды не расширяются.

Подтверждённая локальная структура закреплённой БСП: одна форма регистра накопления
имеет descriptor + Ext/Form.xml без BSL и справки; две формы плана видов
характеристик имеют descriptor/layout без BSL, одна с полной парой справки,
другая без неё. Реальные имена/данные в облако не переносились.
Код прежнего профиля требовал `modules == 1` в обоих семействах, поэтому это два
подтверждённых разрыва. Новый профиль исправляет только их.

## Грамматика и совместимость

Для формы по точным путям:

```text
AccumulationRegisters/<Owner>/Forms/<Form>.xml
ChartsOfCharacteristicTypes/<Owner>/Forms/<Form>.xml
```

разрешены ноль или один `<Form>/Ext/Form/Module.bsl`. Обязательны корректные
MDClasses XML 2.21 descriptor с совпадающим именем/типом/UUID, scalar reference
у действительного владельца и `<Form>/Ext/Form.xml`. Layout остаётся непрозрачным
payload: его содержимое не выдаётся за разобранную семантику формы.
Пара `<Form>/Ext/Help.xml` + `<Form>/Ext/Help/ru.html` независимо необязательна,
но половина пары запрещена. Нет layout, неправильный descriptor, чужой вид,
неизвестный BSL/path/layout — строгий отказ, без опубликованного снимка.

AccountingRegister, CalculationRegister, ChartOfAccounts, ChartOfCalculationTypes
сохраняют ровно один BSL формы. Recalculation сохраняет ноль. Остальные ранее
поддержанные правила не меняются. Наличие BSL определяется сохранённым источником,
а не результатом его разбора: unparsed-файл не превращается в отсутствие.

DDL наследуется от diagnostics1, меняется только profile discriminator.
`build_parameters.optional_form_guard =
accumulation-characteristic-module-0-or-1-layout-v1` и новый profile входят в
snapshot_id; старые identity и facts algorithms не меняются. Offline verify
воспроизводит тот же профиль и грамматику из sources, проверяет источники,
диагностики, владельцев и факты. Git provenance не заявляется.

## MCP и точные исходники

У ветки объекта Form двух разрешённых видов в новом профиле есть
`form_module_presence`: `absent`, `accepted`, `unparsed` (защитное `unknown` для
неизвестного статуса). Это факт сохранённого снимка. При absent нет строки в
modules, нет выдуманного источника/пустого BSL или методов; запрос отсутствующего
source возвращает ошибку отсутствия данных. При unparsed точные bytes доступны
через source byte range/Base64; методы не выдумываются. Accepted может иметь
ноль методов и отличается от отсутствия.

Форма имеет непосредственного владельца-корень, её Module.bsl принадлежит форме.
Descriptor, layout, help и BSL сохраняют реальные пути и bytes. Подмена parent
при согласованных facts/file digest выявляется offline verify.

## Запуск и пределы

Команда идентична [ADR-031](adr-031-diagnostics.md), заменить только
`build-diagnostics` на `build-optional-form`. Все 21 позиционный аргумент после имени команды, включая
последние `MAX_OPAQUE_BIN_BYTES MAX_DIAGNOSTICS_BYTES`, обязательны по прежнему
контракту. Verify/MCP запускаются без изменений; verify требует явного
`--max-diagnostics-bytes` и `--max-opaque-bin-bytes`.

Сохранены XML/BSL/Git/SQLite/facts/time caps, DOM исключение только Configuration,
строки diagnostics до 16 KiB, явная сумма до 64 MiB, metadata до 8 KiB,
полный wire JSON-RPC до 65536 bytes. Источники не исключаются из scope.
Reader по-прежнему для доверенных артефактов, Linux x86_64, с независимым SHA
и trusted staging. Реальный BSP и пользовательский Git остаются на NucBox.

## Синтетическая проверка

Реальный CLI build → удаление только временного синтетического Git → offline
verify → stdio MCP с пустым PATH: оба новых вида, без справки/с полной справкой,
accepted BSL и invalid-UTF8 unparsed BSL, точные descriptor/BSL bytes, правильные
parent/owner, отсутствие source/module при absent. Проверяется наследуемая
навигация Recalculation и diagnostics storage/limit. Повторная сборка даёт те же
snapshot_id/facts digest.

Отрицательные: missing layout, каждая половина help, неожиданный BSL,
неправильный тип descriptor, отсутствие BSL у четырёх строгих семейств,
оба старых запрета diagnostics1, поздняя ошибка и откат. Подмена parent с
пересчитанными digest проходит формальный open и отвергается verify.

```sh
cargo +1.85.1 test --locked --features experimental-snapshot-mcp optional_forms_cli -- --nocapture
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
```

Это синтетический кандидат; реальная БСП требует отдельного локального повтора.
Никаких публикаций, изменения main или доступа к реальным данным.
