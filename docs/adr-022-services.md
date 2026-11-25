# ADR-022: структурные HTTPService, WebService и IntegrationService

Основа 31c4a204a4277bde9289eef310521b3fb62b6dd2. Новый opt-in `build-services`,
профиль `linux-v5-service1-xml221-r09`, Linux x86_64. Предыдущие профили не расширены.
Нормализованные бюджеты прежние; `service_guard=service-inline-xml221-v1` входит
в identity. Добавлены только три корневых вида и подтверждённая inline-структура.

## Грамматика и доказательства

| Корень → каталог | Допустимые inline-дети |
|---|---|
| HTTPService → HTTPServices | URLTemplate → Method |
| WebService → WebServices | Operation → Parameter |
| IntegrationService → IntegrationServices | IntegrationServiceChannel |

Дескриптор: `<каталог>/<Name>.xml`, MDClasses/MetaDataObject 2.21, точный тип и
Properties/Name, валидный уникальный unqualified uuid. Для каждого корня обязателен
ровно один известный `<каталог>/<Name>/Ext/Module.bsl`. Других файлов в этих
семействах профиль не принимает; inline-объявления отдельных файлов не имеют.
Все файлы семейств выбираются под явными лимитами, неизвестные не исчезают в excluded.

Каждый inline-узел имеет ровно один unqualified uuid и прямой Properties с одним
скалярным непустым Name. Сохраняются kind, name, uuid, реальный XML path и диапазон
байтов, owner_id корня и parent_key непосредственного inline-родителя. Ключ
`path#uuid` не выдаётся за путь к отдельному файлу. Диапазоны учитывают исходный BOM.

URLTemplate имеет ровно один непустой ChildObjects только с Method. Operation
имеет ровно один ChildObjects только с Parameter; пустой контейнер допустим.
Method, Parameter и IntegrationServiceChannel — листья, любой ChildObjects у
них отвергается. Неверная роль, неизвестный тег/структурная оболочка, namespace,
uuid/Name, дубли или неподдержанная глубина вызывают отказ.

InternalInfo допускается только непосредственно у IntegrationService и
IntegrationServiceChannel. Он отмечается unverified; его содержимое не получает
семантической интерпретации. Properties помимо Name также остаются unverified.
Нет распознавания обработчиков, типов/направлений параметров, URL/HTTP-глаголов,
транспортных настроек или связи XML Method/Operation с BSL-методом.

Основание грамматики — переданные обезличенные агрегаты закреплённой БСП:
4 HTTPService / 24 URLTemplate / 27 Method; 13 WebService / 143 Operation /
392 Parameter, 14 пустых ChildObjects операций; 1 IntegrationService / 4 канала.
Все 18 корней имеют модуль. Это сведения локальной проверки в постановке задачи,
а не выполненный здесь прогон реальных данных.

## Совместимость и навигация

Схема journal1 с новым CHECK профиля и расширением inline kind CHECK на URLTemplate,
Method, Operation, Parameter, IntegrationServiceChannel. Новых таблиц нет.
Исходный schema.sql и DDL предыдущих профилей неизменны. Канонический формат фактов
journal1 сохранён: новые значения kind и parent_key уже входят в хеш. Новый профиль
и guard различают snapshot_id. Reader/verify распознают только точную новую схему.

Модуль сервиса принадлежит объекту-корню. Он не привязывается к inline-узлу по
совпадению имени. При отказе BSL-разбора сохраняется unparsed, причина и bytes,
а методы не выдумываются. Inline Method — структурное XML-объявление, не результат
BSL-парсера; его children=NULL. URLTemplate/Operation.children ведёт к одной
непосредственной ветке inline. Empty Operation даёт пустой полный список детей.

Корень → вид → объект → inline-группа → URLTemplate/Operation → листья.
Source каждого объявления возвращает точные bytes XML; branch.source объекта —
реальный дескриптор; companions содержит диапазон ссылки в Configuration.xml.
BSL читается отдельно через модуль/метод и физические строки, UTF-8 или Base64
без lossy. `service_structure=complete_known_grammar`, `service_semantics=not_analyzed`;
relations также not_analyzed. Это не полнота всей БСП или семантики сервисов.

Точный lookup ADR-021 сохранён для объектов и BSL-методов. Имена методов R09 видимы
в нижнем регистре, исходник не меняется. Фильтр не распространяется на inline-ветки:
их прежние лимиты строк/байтов действуют, слишком большая ветка отвергается целиком.

## Безопасные границы и тестирование

Build использует ограниченный пофайловый ingest и producer из сохранённых sources.
Open не разбирает BSL; offline verify тем же producer пересчитывает XML-структуру,
владельцев, методы, индексы и OID/SHA. Git не нужен и происхождение из Git не
подтверждается. Формально целая подмена parent_key с пересчитанными хешами проходит
open, но отвергается verify. Любой поздний отказ откатывает факты, удаляет собственную
временную БД и не публикует снимок. Неизвестная структура не превращается в успех
с пропущенными данными. Сохранены file/total/count/Git/facts/coverage/SQLite/time,
private staging, лимиты reader и полного JSON-RPC 65536 bytes. Reader предназначен
для доверенных артефактов с независимым expected SHA, не произвольного SQLite.

```sh
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
cargo +1.85.1 test --locked --features experimental-snapshot-mcp --test mcp_snapshot services_cli
cargo +1.85.1 test --locked --features experimental-snapshot-mcp --test mcp_snapshot exact_name_cli
```

Запускать последовательно при общем target. Сервисный тест вызывает реальный CLI,
удаляет только свой синтетический Git, выполняет offline verify/open, затем stdio
initialize/metadata/tree/source при пустом PATH. Проверены владельцы, parent_key,
byte-exact XML/BOM и BSL/CRLF/last-no-LF, пустая Operation, unparsed, lookup, реальная
ссылка в Configuration, deterministic reuse и parity прежних фактов. Отрицательные
случаи: каждый обязательный модуль, лишний файл inline-узла, пустой/отсутствующий/
двойной ChildObjects, неверные роли, глубина, namespace, qualified/invalid uuid,
нескалярный Name, недопустимый InternalInfo/оболочка и сирота. Output остаётся пустым.
Отдельный прежний тест lookup проверяет 804 CommonModule и 261 метод.

[Перенос и команды](nucbox-service-diagnostic.md). Реальные данные не использовались,
NucBox не запускался. Полная приёмка pinned БСП и оставшиеся неподдержанные виды/
структуры остаются отдельной работой. GitHub/main не меняются.
