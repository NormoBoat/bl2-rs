# 15 — Передача работы (GPT/Codex, ChatGPT, Claude)

Документ самодостаточен: продолжающий агент не имеет истории прошлых диалогов. Формулировки «как обсуждали ранее» запрещены — только точные ссылки на файлы/разделы/ID.

## 1. Начало каждой сессии (строго по порядку)

1. Прочитать Ponytail: `.agents/skills/ponytail/SKILL.md` (Codex) или `.claude/skills/ponytail/SKILL.md` (Claude) — уровень **full**. Ограничения: `14-decisions.md` D-001 (Ponytail не сокращает объём, не упрощает оригинальное поведение, не отменяет проверки, три ОС и совместимость обязательны).
2. Прочитать `plan/README.md`.
3. Прочитать `plan/status.md`.
4. Прочитать этот файл.
5. Проверить REA (§3).
6. Выбрать задачу (§2).

## 2. Выбор задачи

- В `12-tasks.md` — первая по порядку задача `TODO`, у которой все зависимости `DONE`, нет блокера.
- Если такой нет — продолжить `IN_PROGRESS` из `status.md`; если и её нет — оформить запросы на снятие блокеров (`13-risks.md` §1) и выполнить исследовательские подзадачи, не требующие заблокированных данных.
- Перед началом: поставить задаче `IN_PROGRESS` и записать в `status.md`.

## 3. REA в каждой сессии

1. Проверить доступность: MCP-сервер `rea` (Claude: `.mcp.json`; Codex: `.codex/config.toml`, см. T-005) **или** CLI: `. .\tools\research\rea-env.ps1; rea --version; rea doctor --provider ghidra --json`. Версия должна быть 6.7.0 (или новее только по решению в `14-decisions.md`).
2. Использовать закреплённые версии и инструкции `02-rea-workflow.md`.
3. Применять REA по необходимости для: уточнения поведения оригинала; проверки гипотез и результатов прошлых исследований; анализа функций, форматов данных и сетевого поведения; поиска причин расхождений между оригиналом и Rust-реализацией.
4. Не повторять подтверждённое без причины: сначала искать в `03-evidence.md`.
5. Не выдумывать результаты вызовов REA. Нет вызова — нет доказательства.
6. Результаты — в `03-evidence.md` (версия инструмента, сборка/хеш, команда или MCP-вызов, выводы, ограничения) + сырой вывод в `plan/evidence/raw/` (или `.research/` для производных от кода оригинала).

## 4. Если REA недоступен

1. Явно записать ограничение в `status.md` («Состояние инструментов», дата, что пробовали, ошибка).
2. Продолжать задачи, для которых достаточно уже сохранённых доказательств.
3. Для задач, требующих нового исследования, — подготовить точное описание проверки: вопрос, входные файлы (путь + SHA-256), команды REA, ожидаемый вид результата — в разделе задачи «Ожидает REA».
4. Передать такие проверки агенту/среде с REA (указать в `status.md`).
5. Зависимые задачи пометить `BLOCKED` с причиной «нужно исследование REA: <что>», если без него нельзя достоверно продолжить.

## 5. После задачи (обязательно)

- Обновить в `12-tasks.md`: статус, доказательства (E-номера), выходные файлы.
- `03-evidence.md`: новые факты, расхождения (DIVERGENCE-xxx).
- `14-decisions.md`: принятые решения.
- `status.md`: что сделано, текущее состояние, следующий шаг, изменения блокеров.
- **Опровергнутая гипотеза**: найти все места плана, которые на ней основаны (`Select-String -Path plan\*.md -Pattern '<E-номер или ключевое слово>'`), исправить, записать причину; не продолжать реализацию по опровергнутому предположению. Связанные DONE-задачи, чьи критерии зависели от гипотезы, вернуть в `IN_PROGRESS` с пояснением.

## 6. Правила реализации

