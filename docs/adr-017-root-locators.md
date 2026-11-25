# ADR-017: явный корневой локатор и 11 descriptor-only видов

Основа — 59bdc1d7584c305ef915932e8b0f18dd2a9d7acc. Новая команда `build-roots`,
профиль `linux-v5-roots1-xml221-r09`. Старые профили не расширяются. DDL остаётся
companion1 с новым CHECK профиля, SQLite v5; таблицы и зависимости не добавлены.
В snapshot_id входят новый профиль, прежние нормализованные бюджеты и
`root_locator_guard=descriptor-only-allowlist-v1`. Старая канонизация фактов прежняя.

## Основание и границы

По переданному результату локальной классификации замороженной БСП: 3607/3607
валидных корневых дескрипторов, 42 вида, 22 с вложениями, 9 без вложений с
компаньонами и 11 descriptor-only. Реальные имена/тела в облако не передавались;
файл самой классификации в workspace отсутствует. Здесь реализован только
перечисленный узкий контракт; это не повторная проверка 3607 дескрипторов.

Точное имя `FunctionalOptionsParameter` независимо подтверждено
[официальным API 1С:EDT](https://edt.1c.ru/dev/edt/2025.2/apidocs/com/_1c/g5/v8/dt/metadata/mdclass/FunctionalOptionsParameter.html).
Это подтверждение имени типа, не доказательство каталога конкретной БСП.
Локаторы каталогов ниже — явный контракт данного кандидата, проверенный на
синтетике; полная локальная приёмка на pinned БСП остаётся отдельным шагом.

## Таблица грамматики

| XML-тип | Каталог |
|---|---|
| CommandGroup | CommandGroups |
| CommonAttribute | CommonAttributes |
| DefinedType | DefinedTypes |
| EventSubscription | EventSubscriptions |
| FilterCriterion | FilterCriteria |
| FunctionalOption | FunctionalOptions |
| FunctionalOptionsParameter | FunctionalOptionsParameters |
| Language | Languages |
| PaletteColor | PaletteColors |
| SessionParameter | SessionParameters |
| StyleItem | StyleItems |

Таблица `ROOT_LOCATORS` также содержит прежние Catalog, Document,
InformationRegister, CommonModule: новый профиль использует общий локатор для
всех поддержанных корневых ссылок. Configuration.xml — фиксированная точка входа.
Form/Template остаются отдельными companion-ссылками, не корневыми локаторами.
Нет pluralization, угадывания соседних файлов, альтернативного spelling или
fallback по содержимому. `FilterCriterions`, `FunctionalOptionParameter`, изменение
регистра и неизвестные корневые типы отвергаются. Путь — `<каталог>/<Name>.xml`,
имя проверяется прежним identifier, затем path policy и пределом 1024 байта.

Дескриптор: MetaDataObject MDClasses v2.21, один элемент точного типа, один
валидный unqualified UUID, одна прямая Properties/Name с точным именем из ссылки.
UUID уникален среди всех объектов. ChildObjects запрещён, в том числе пустой.
Другой прямой дочерний элемент тоже отвергается, кроме Properties и InternalInfo;
семантика содержимого этих оболочек не анализируется. InternalInfo и неизвестные
свойства остаются unverified, не превращаются в структурные факты или связи.

Весь Git-инвентарь в 11 каталогах проверяется до публикации. Допустимы только
корневые `.xml`; вложенный или посторонний файл означает отказ, даже если это
невыбранный binary. Это исключает незаметный пропуск companions через excluded.
XML без ссылки, недостающий XML, неверные name/type/UUID/namespace, повторная
ссылка, неизвестный тип/вложение — строгий отказ. Если у типа в другой выгрузке
появятся вложения/companion, этот профиль не выдаст неполноту за успешную структуру.

## Reader, verify, MCP

Пересчёт остаётся пофайловым из sources, с общими source/Git/facts/SQLite/time
бюджетами и атомарным rollback. Offline verify проверяет OID/SHA исходников и
пересчитывает факты тем же локатором/парсером. Он не доказывает Git provenance.
Open проверяет формальную целостность, не разбирает XML/BSL заново.

В MCP путь: корень → вид → объект. У descriptor-only объекта нет фиктивной группы
модулей/inline/companions: `items=[]`, `branch.descriptor_only=true`. Реальный XML
доступен через `branch.source` (saved_path, start_byte, end_byte). Это отсутствие
вложенной структуры, а не утверждение о разобранной семантике свойств. Для большого
XML диапазон нужно разделить; reader и весь JSON-RPC сохраняют прежние лимиты.
BOM и остальные bytes возвращаются точно, payload/BSL прежних типов не меняются.

## Воспроизведение

```sh
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
cargo +1.85.1 test --locked --all-features --test mcp_snapshot root_locators_cli
```

Запускать сборки последовательно в одном target. Тест реального CLI создаёт
синтетический Git, строит все 11 видов вместе с прежними объектами, проверяет
parity общих фактов и детерминизм, затем удаляет только свой Git. Offline verify,
open и реальный stdio MCP работают с пустым PATH: дерево, точный XML с BOM,
сохранённый BSL CommonModule. Неверное имя объекта с пересчитанными facts/file SHA
проходит open, но отвергается verify. Отрицательные случаи проверяют rollback,
включая невыбранный binary companion и ошибочное множественное число каталога.

Параметры `build-roots` те же 18 позиционных аргументов, что у build-common-modules;
[команды локальной диагностики и переноса](nucbox-roots-diagnostic.md).
Реальная БСП и все восемь разрезов Навигатора ещё не приняты. Это Linux x86_64
прототип для доверенных артефактов, не sandbox произвольного SQLite. Остальные
корневые типы, их вложения/companion и семантический анализ не входят в этот шаг.
