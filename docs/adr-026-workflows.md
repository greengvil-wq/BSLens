# ADR-026: ExchangePlan, BusinessProcess и Task

Основа `dd9f759e4dfa337cb9c2739418dcbf41c0346780`. Новый opt-in `build-workflows`,
профиль `linux-v5-workflow1-xml221-r09`, Linux x86_64. Старые профили не расширены.
`workflow_guard=exchange-process-task-v1` и нормализованные бюджеты входят в identity.

## Структура и файлы

| Корень → каталог | Inline | Scalar-ссылки |
|---|---|---|
| ExchangePlan → ExchangePlans | Attribute, TabularSection → Attribute | Form, Template |
| BusinessProcess → BusinessProcesses | Attribute, TabularSection → Attribute | Form |
| Task → Tasks | AddressingAttribute, Attribute, Command | Form |

Дескриптор — `<каталог>/<Name>.xml`, MDClasses/MetaDataObject 2.21, прямой элемент
правильного типа с unqualified UUID и scalar Properties/Name. Пустой ChildObjects
корня допустим. TabularSection требует один непустой ChildObjects только с Attribute;
листья не имеют вложений. InternalInfo корня/TabularSection и свойства помимо Name
отмечаются unverified. Неизвестные структурные оболочки, роли, теги, UUID/Name,
namespace, дубли и глубина вызывают отказ. AddressingAttribute не выдаётся за
расшифрованное правило адресации задачи.

Для всех трёх видов обязательны точные `Ext/ManagerModule.bsl` и
`Ext/ObjectModule.bsl`. Каждый Command задачи требует
`Commands/<Name>/Ext/CommandModule.bsl`. modules.owner_id указывает контейнер Task,
inline_module_owners связывает модуль с конкретным Command; связь не угадывается
по имени BSL-метода. Другие дорожки и модули-сироты отвергаются. Unparsed сохраняет
причину и bytes, методы при этом не создаются.

У ExchangePlan обязателен `Ext/Content.xml`, у BusinessProcess —
`Ext/Flowchart.xml`; у Task аналогичных payload нет. Эти XML непрозрачны,
не проверяются как MDClasses и не интерпретируются как состав обмена/маршрут.
Справка корня/формы допускается полной парой `Ext/Help.xml` + `Ext/Help/ru.html`
либо отсутствует целиком. Неполная пара — отказ.

Form ref разрешается в `Forms/<Name>.xml` и обязательный
`Forms/<Name>/Ext/Form.xml`. `Ext/Form/Module.bsl` формы необязателен только
для ExchangePlan; для BusinessProcess и Task он обязателен. Дополнительные
ресурсы форм в данном срезе не подтверждены и отвергаются.

Template допускается только у ExchangePlan: `Templates/<Name>.xml` плюс
`Templates/<Name>/Ext/Template.txt`. XML/BIN/HTML и дополнительные дорожки макета
этот новый вид не принимает. Подтверждённые TXT остаются непрозрачными bytes.
Предыдущие профили и их правила макетов не изменены.

У каждого inline-объявления сохраняются реальный XML path, UUID, owner_id,
parent_key непосредственной табличной части и диапазон bytes с учётом BOM.
В companions находятся диапазоны реальных scalar-ссылок Configuration/Form/Template.
Payload принадлежит корню/форме/макету и выдаётся Base64 без lossy. BSL сохраняет
BOM, CRLF, последнюю строку без LF. Никакой обмен, маршрут или код не исполняется.

Основание — переданные обезличенные агрегаты закреплённой БСП: шесть ExchangePlan
(17 Attribute, четыре TabularSection/пять Attribute, шесть Form, два Template,
один пустой ChildObjects), два BusinessProcess (48 Attribute, одна TabularSection/
пять Attribute, шесть Form), один Task (четыре AddressingAttribute, 14 Attribute,
четыре Command, восемь Form). Все имеют Manager/Object; планы — Content.xml,
процессы — Flowchart.xml. BSL форм планов есть у 5/6, процессов у 6/6, задач у 8/8.
Это сведения постановки, не выполненный здесь прогон реальной БСП. Количества
не являются универсальными ограничениями схемы.

## Совместимость и безопасность границ

Схема наследует report1: новый точный профиль и одно дополнительное значение
AddressingAttribute в CHECK inline kind. Новых таблиц/зависимостей нет;
schema.sql и предыдущие схемы неизменны. Канонический формат journal1 уже включает
kind/parent_key, его реализация не дублируется. Snapshot identity отделена профилем
и guard. Reader/verify используют единый пофайловый producer из сохранённых sources.

Open не разбирает BSL повторно. Offline verify без Git сверяет OID/SHA, XML-факты,
родителей/владельцев, методы и строки; Git provenance этим не подтверждается.
Неверный владелец Content.xml с пересчитанными хешами может пройти open, но
должен быть отвергнут verify. Поздний отказ откатывает факты, удаляет свою
временную БД и оставляет output без опубликованного снимка.

MCP показывает структуру, `workflow_structure=complete_known_grammar`,
`workflow_semantics=not_analyzed`; relations и companion_contents not_analyzed.
Сохраняются прежний exact lookup и Recalculation в смешанном снимке, inline-лимиты,
бюджеты file/total/count/Git/facts/coverage/SQLite/time, owner-only staging, reader
и полный JSON-RPC 65536 bytes. Reader рассчитан на доверенные артефакты с
независимым expected SHA, не гарантирует безопасность произвольного SQLite.

## Синтетические проверки

```sh
cargo +1.85.1 test --locked --all-features
cargo +1.85.1 test --locked --features experimental-router
cargo +1.85.1 test --locked
cargo +1.85.1 fmt --check
git diff --check
cargo +1.85.1 test --locked --features experimental-snapshot-mcp --test mcp_snapshot workflows_cli
```

Реальный CLI строит смешанный снимок; только созданный тестом Git удаляется.
Offline verify/open и stdio initialize/metadata/tree/source выполняются с пустым
PATH. Проверяются точные методы/исходники/XML/payload, пустой план, форма без кода,
четыре команды Task (включая unparsed), AddressingAttribute, непосредственный
родитель реквизита табличной части, владельцы модулей/команд, TXT и справка.
Регрессия Recalculation в новом профиле проверяет видимость, parent_id, владельца
Dimension и точные XML-байты. Наследуется полный тестовый набор lookup/лимитов.

32 отрицательных варианта проверяют обязательные модули/файлы, команды-сироты,
формы, неполную справку, неизвестные дорожки/виды/глубину, UUID/Name и orphan root;
output остаётся пустым. Проверяются deterministic reuse, parity прежних фактов,
отказ старого профиля от новых видов и offline обнаружение неверного владельца.

[Перенос и команды](nucbox-workflow-diagnostic.md). Source-only архив не содержит
БСП/ERP, бинарников или секретов. NucBox и реальные данные не затрагивались.
Полная БСП не принята: реальная локальная приёмка и оставшиеся неподдержанные
структуры требуют отдельной работы.