- Ponytail full (D-001): минимальный полный код, без абстракций «на будущее», тест для нетривиальной логики, комментарий-ссылка на источник поведения `// orig: …`.
- Не исполнять код оригинала; оригинальные ресурсы — только из установки пользователя (`BL2_INSTALL`); производные — не коммитить.
- Новая зависимость — только после проверки `17-dependencies.md` §1 и записи там.
- Каждая платформенная особенность — в одном месте, с записью в `16-platforms.md`.
- Не ставить DONE без доказательства выполнения критерия задачи.

## 7. Полезные факты для быстрого старта

- Установка: `D:\Projects\bl2-rs\Borderlands 2` (Windows; данные и тесты). Exe SHA-256 `1231B384AEA791BC51286664627D03E27D6A77FFD4EC3BB5001186C136504C17`.
- Запуск оригинала: Steam-установка `D:\SteamLibrary\steamapps\common\Borderlands 2`, buildid 9218157, побайтно равна копии (E-049, D-015).
- Данные пользователя оригинала: `C:\Users\normo\OneDrive\Documents\My Games\Borderlands 2` (лог, конфиги, сохранения); копии — `.research/user-data/2026-10-21/`. Работать только с копиями.
- Исследовательские скрипты: `tools/research/lzo1x.mjs` (декодер), `lzo-chunk-check.mjs` (пакеты), `save-envelope-check.mjs` (сохранения), `upk-header-survey.mjs`.
- Снапшот Ghidra: `.research/rea/Borderlands2.exe.snapshot.json` (если отсутствует — T-003).
- Имена нативов: `plan/evidence/raw/exe-native-exec-names.txt`.
- Заголовки пакетов: `plan/evidence/raw/upk-header-survey.json` (скрипт `tools/research/upk-header-survey.mjs`).
- Манифест установки: `plan/evidence/raw/install-manifest.sha256.csv`.

## 8. Передача после проверки Codex 2026-10-21

- Не начинать T-015 заново: корпус уже разобран (E-026/E-060/E-061), повторный скан — `evidence/raw/T-015-codex-recheck.txt`.
- Доступ Codex к проектному REA 6.7.0 проверен прямым `binary_session`, E-021. Глобальную регистрацию не менять: она не определяет сервер этой проектной сессии. Старая очередь `.research/rea/queue` хранит результаты, а не доказательство живого процесса.
- T-011: сначала E-023 и сохранённый `batch01.out`. SSE в математических обёртках не устанавливает MXCSR/x87 для движения/урона. Нужны `_controlfp_s`/настройки округления и тела функций за exec-обёртками. D-007 пока PROPOSED.
- T-032a: скаляры — `Package::read_scalar_tag`, метаданные — `schema::Schema` (E-062/E-063). `NameRef`/byte ranges схемы относятся к исходному Package; индексы ссылок — к пакету конкретной схемы. Ссылки `ObjectReference::Unresolved` никогда не использовать как import/export index; их наличие не считать подтверждённым разрешением объектов.
- Запуск проверок: `$env:BL2_INSTALL='D:\Projects\bl2-rs\Borderlands 2'`; `cargo test -p bl2-formats --release --test property_scan --test script_scan --test schema_scan -- --ignored --nocapture`. Unit tests и clippy: `cargo test --workspace`; `cargo clippy --workspace --all-targets`. Полные локальные логи — `.research/T-032a-scalar-scan.log`, `.research/T-015-recheck.log`; сводки — `plan/evidence/raw/T-032a-*`.
- T-032a DONE 2026-10-22 (E-064/E-065, D-017): значения — `values::Decoder::read_tag(package, owner_path, tag, class_default)`, CDO — `defaults::Defaults`. Скан значений: `cargo test -p bl2-formats --release --test value_scan -- --ignored --nocapture class_defaults every_composite_value` (≈3 мин).
- На старте существовал untracked `tools/research/rea-show.mjs`; он сохранён без изменений. Исследовательские результаты/ассеты не добавлять в git.

## 9. Передача после сессии Claude Code 2026-10-22

