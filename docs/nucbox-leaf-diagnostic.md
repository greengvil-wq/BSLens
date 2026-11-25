# Перенос исходников и диагностический прогон только внутри NucBox

**Текущий профиль неполон.** Содержимое Form/Template не анализируется, а корневые типы вне
Configuration/Catalog/Document/InformationRegister/CommonModule и 11 descriptor-only видов плюс CommonCommand/CommonForm/CommonTemplate и CommonPicture/Constant/Role/ScheduledJob/XDTOPackage ADR-019 не поддержаны. Наличие неподдержанных корневых типов
вызывает отказ. Успех синтетики или запуск команды на БСП не означает приёмку.
Для приёмки всего «Навигатора» нужны полный build, offline verify и ручная сверка
дерева/строк на закреплённой БСП; отказ записывается как blocker, не как success.

Комплект `bslens-leaf-source-<SHA>.tar.gz` создаётся локально через git archive
точного кандидата. Это минимальная build-source выборка exact commit: Cargo.toml/Cargo.lock, src,
три SQL-ресурса, документация и tests с синтетическими fixtures. В ней нет .git,
target, бинарников, toolchain/cache, credentials или реальных выгрузок БСП/ERP.
Полный патч/bundle для ревью выдаются отдельно. Синтетические тесты включены
для воспроизведения; рабочие данные на принимающей стороне не нужны для них.
Рядом выдаются exact commit и SHA-256 архива. Архив не опубликован и не перенесён
на NucBox автоматически. Поддерживаемый способ: скачать/скопировать локальный
артефакт через разрешённый пользователю канал, проверить checksum на принимающей
стороне и распаковать в НОВЫЙ каталог. Никаких попыток обхода Library/PR/access.
Если интерфейс не предоставляет файл из workspace, это blocker передачи: нужен
разрешённый канал доставки именно этого архива. Текст SHA не заменяет файл.

Архив не содержит зависимостей Rust. Если NucBox offline и точных зависимостей
Cargo.lock/toolchain 1.85.1 ещё нет в его доверенном локальном cache, сборка будет
блокирована. Нельзя скрытно выходить в сеть или снимать --locked. Vendoring либо
перенос официального toolchain/зависимостей требуют отдельного разрешённого канала.

## Команды на NucBox (здесь не выполнялись)

Подставить полный закреплённый commit/tree из доверенной локальной записи.
Из контекста известны только префиксы `554b228…` и `4c4315fb…`; они **не годятся**
как аргументы builder и не расширяются догадкой. Не делать checkout/reset/clean
рабочего репозитория. Git ниже используется только read-only, источники остаются
на NucBox. Выход/staging/log — отдельные новые приватные каталоги пользователя.

```sh
# После проверки SHA-256 и распаковки исходников в новый каталог:
set -eu
rustup run 1.85.1 cargo build --locked --offline --features experimental-snapshot-mcp --bin bslens-v5 --bin bslens-mcp

# Все пути и полные OID задаются вручную из доверенной локальной записи:
export BSP_REPO=/absolute/existing/repo
export BSP_COMMIT=FULL_PINNED_COMMIT
export BSP_TREE=FULL_PINNED_TREE
# Полные значения сверяются с доверенной локальной записью, а не только префиксами.
case "$BSP_COMMIT" in 554b228*) ;; *) exit 1;; esac
case "$BSP_TREE" in 4c4315fb*) ;; *) exit 1;; esac
# Read-only проверка совпадения tree с полным commit:
test "$(git -C "$BSP_REPO" rev-parse "$BSP_COMMIT^{tree}")" = "$BSP_TREE"

# Заранее создать private output/staging (0700) с допустимыми родителями.
# Лимиты НУЖНО согласовать для локального стенда; это имена, не скрытые defaults:
./target/debug/bslens-v5 build-leaves "$BSP_REPO" "$BSP_COMMIT" "$BSP_TREE" "$SOURCE_ID" "$OUTPUT" \
 "$MAX_SOURCES" "$MAX_SOURCE_BYTES" "$MAX_TOTAL_BYTES" "$MAX_SQLITE_BYTES" \
 "$GIT_ENTRIES" "$GIT_TREES" "$GIT_READS" "$GIT_OBJECT_BYTES" "$GIT_TOTAL_BYTES" "$GIT_TIME_MS" \
 "$MAX_FACTS" "$MAX_COVERAGE_BYTES" "$FACTS_TIME_MS"
```

