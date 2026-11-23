# Проверки R03

Только cloud Linux и авторские временные Git-репозитории. Windows, БСП и GitHub
не использовались. Тестовые Git write-команды создают fixtures; библиотечный API
исходный репозиторий не меняет. Новых Cargo-зависимостей нет.

Проверки:

- два точных commit SHA, грязная рабочая копия и untracked файл не меняют результат;
- отсортированные адреса, точные blob/tree OID, повторяемый manifest;
- Git SHA256 object format и пустой blob;
- сломанные исходные config/HEAD не влияют на fixed isolated Git-команду;
- revision expressions, короткий/uppercase SHA, неизвестный объект отвергаются;
- лимиты числа файлов/объектов и размера stdout/объектов/общего объёма;
- timeout дочернего процесса и остановка бесконечного вывода;
- symlinks, alternates, promisor markers, corrupted compressed object;
- корректно распакованный, но подменённый blob не проходит проверку Git hash;
- blob вместо commit, symlink/gitlink, плохие/дублированные tree paths,
  malformed tree и отсутствие границы commit headers;
- exact Git bytes → typed XML subset, без открытия production gate.

Проверки read-only включают побайтовую неизменность всех файлов репозитория (включая
.git), неизменность `git status --porcelain` при грязной рабочей копии и удаление
private временного каркаса после ошибки.
Это не adversarial concurrency audit и не доказательство отсутствия гонок файловой
системы. Стабильность object-store во время чтения — ограничение текущего этапа.

Исполнено на изолированном Rust 1.85.1 и системном Git 2.47.3 Linux:

- `cargo test --locked --features experimental-git`: 11 успешно
  (2 внутренних проверки процессов, 8 Git integration, 1 production gate).
- `cargo test --locked`: 1 успешно, экспериментальные слои выключены.
- `cargo test --locked --all-features`: 26 успешно, включая маршрут Git → XML.
- `cargo fmt -- --check`, `git diff --check`: успешно.

Предупреждений компилятора в итоговом прогоне нет. Независимое ревью требуется
отдельно; измерения скорости на реальных конфигурациях не проводились.