- Сделано и проверено: T-032a, T-033, T-019, T-020, T-021, T-011 (DONE); T-012, T-013 — IN_PROGRESS с точными следующими шагами в `status.md`. Доказательства — E-023…E-025, E-030…E-032, E-064…E-066; решения D-007, D-008 ACCEPTED, D-017.
- Сканы установки (все `#[ignore]`, нужен `BL2_INSTALL`): `value_scan`, `property_scan`, `script_scan`, `schema_scan`, `install_check`, `shader_cache_scan`, `wwise_scan`, `gfx_scan`. Исследовательские тесты в `value_scan.rs` (`struct_layouts`, `dump_tag`, `schema_fields`, `function_natives`, `enum_values` …) управляются переменными окружения, описанными в атрибуте `ignore`.
- REA через очередь: `node tools/research/rea-session.mjs --serve .research/rea/queue` (фон), пакеты `<имя>.json` → `<имя>.out/`; чтение — `tools/research/rea-show.mjs <out> <regex>`; группировка по процедурам — `rea-procs.mjs`; классы FP-инструкций — `rea-fpclass.mjs`; граф вызовов — `rea-bfs.mjs` (вперёд) и `rea-callers.mjs` (назад); RNG-достижимость — `rng-reach.mjs`. `find_xrefs_to_name` не находит импорты — брать адрес ячейки IAT (скрипт в истории: разбор импорта PE, как `pe-imports.mjs`) и `xrefs` по нему. Ожидать завершения пакета: в bash `until [ -e <q>/<имя>.done ]; do sleep 2; done`.
- Приёмы, давшие результат: виртуальные функции движка — по широким строкам `PURE_VIRTUAL` (`UEngine::Tick` и т.п.) → заглушка → слот vtable → тот же слот у наследников; фазы `UGameEngine::Tick` — по записям байта метки `GBX_MEM_*` в стек меток.

## 10. Передача после сессии Claude Code 2026-10-23

- T-037 DONE (E-067), T-038 IN_PROGRESS (E-068). Код: `crates/bl2-formats/src/{level,model,material}.rs`, `package.rs::{level_guids, CrossLevelGuids::import_guid}`; сканы `tests/level_scan.rs` (14 тестов + помощники `dump_native_tail`, `tag_list`, `smallest_tail` с переменными окружения из атрибута `ignore`), `tests/shader_cache_scan.rs::materials_link_to_shader_maps`.
- Приём, давший результат для нативных форматов: `search_strings` имени класса → `xrefs` на широкую строку (`L"Terrain"`, у подклассов — суффикс строки `UTerrainComponent` со смещением +2) → функция регистрации `FUN_00475f50(…, L"<Class>", …, InternalConstructor, …)` → конструктор (`*this = vtable`) → **слот 13 vtable = `Serialize`** (проверено на ATerrain, AGBXNavMesh, UTerrainComponent). Имена полей Gearbox — из скриптовых структур (`value_scan::schema_fields`, `PREFIX=Engine.<Class>`).
- Псевдокод Ghidra иногда переворачивает условия версий (`LicenseeVer`): сверять с `read_function_instructions` (пример: 0x5cd4a0, E-067). Версии архива BL2: `ArVer` 832, `LicenseeVer` 46 (`param_1[1]`, `param_1[3]` в сериализаторах).
- Shell: в `node -e "…"` внутри bash обратные кавычки исполняются — doc-комментарии с `…` писать через Write/Edit или файл-скрипт, не через `node -e`.

## 11. Передача после второй сессии Claude Code 2026-10-23

