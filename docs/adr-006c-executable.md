# R06-C. Запускаемый stdio endpoint

`bslens-mcp` — небольшой экспериментальный executable над готовым CoreSession.
Он позволяет MCP-клиенту пройти дерево выбранного коммита до метода и прочитать
точные строки. Это не HTTP-сервис и не production-проверка конфигурации 1С.

## Запуск

```sh
cargo build --locked --features experimental-router --bin bslens-mcp
./target/debug/bslens-mcp --repo /absolute/path/to/repository --commit FULL_COMMIT_SHA
```

При заполненном Cargo cache можно добавить `--offline`. Binary включён только при
feature experimental-router. По умолчанию этот target не собирается.

Флаги --repo и --commit обязательны, каждый ровно один раз. Порядок произвольный.
--repo принимает только абсолютный путь; относительный отвергается до открытия core,
без вывода самого пути. Usage явно требует ABSOLUTE_PATH.
Неизвестные аргументы отвергаются. Не принимаются HEAD, имя ветки, краткий SHA и
revision expressions. Используются доверенный системный Git и неизменённые Limits
ядра; flags для выполнения произвольных команд отсутствуют.

Core читается один раз до начала протокола. Отсутствующий/неподдержанный repository,
неверный SHA, неподдержанная платформа или XML вызывают nonzero exit. stdout остаётся
пустым, stderr содержит только краткую фиксированную ошибку без пути, SHA и исходников.
После старта stdout содержит исключительно NDJSON MCP frames; диагностик/баннера нет.

Клиент выполняет initialize → notifications/initialized → tools/list → tools/call.
Схемы tools/list содержат обязательные commit/scope_id, source hint метода подходит
для последующего source-вызова. Встроенной команды quit нет: неизвестный RPC method
получает -32601. Нормальное завершение — EOF stdin. Исчерпание session budget также
закрывает сеанс. Незавершённый или oversized frame даёт protocol error и nonzero exit.
Ошибка ввода-вывода — nonzero exit с фиксированным stderr.

## Проверено отдельным процессом

Три Rust integration test запускают настоящий OS subprocess, не вызывают router
напрямую. Используются только новые временные авторские Git-репозитории:

- initialize → tools/list → tree → module → method → source → EOF;
- точные русские UTF-8/CRLF строки, commit/scope, отсутствие stdout banner;
- unknown quit method и последующее нормальное EOF;
- partial JSON без LF и oversized frame: ограниченный protocol error и nonzero exit;
- отсутствующие/повторные/неизвестные флаги и HEAD: nonzero, пустой stdout,
  никаких путей/SHA/source в stderr;
- исходный BSL файл неизменён после завершения.

Первый прогон: Rust 1.85.1, locked/offline, 3/3 успешно (0.24 s выполнения тестов).
Это проверка запуска на маленьких синтетических данных, не benchmark production
и не сравнение с Windows/БСП. Lifecycle/router/lexical core в R06-C не менялись.
Production ValidatedSnapshot остаётся закрытым; GitHub push не выполнялся.
