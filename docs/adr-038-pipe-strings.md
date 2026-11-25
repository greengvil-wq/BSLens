# ADR-038: непрозрачные многострочные строки с `|`

Основа `08edd98508cdd2ad1957c7d0569627383d10b715`. Новый CLI `build-strings`,
profile `linux-v5-strings1-xml221-r09`, lexical_version `r09-pipe-strings-v1`.
Старые guards/profile artifacts остаются прежними.

Основание: [1C:Enterprise 8.3.27 Developer Guide, §4.3.6 String](https://1c-dn.com/library/tutorials/1c_enterprise_developer_guide_8_3_27/?sphrase_id=1678862),
проверено по официальной странице. Раздел описывает продолжение незакрытого
строкового литерала через `|`, удвоение кавычек и отдельные строки-комментарии.
В этой задаче значение строки не вычисляется.

## Точный срез guard

После открывающей кавычки без закрытия на физической строке начинается opaque
state. Каждая следующая строка после отступа только из ASCII space/tab обязана
начинаться с `|` либо `//`. После `|` все символы — содержимое строки до первой
неудвоенной кавычки. `""` остаётся внутри литерала. `//` после `|` — тоже данные,
а не комментарий. Comment-only строка не закрывает литерал, даже если содержит
кавычки, директивы или похожие на метод слова. Пустая строка без `|`, неверный
отступ/маркер и EOF в opaque state отвергаются как MultilineString.

Внутри opaque state не распознаются скобки, объявления, аннотации или директивы.
При закрытии обычный токенизатор продолжает **с оставшегося suffix той же строки**.
Он проверяет delimiters и неизвестные токены; inline method/end/directive не
становятся новой границей. Допускается открытие следующего строкового литерала
в suffix обычного выражения. Это не оценка выражения или типа значения.

Defaults и multiline headers не добавляются: заголовок по-прежнему обязан
полностью соответствовать одноcтрочной грамматике без defaults. Второй описанный
в руководстве вариант с отдельно закрытыми строковыми фрагментами не объявляется
новым покрытием и не получает новой логики объединения. Старые однострочные
строки обрабатываются как раньше.

## Байты, состояние ветки и лимиты

Исходные bytes не изменяются и не собираются в новое значение строки. Guard
хранит только opaque state/строку открытия, сканирует физические фрагменты и
сохраняет прежние offsets. BOM/CRLF, отступы, `|`, комментарии и EOF без LF остаются
в sources. Method/header/source_lines ranges не пересчитываются по нормализованному
тексту. Содержимое строки не меняет conditional branch evidence или region state.
Настоящие условные границы после закрытия по-прежнему обязаны восстановить entry.

Пределы не увеличены: 1 MiB на лексический исходник, 65536 физических строк,
100000 внешних токенов. Opaque literal учитывается одним токеном при открытии;
его продолжения и comment-only строки ограничены исходными bytes/lines.
Глобальная проверка UTF-8 и управляющих символов выполняется и внутри строк.
Остальные source/facts/diagnostics/SQLite/time/transport caps наследуются.

Имена методов в 152 UTF-8 байта остаются доступны. Общий legacy lexical bound
имени сохранён из conditionals1, но **MCP exact-name ограничен 1024 UTF-8 байтами**;
это самостоятельный предел запроса/ответа, не обещание навигации по любому имени,
которое способен принять лексический guard. Лимит MCP здесь не расширяется.

## Воспроизводимость и совместимость

В build_parameters/identity записаны новый guard, точная грамматика продолжения,
indent/comment/quotes policy, отсутствие вычисления, defaults и multiline headers.
DDL наследует method_branches conditionals1, меняется profile discriminator.
Условные факты, их digest, sources/owners, XML diagnostics/count/digest, selector
и coverage сохраняются. Общий producer выбирает тот же guard в build и offline
verify; повторного Git или значения вычисленной строки не требуется.

Ошибка делает весь модуль unparsed, без частично опубликованных методов;
исходник остаётся доступным. Accepted с нулём методов и unparsed не смешиваются.
MCP не выбирает активную условную ветку, одноимённые candidates остаются ambiguous.

Команда и 21 аргумент по [ADR-034](adr-034-root-sidecars.md): имя build заменить
на `build-strings`. Verify/stdio прежние, с независимым expected SHA, trusted
staging и явными BIN/diagnostics quotas.

```sh
cargo +1.85.1 test --locked --features experimental-snapshot-mcp pipe_strings -- --nocapture
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
```

Реальный synthetic CLI → удаление только временного Git → offline verify → stdio
с пустым PATH проверяет BOM/CRLF, fake method/conditional/annotation spellings,
удвоенные кавычки, comment-only строки, точные saved bytes, условный путь,
ambiguity и 152-byte имя. Negative fixtures проверяют malformed/EOF, defaults,
multiline headers и проверку suffix. Сохранены quota rollback и отказ verify при
подмене branch evidence с пересчитанными внешними digest. Unit tests проверяют
предел 65536/65537 строк и превышение 1 MiB.

Реальные данные БСП не использовались; изменение accepted/method counts неизвестно.
Main/GitHub не меняются. Это кандидат для отдельного локального повтора, не приёмка.

## Статус проверки этого кандидата

Rust 1.85.1: cargo check --locked --all-features и два focused pipe_strings tests
прошли, включая реальный CLI/remove synthetic Git/verify/stdio. Первый полный
all-features прогон завершил 106 тестов в успешных suites, затем pipeline suite
отказал на создании Git-процессов: EAGAIN/Resource temporarily unavailable.
В облаке обнаружены 32369 zombie Git processes под PID1, оставшиеся от прежних
запусков. Последовательный повтор с одним build job/test thread и локальным
subreaper только для собственных потомков не помог: rustc не может создать поток.
Лимиты ОС и чужие процессы не изменялись.

Полный all-features, отдельный R09/default и offline rebuild архива требуют
свежего облачного окружения. Этот кандидат не объявляется полностью проверенным.
fmt/diff прошли до исчерпания процессов; продуктовые ограничения не ослаблялись.