Если build отказал — остановиться, сохранить только разрешённую обезличенную
диагностику. Не обходить отказ повышением чисел и не запускать verify на чужом файле.
При успехе задать `SNAPSHOT` из результата и `EXPECTED_SHA` из доверенной независимой
записи/проверки. Сам рабочий BSP_REPO **не удалять**: удаление fixture Git выполняли
только синтетические тесты. Отсутствие Git у runtime проверяется пустым PATH.

```sh
V5="$PWD/target/debug/bslens-v5"
MCP="$PWD/target/debug/bslens-mcp"
env PATH= "$V5" verify "$SNAPSHOT" --staging-dir "$STAGING" --max-sqlite-bytes "$MAX_SQLITE_BYTES" --expected-sha256 "$EXPECTED_SHA" \
 --max-sources "$MAX_SOURCES" --max-source-bytes "$MAX_SOURCE_BYTES" --max-total-bytes "$MAX_TOTAL_BYTES" --max-scratch-bytes "$MAX_SCRATCH_BYTES" \
 --max-facts "$MAX_FACTS" --max-coverage-bytes "$MAX_COVERAGE_BYTES" --facts-time-ms "$FACTS_TIME_MS"
env PATH= "$MCP" --snapshot "$SNAPSHOT" --staging-dir "$STAGING" --max-sqlite-bytes "$MAX_SQLITE_BYTES" --expected-sha256 "$EXPECTED_SHA"
```

Последний процесс принимает JSON-RPC по одной строке. Пример начала сессии:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"local-acceptance","version":"1"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"metadata"}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"tree"}}
```

Для форм/макетов пользоваться группой companions и `source` из её ответа;
payload всегда декодировать как Base64, не считать BSL. Для длинных XML/payload
читать диапазоны частями в пределах бюджета. Подробности — ADR-019.

Далее пользоваться `children` из ответа: тип → объект → группа inline → табличная
часть → реквизит. Для source передавать возвращённый `source` объект, не угадывать
ID/пути/диапазоны. Проверить XML path и bytes против исходной локальной записи;
аналогично выбрать BSL метод и сверить точные строки, BOM/CRLF/последнюю строку.
UTF-8 data повторно кодируется UTF-8, Base64 декодируется стандартно. Не передавать
реальные ответы/исходники в облако. Коды отказа, счётчики и согласованные агрегаты
достаточны для сообщения о blocker.

Для общего модуля: Configuration → CommonModule → объект → Ext/Module.bsl → метод.
При unparsed проверить reason и отсутствие методов, затем запросить source по
физическим строкам и сверить локально точные bytes. Никакого анализа серверных/
клиентских свойств или подтверждения Git-происхождения это не выполняет.

Для descriptor-only вида: корень → вид → объект → branch.source. Пустые items
здесь сопровождаются descriptor_only=true и точным XML; это не анализ свойств.
Проверить список 11 локаторов ADR-019 против локальной pinned классификации.

Для общего объекта проверить собственника модуля и список companions. Form.xml
обязателен, Module.bsl у CommonForm может отсутствовать. Справка формы и Help-макета
должна присутствовать полной парой XML/HTML. XML payload не считать дескриптором;
проверять точные bytes, после Base64-декодирования. Отказ не обходить пропуском файлов.

Для пяти новых видов проверить точные companions и обязательность по ADR-019.
У Constant оба модуля независимо необязательны; branch.module_presence показывает
каждый отдельно. У ScheduledJob отсутствие Schedule.xml допустимо. Picture должен
содержать ровно один из пяти форматов плюс Picture.xml. Никакой ZIP не распаковывать
и payload не исполнять. Неизвестный путь — blocker, а не основание пропускать файл.
