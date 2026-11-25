# ADR-019: пять корневых видов без вложений

Основа: f56a3afe9b6d05ffce0d587186eb822bc5857457. Новый opt-in `build-leaves`,
профиль `linux-v5-leaf1-xml221-r09`. Предыдущие команды и профили не расширяются.
Схема companion1 с отдельным CHECK профиля; SQLite v5, без новых таблиц, зависимостей
или изменения исходного schema.sql. Нормализованные бюджеты и
`leaf_guard=five-leaf-roots-known-files-v1` входят в snapshot_id. Канонизация
фактов прежняя; новые строки объектов, модулей и payload входят в общий хеш.

## Явная грамматика

Каждый объект — непосредственный ребёнок Configuration. Ссылка берётся из
Configuration.xml, сохраняется её точный байтовый диапазон (включая поправку BOM).
Дескриптор — `<каталог>/<Name>.xml`, MDClasses/MetaDataObject 2.21, ожидаемый тип,
точное Name, уникальный валидный UUID. ChildObjects и неизвестные структурные
оболочки запрещены. Properties помимо Name и InternalInfo не считаются семантически
разобранными. Все прочие поддержанные предыдущими профилями виды сохраняются.

| Вид → каталог | Файлы под `<каталог>/<Name>/` |
|---|---|
| CommonPicture → CommonPictures | обязательный `Ext/Picture.xml` и ровно один `Ext/Picture/Picture.zip`, `.png`, `.svg`, `.gif` или `.ico` |
| Constant → Constants | независимо необязательные `Ext/ManagerModule.bsl` и `Ext/ValueManagerModule.bsl` |
| Role → Roles | обязательный `Ext/Rights.xml` |
| ScheduledJob → ScheduledJobs | необязательный `Ext/Schedule.xml` |
| XDTOPackage → XDTOPackages | обязательный `Ext/Package.bin` |

Для Constant допустимы все четыре сочетания наличия модулей. Владельцем каждого
является именно этот Constant, не Configuration и не соседний объект. Отсутствие
модуля не создаёт фиктивный BSL или методы. Unparsed сохраняет точный исходник и
причину отказа, методы отсутствуют. Отсутствие расписания допустимо; неизвестный
путь не интерпретируется как отсутствие.

Все файлы пяти каталогов выбираются, включая бинарные тела, расходуют явные Git,
source-count, file/total-bytes, SQLite, facts, coverage и time бюджеты. Лимиты не
повышаются автоматически. Неизвестный файл, второй формат картинки, сирота,
неверный дескриптор или пропуск обязательного файла отвергает всю сборку.
Ошибка после расчёта части фактов откатывает savepoint; writer удаляет свою
временную БД и не публикует снимок. Публикация только после полного успешного расчёта.

Все пять видов payload непрозрачны, включая XML. Ни MDClasses/2.21, ни корректность
XML, ZIP, изображения, прав, расписания или XDTO по содержимому не проверяются.
ZIP не распаковывается, файлы не исполняются. OID/SHA и bytes сохраняются точно.
`leaf_structure=complete_known_grammar` означает только эту грамматику путей и
связей. `leaf_contents=not_analyzed`, relations также not_analyzed.

Основание грамматики — переданные обезличенные агрегаты локальной замороженной БСП:
форматы Picture 597/51/29/2/1; ManagerModule 10, ValueManagerModule 79, оба у 9,
ни одного у 171; Rights 166; Schedule 54 из 60; Package.bin 69. Это сведения
постановки задачи, а не облачный прогон реального корпуса.

## Reader, verify и MCP

Open проверяет новый профиль по общему контракту; BSL повторно не разбирается.
Offline verify пересчитывает сохранённые OID/SHA, владельцев, структуру, методы и
индекс тем же ограниченным producer. Git не нужен, его происхождение не доказывается.
Сохраняются Linux x86_64, private staging, независимый expected SHA, bounded reader
и 65536 bytes на весь сериализованный JSON-RPC. Reader рассчитан на доверенные
артефакты; защиты от произвольного вредоносного SQLite этот шаг не обещает.

MCP: корень → вид → объект → модуль/метод или companions. `branch.source` даёт
дескриптор; companions содержит реальную ссылку из Configuration.xml и каждый
payload. Payload имеет роль additional, encoding=base64, content=not_analyzed;
декодированные bytes сравнивать с исходными. `payload_presence` учитывает эти
файлы и различает present_unparsed/absent. Для Constant `branch.module_presence`
отдельно перечисляет ManagerModule и ValueManagerModule: accepted/unparsed/absent.
Для ScheduledJob `branch.schedule_presence`: present_unparsed/absent. Неизвестная
грамматика отвергается builder/verify, не превращается в успешный частичный снимок.

## Воспроизведение на синтетике

```sh
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
cargo +1.85.1 test --locked --features experimental-snapshot-mcp --test mcp_snapshot leaf_objects_cli
```

Запускать последовательно при общем target. Последний тест строит снимок реальным
CLI, удаляет только созданный им синтетический Git, выполняет open и offline verify,
затем реальный stdio handshake/tree/source с пустым PATH. Проверяет все пять
форматов Picture, четыре сочетания модулей, расписание с/без файла, byte-exact BSL
и binary, BOM/CRLF/последнюю строку без LF, unparsed, реальных владельцев и ссылки.
Отрицательные случаи включают пропущенный последний XDTO payload, неверные
name/type/uuid/namespace, ChildObjects, неизвестный root/path, сирот и fact-limit;
каждый оставляет каталог публикации пустым. Повторная сборка проверяет детерминизм,
старые факты — parity. Подмена владельца при пересчитанных контрольных суммах
проходит формальный open, но отвергается verify.

[Команды переноса](nucbox-leaf-diagnostic.md). Синтетика не является приёмкой полной
БСП. Остальные корневые виды/вложения, локальная приёмка pinned корпуса и остальные
разрезы Навигатора остаются вне этого шага. Реальные данные в облаке не использовались.