- Новые модули: `vorbis.rs` (Wwise Vorbis, `CodebookLibrary::from_exe` — VA таблицы зафиксирован для проверенной сборки), `wwise::{cue_labels, facefx_animation, ConvolutionIr}`. Сканы: `tests/vorbis_scan.rs` (сверка с vgmstream: `VORBIS_SAMPLE_DIR=.research/T-020a-samples`, `write_samples` → `.tools/vgmstream-r2117/vgmstream-cli.exe -i -o NN.wav NN.wem` → `compare_samples`).
- Приём для RTTI-классов (Wwise и прочие C++ без UClass): строка `.?AV<Class>@@` → TypeDescriptor = адрес − 8 → `search_bytes` его адреса → CompleteObjectLocator (ссылка − 12) → `search_bytes` адреса COL → vtable = найденное + 4.
- Приём для pure virtual: строка `<Class>::<Method>` у заглушки → `search_bytes` адреса заглушки → слот vtable базового класса → тот же слот в vtable наследников (vtable — из конструктора, переданного в регистрацию класса).
- Пока тест выполняется, его exe заблокирован линковщиком Windows: параллельные сборки — с `CARGO_TARGET_DIR=target/aux`.
- В bash не использовать `cat > файл` без heredoc (ждёт stdin) и `node -e` с обратными кавычками; скрипты правок — файлами через Write.

## 12. Запуск и запись оригинала (2026-10-24)

- Агент управляет игрой через computer use (Claude desktop; доступ к Steam, Launcher, Borderlands2 выдаёт пользователь). Перед запуском: хеш exe и buildid (D-015), копия `SaveData`/`Config` в `.research/user-data/<дата>/`.
- Запуск только так: `node tools/research/frida-record.mjs <out.jsonl> "D:/SteamLibrary/steamapps/common/Borderlands 2/Binaries/Win32/Borderlands2.exe" -windowed -ResX=1280 -ResY=720` (Steam запущен пользователем). Без Frida игра падает при старте (E-028); лаунчер 2K аргументы не передаёт.
- Окно игры: 1280×720 по центру экрана 2912×1638 (снимки computer use в масштабе 1456×819: заголовок окна ≈ (600, 214), область игры ≈ (366, 206)–(1094, 632)). Меню — клавиатурой (стрелки, Enter, Esc); мышь — только строками `m dx dy` в `<out.jsonl>.ctl`.
- Выход: Esc → вниз ×5 → Enter → вверх → Enter (Save and Quit) → ждать 25 с → вниз ×9 → Enter → вверх → Enter.
- `computer_batch` ограничен 300 с: длинные ожидания — в отдельных вызовах; между вызовами задержка до ~2 с (входит в измеренный разброс, E-028 (7)).
- Каждый New Game создаёт новое сохранение `SaveNNNN.sav` (так появились Save0007…0009, персонажи Axton); сохранения пользователя 0001…0006 не трогать (Continue загружает последнего персонажа).

## 13. Два экземпляра и LAN (2026-10-24, T-018)

- Запуск: два `frida-record.mjs` (A — Steam-установка, B — копия проекта), `-WinX` не действует — окна двигать `SetWindowPos` (PowerShell `Add-Type`), фокус — `SetForegroundWindow(MainWindowHandle)`: клик computer use по второму окну фокус не переносит, клавиши уходят в прежнее окно.
- Порядок, который сработал: B — Network Options → Network Mode → LAN → Continue (хост); затем A — LAN → FIND GAMES → Enter → PLAY → Enter. При обратном порядке первый хост A завершился без отчёта (E-029 (а)).
- Захват: `tshark -i '\Device\NPF_Loopback' -i '\Device\NPF_{C7989E22-F269-4F62-B518-304F35C11B70}' -f "udp and not port 53 and not port 443 and not port 5353 and not port 1900" -w .research/pcap/<имя>.pcapng`; игра — UDP 7777 на loopback, обнаружение — UDP 14001 broadcast.
- Завершать игры через меню (Save and Quit → Quit), иначе `Launch.log` обрезан на 4 КБ.

## 14. FaceFX (2026-10-24, T-021b)

