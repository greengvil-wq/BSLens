# ADR-033: окончательный родитель при INSERT

Основа `e5fc45d3185c2900a895343fdf72d19883577c04`. Исправление общего
`streaming_facts::record_object`: parent_id передаётся явно и записывается сразу.

* Configuration: NULL.
* Корневые виды из Configuration: ID Configuration (1).
* Внешние потомки: фактический непосредственный parent из очереди обхода.

Удалён последующий UPDATE parent_id. Это распространяется на Subsystem, Form,
Template и Recalculation во всех вызовах общего producer, включая offline verify.
Ни временного родителя Configuration, ни отключения UNIQUE/FK теперь нет.

## Почему без нового профиля

Это исправление алгоритма записи уже разрешённой структуры, не новая грамматика.
UNIQUE(parent_id,kind,name) всегда выражал уникальность **у непосредственного
владельца**. Временная вставка всех потомков под Configuration ошибочно запрещала
дочернюю подсистему с именем корневой. Последующий UPDATE не успевал выполниться.

Переданная локальная проверка закреплённой БСП: 18 корней, 176 детей, 42 внука;
одна дочерняя подсистема совпадает именем с корневой, 23 имени повторяются в разных
ветках, настоящих дублей под одним родителем нет. Реальные данные не переносились.

Поэтому старые профили, DDL, UNIQUE, FK, identity/domain и лимиты не изменены.
Ранее успешные сборки сохраняют итоговые строки и ID: сортировка root refs,
очередь потомков и счётчик objects прежние; новое значение INSERT равно прежнему
значению после UPDATE. Неуспешный из-за временного ложного конфликта разрешённый
вход теперь принимается и в старом поддерживающем его профиле. Старые снимки
переписывать не требуется. Это осознанная совместимая коррекция producer;
если ранее сохранённый файл имеет неправильного родителя, offline verify
по-прежнему выявляет несоответствие.

## Проверка

`final_parent_cli_homonyms_offline_stdio_and_real_duplicate_rollback` создаёт только
синтетический Git и запускает настоящий `build-optional-form`:

* root Subsystem Same и child Same под Other, а также под Same и на следующей
  допустимой глубине; одинаковые имена у разных непосредственных родителей;
* одноимённые Form, Template и Recalculation под разными корневыми объектами;
* optional Form без BSL и форма с BSL, наследуемое сохранение diagnostics;
* повторяемые snapshot_id/facts digest и foreign_key_check;
* удаление своего Git, offline verify и реальный stdio MCP с пустым PATH;
* точный parent, descriptor bytes и reference range в XML владельца, точные BSL;
* повтор ссылки у одного parent отвергается без опубликованного снимка;
* самостоятельная проверка SQL UNIQUE возвращает 2067 и откатывается savepoint.

Тот же новый тест на неизменном исходном коде exact e5fc45d в отдельном каталоге
воспроизводит ложный UNIQUE 2067. На исправленном коде он проходит.
Отдельный ручной parity-прогон baseline/candidate на ранее допустимой синтетике
с одноимёнными формами/макетами подтвердил равенство snapshot_id, facts_sha256 и
полных строк objects (включая ID), modules, methods, source_lines, companions,
companion_payloads, xml_diagnostics. Это отдельная проверка, не единый CI-тест.
Полный набор также проверяет ранее успешные профили, golden facts/identity,
подмену владельцев с пересчитанными digest, optional forms и bounded diagnostics.

```sh
cargo +1.85.1 test --locked --features experimental-snapshot-mcp final_parent_cli -- --nocapture
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
```

Команды NucBox и аргументы остаются по [ADR-032](adr-032-optional-form.md) и
[ADR-031](adr-031-diagnostics.md): build-optional-form → verify → stdio MCP.
Новых флагов, схем или ослабления лимитов нет. Проверена синтетика Linux x86_64;
реальная БСП требует отдельного повтора на NucBox. Рабочий Git удалять нельзя.
GitHub/main и реальные данные не затрагиваются; семантика XML payload и
Git provenance не объявляются проверенными.
