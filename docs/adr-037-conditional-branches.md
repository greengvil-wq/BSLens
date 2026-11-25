# ADR-037: физические условные ветки без выбора контекста

Основа `4e5f31680db4d6d32cef102ad2c4938964936cf9`. CLI `build-conditionals`,
profile `linux-v5-conditionals1-xml221-r09`, lexical_version `r09-conditionals-v1`.
Старые guards и profile artifacts не переопределяются. Defaults и multiline
заголовков всё ещё вне грамматики; контекстные аннотации и параметры наследуются.

## Только подтверждённые директивы и atoms

Директива начинается после необязательного отступа на отдельной физической строке.
Точные RU формы: `#Если Expr Тогда`, `#ИначеЕсли Expr Тогда`, `#Иначе`, `#КонецЕсли`.
После полной директивы возможен `//` комментарий. Регистр директив, операторов
и atoms **точный**. Английские варианты и другие atoms не угадываются.

```text
Expr  = And { Или And }
And   = Unary { И Unary }
Unary = Atom | Не Unary | '(' Expr ')'
Atom  = Сервер | ТолстыйКлиентОбычноеПриложение | ВнешнееСоединение
```

Приоритет используется лишь для проверки грамматики. Значения не вычисляются,
активная ветка не выбирается, код не выполняется. Нет доказательства применимости
метода к какому-либо режиму запуска. Строковые literals в выражениях запрещены;
`#Если` внутри строк или комментариев обычного BSL не становится директивой.
Inline, неизвестные/malformed/crossing директивы делают весь модуль unparsed.

## Точное состояние каждой arm

На каждой условной границе обязательны пустые открытые delimiters и отсутствие
pending-аннотации. На входе сохраняются identity внешнего метода (байтовое начало)
и точный prefix областей (физические строки открытий). При `ИначеЕсли`, `Иначе`,
`КонецЕсли` они обязаны совпадать с entry. Закрыть внешний метод или pre-existing
region даже временно и открыть заново нельзя. Новый метод внутри метода запрещён.

На уровне модуля каждая arm может содержать только целиком закрытые методы.
Внутри метода conditional arms могут содержать код тела, возвращающий тот же
контекст. Локальные области в top-level arm должны быть закрыты до её границы.
Ветвь без else допускает неявную пустую альтернативу, которая сохраняет entry;
фиктивных деклараций для неё нет. Повтор else, elseif после else, отсутствующий
endif, unmatched boundary или незакрытая структура отвергаются целиком.

## Ограничения

На модуль максимум 4096 блоков, 8 уровней conditional nesting, 64 arms на блок.
Выражение: до 4096 байт, 256 токенов и 16 уровней parentheses/unary recursion.
Общие caps 1 MiB/65536 строк/100000 токенов и region depth64 сохранены.
Лимиты файла, полного ввода, facts, diagnostics, SQLite, времени и ответа MCP не
увеличиваются. Структура веток хранится только как ограниченный stack, без
перебора комбинаций путей или копирования всех альтернатив исходника.

Отдельная подтверждённая регрессия parameters1 исправляется **только здесь**:
имя метода больше не получает новый 128-byte token cap. Для него действует прежний
общий предел исходника; его bytes исключены из новых 8192 header bytes.
Параметры по-прежнему ограничены 64 именами, 128 UTF-8 bytes/token, 200 header
токенами. Parameters1 остаётся прежним, включая прежний отказ длинных имён.
В successor проверены имена 152 и 10000 UTF-8 байт; 152-byte имя прошло stdio/source.
Это не изменение лимита имени в bounded ответах reader/MCP.

## DDL и канонические факты

Только новый профиль добавляет таблицу:

```sql
CREATE TABLE method_branches (
  method_id INTEGER PRIMARY KEY REFERENCES methods(method_id),
  branch_path TEXT NOT NULL CHECK(length(CAST(branch_path AS BLOB)) <= 4096)
) STRICT;
```

Ровно одна строка на физический метод, включая `[]` у безусловного объявления.
Каждая запись пути от внешнего блока к внутреннему содержит `block_line`,
`arm` (с нуля), `kind` (`if`, `elseif`, `else`), `arm_line`,
`directive_start_byte`, `directive_end_byte`. Диапазон указывает на точные bytes
строки открывающей эту arm директивы с отступом/комментарием, без CRLF/BOM.
Тело условия доступно из сохранённого source. Путь описывает **объявление**,
а не все условия внутри тела; для method, объявленного вне conditional, путь пуст.

PK/FK и полный inventory проверяются. Строка path считается дополнительным фактом
и ограничена 4096 bytes; размер БД под общей SQLite квотой. JSON канонически
воспроизводится общим producer. Новый facts domain `BSLens-v5/facts/conditional1`
добавляет path с ключом module_path/start_line и наследует все diagnostics/objects/
modules/methods/source_lines/owners. Старые facts domains не меняются.
Profile, lexical_version, whitelist, grammar, caps и исключение имени метода из
новых budgets входят в нормализованные build_parameters/identity. Исходники,
selector, XML provenance и diagnostics digest сохраняются.

Offline verify из сохранённых sources повторяет тот же guard и path, без Git.
Удаление или подмена evidence не маскируются согласованными внешними digest:
формальный open проверяет целостность, verify сравнивает пересчитанные факты.

## MCP и воспроизводимая проверка

У method.location появились `declaration_branch_path` и
`conditional_evaluation=not_evaluated` только в новом профиле. Одноимённые методы
разных arms остаются несколькими физическими candidates, lookup возвращает
ambiguous. Никакой active branch не выбирается. Exact method/source byte/line
ranges сохраняются, BOM/CRLF не нормализуются. Unparsed не означает отсутствие.

Команда и 21 аргумент по [ADR-034](adr-034-root-sidecars.md): имя build заменить на
`build-conditionals`. Verify/stdio остаются прежними; независимый expected SHA,
trusted staging, явные BIN/diagnostics quotas обязательны по прежнему контракту.

```sh
cargo +1.85.1 test --locked --features experimental-snapshot-mcp conditional_ -- --nocapture
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
```

Реальный synthetic build → удаление только временного Git → offline verify →
stdio с пустым PATH проверяет top/body/nested/implicit arms, evidence, ambiguity,
точные bytes и длинное имя, unparsed/crossing, quota rollback и подмену
branch_path при пересчитанных digest. Unit tests проверяют границы 8/9, 4096/4097,
64/65, expressions/comments и запрет временного закрытия entry state.

Переданная frozen статистика — основание для whitelist, не результат этого
кандидата. Реальная БСП и её данные здесь не используются. Эффект на accepted и
число методов требует отдельного локального повтора; main/GitHub не меняются.