- Формат архива — `bl2-formats::facefx` (E-072), сканы `vorbis_scan::facefx_animations` и `level_scan::facefx_archives` (BL2_INSTALL; при ошибке архив пишется в FACEFX_OUT).
- Часы воспроизведения и вычисление графа — `bl2-sim::facefx` (`Playback`, `curve_value`, `link`, `Graph`); `bl2-sim` теперь зависит от `bl2-formats` (разрешено 04-architecture).
- Сверка с оригиналом: `node tools/research/frida-facefx.mjs <pid> <out.jsonl>` к игре, запущенной `frida-record.mjs` (pid в его выводе). Нужна сцена с NPC, у которого есть FaceFX (Liar's Berg); в прологе вызовов нет.
- REA-очередь `.research/rea/claude-1026` (адреса FaceFX — в E-072).

## 15. Анимации и код без функций Ghidra (2026-10-25, T-036)

- Цели vtable, на которые нет прямых вызовов, Ghidra не размечает функциями: `procedure_pseudo_code` по такому адресу молча берёт адрес как file offset и декомпилирует **другую** функцию — проверять имя `FUN_<адрес>` в ответе. Листинг таких мест — `node tools/research/rea-disasm.mjs <queue> 0x<адрес> > out.asm` (несколько процессов параллельно можно: имя пакета содержит pid; старая версия с Date.now() уронила сервер).
- Сверка декодеров с оригиналом: `frida-anim-keys.mjs` (распаковщики ключей) и `frida-anim-pose.mjs` (`GetPose*` целиком, поля последовательности из памяти) к игре от `frida-record.mjs`; `anim-pose-text.mjs` → тесты `anim_scan::{keys_match_original, poses_match_original}` (ANIM_KEYS / ANIM_POSES — абсолютные пути: тест запускается из каталога крейта).
- В прологе и меню встречаются только форматы 3/9/10; Delta48/Variable/PerTrack нужно искать в других сценах.
- Скелетные меши: `bl2-formats::skeletal` (SkeletalMesh, MorphTarget, GPU-вершина), сканы `anim_scan::{skeletal_meshes, gpu_vertices, morph_targets, anim_sets_match_tracks}`; декомпиляции и листинги — `.research/T-036/skel/` (REA-очередь `.research/rea/claude-1028`, сервер остановить файлом `STOP`).
- Правки плана со спецсимволами (обратные кавычки) — только через Write/Edit или .mjs-файл, не через `node -e "…"` в bash: обратные кавычки там выполняются как подстановка команд.
- Позы скелета: `node tools/research/frida-skel-pose.mjs <pid> <out.jsonl> [N]` (выгрузка внутри `UGameEngine::Tick`), строки `K` для теста `bl2-sim::anim::original::poses_match_original` (SKEL_POSES) — преобразование как в истории сессии 2026-10-25 (node: `['K', mesh, parents.join(','), local, space.slice(0, space_count*64)].join(' ')`).
- FaceFX: `tools/research/facefx-ticks-text.mjs` → `bl2-sim::facefx::original::ticks_match_original` (FACEFX_TICKS).
- Пользователь разрешил использовать любые сохранения игры (2026-10-25); копия перед этим — `.research/user-data/2026-10-25-before-saves-use`. Персонаж FirstKaktus (ур. 41, TVHM) открывает Sanctuary (NPC с FaceFX).
- Узлы анимации: `node tools/research/frida-anim-nodes.mjs <pid> <out.jsonl>` → строки `N` (node: `['N', mesh, nodes_on_component, rotation_only, time, length, frames, ...formats, offsets||'-', stream||'-', tracks.join(',')||'-', bones.join(','), local].join(' ')`) → `anim_scan::sequences_match_local_atoms` (ANIM_NODES; ANIM_NODES_SHIFT — сдвиг времени, ANIM_NODES_DEBUG=<меш> — печать расхождений).
- Convolution Reverb: `bl2-audio::convolution` (KissFFT-порядок, E-077); проверка на IR установки — `cargo test -p bl2-audio --release --test ir_scan -- --ignored --nocapture` (BL2_INSTALL); параметры Fx — `vorbis_scan::fx_objects`. Для меток vtable, а не функций, `procedure_pseudo_code` декомпилирует чужую функцию — сверять `FUN_<адрес>` в результате.
