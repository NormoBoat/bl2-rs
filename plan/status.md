# Статус проекта

Обновлено: 2026-10-25 (Claude Code): T-020b — параметры Convolution Reverb и ядро свёртки (E-077, крейт `bl2-audio`); T-036 — анимации (E-073) и скелетные меши/морфы/AnimSet (E-074); T-034 — Mancana Remaster (E-075); T-021b — FaceFX (E-072); T-018 IN_PROGRESS (E-029).

## Этап

**M1/M2 — исследование и формат-слой.** В workspace реализован `bl2-formats`; игрового runtime/Bevy ECS ещё нет. Завершены T-010/T-011/T-015/T-019/T-020/T-021/T-030/T-031/T-032/T-032a/T-032b/T-033/T-035. Есть крейт `bl2-sim` (пока без Bevy: `rng.rs`, `fp.rs`). Сейчас: T-012 (потребители RNG через виртуальные вызовы), T-013 (содержимое групп тика), T-038 (MIC без карты шейдеров); T-037 DONE 2026-10-23.

## Ответы пользователя (2026-10-21) и что изменилось

| Пункт | Ответ | Изменения в плане |
|---|---|---|
| Запуск оригинала (M-1) | Игра запускается через Steam | B-01 снят; установка `D:\SteamLibrary\…` (buildid 9218157) побайтно равна копии проекта (E-045, E-049); `Launch.log` разобран (E-046); D-015 |
| Сохранения (M-2) | Появились | Скопированы в `.research/user-data/2026-10-21/`; оболочка формата подтверждена (E-047); T-016 → IN_PROGRESS. Важно: большинство сохранений записаны старыми версиями игры |
| Steam (M-5, D-010) | DLL Valve — единственное исключение, опционально: есть — игра через Steam, нет — только LAN | D-010 ACCEPTED; T-095 разблокирована (после T-092…T-094); добавлена проверка N-6 «без Steam». Решение без `steam_api64`: свой Rust-код вместо `steam_api` загружает установленный `steamclient64`; проба получила `SteamNetworking005` для AppID 49520 (E-052…E-055) |
| Wireshark/Npcap (M-10) | Установить | Wireshark 4.6.9 + Npcap 1.89 (loopback) установлены, PATH обновлён, REA видит TShark (E-050) |
| Второй экземпляр (M-3) | «Делай что нужно в пределах рабочей директории» | D-016: два экземпляра на этой машине (Steam + копия проекта); осуществимость — E-UNK-14, T-018 шаг 0 |
| Linux/macOS (M-4) | Не трогать до готовности Windows | D-014; M7 BLOCKED до T-110W; добавлена T-110W; требование трёх ОС не ослаблено |
| Git (M-9) | Установлен | T-004 выполнен: репозиторий создан, первый коммит |
| Посторонние файлы (M-7) | Удалены | Подтверждено (E-011) |

Новое из запуска оригинала: middleware **FaceFX 1.7.4** (E-046) → S-09 и задача T-021a; «Документы» перенаправлены в OneDrive → `16-platforms.md` §3.

## Сделано ранее (сессия 1)

Инвентаризация и манифест; целевая сборка (exe SHA-256 `1231B384…4C17`); Ponytail (T-001) и REA 6.7.0 (T-002) подключены; T-003 (REA/Ghidra обзор exe: 109 710 процедур, импорт ≈27 мин, эфемерный — работать в одной MCP-сессии); LZO1X пакетов (E-040); документы 00–17, AGENTS.md, CLAUDE.md.

## Состояние инструментов (2026-10-21)

| Инструмент | Состояние |
|---|---|
| REA CLI 6.7.0 | Доступен (`tools/research/rea-env.ps1`) |
| REA MCP-сервер | `rea mcp doctor` healthy, 207 инструментов (E-012) |
| REA MCP в Claude Code / Codex | Codex: прямой MCP 6.7.0 из проекта подтверждён (T-005, E-021). Claude: сохранённые CLI/MCP-вызовы `rea-session.mjs` (E-022); прямой UI-клиент отдельно не проверялся |
| Ghidra 12.1.4 / JDK 21 | Работают |
| git 2.56.0 | Установлен (`C:\Program Files\Git\cmd\git.exe`) |
| Wireshark/TShark 4.6.9, Npcap 1.89 | Установлены; REA PCAP-провайдер работает (E-050) |
| Frida, apitrace/PIX | Не установлены (T-017/T-080) |

## Открытые блокеры и неизвестные

- ~~E-UNK-14~~ — закрыто E-029: два экземпляра на одной машине образуют LAN-сессию (B-хост, A-клиент).
- **R-16** — совместимость P2P-трафика нового клиента с оригиналом через Steam (проверка в T-095; LAN не затронут).

## Сделано в сессии 3 (2026-10-21)

- Steam: Rust-реализаций `steam_api` нет (E-052); найдено решение — своя Rust-замена тонкого загрузчика `steam_api` поверх установленного `steamclient64`; проба работает (E-053…E-055); D-010 уточнён.
- **T-030, T-031 DONE**: workspace + `crates/bl2-formats` (LZO1X, заголовок, чанки, таблицы имён/импортов/экспортов), 2 008/2 008 пакетов без ошибок (E-034).

- **T-032 DONE**: заголовок объекта + тегированные свойства читаются у всех 4 654 550 не-скриптовых экспортов (исправленный счётчик E-026), 0 ошибок; правила `RF_HasStack`/шаблонов компонентов/доминантных светов выведены из данных; 54 класса с нативным хвостом — список работ для T-034…T-038 (E-035). Добавлены T-032a (схема классов/типизированные значения) и T-032b (ini).

- **T-032b DONE**: слияние ini совпадает с файлами, записанными оригиналом (E-057).
- **T-010 DONE**: карта нативов 6 891/6 891, структура таблиц и 10/10 выборочных функций подтверждены REA (E-022); пакетный раннер REA `tools/research/rea-session.mjs` — первый запрос 27 мин, остальные мс.

- **T-034 IN_PROGRESS**: мипы всех 96 507 текстур разобраны, все 237 520 мипов из `.tfc` распакованы с точным размером (E-058).

- **T-035 DONE**: все 40 090 StaticMesh разобраны побайтно, геометрия согласована с `Bounds` (E-059).

## Проверка и продолжение Codex (2026-10-21)

- T-005 DONE: прямой MCP и Ponytail проверены, E-021. Настройки пользователя не менялись.
- T-015 уже DONE (E-026/E-060/E-061); повторный `script_scan` проходит, 20 384 тела bytecode. Старое предложение начать T-015 заново удалено.
- T-011 IN_PROGRESS: сохранённая выборка SSE-инструкций оформлена в E-023; неизвестны режим MXCSR/x87 и тела движения/урона. Новый холодный импорт Ghidra запущен через MCP для `list_names` (поиск `_controlfp_s`).
- T-032a IN_PROGRESS: `properties.rs` читает скаляры, `schema.rs` строит qualified схему; E-062/E-063. Полный скан: 15 360 176 скаляров, 0 ошибок payload; 6 011 ссылок остаются явно Unresolved. 7 702 146 массивов/структур/атрибутов ещё не декодированы. `Reader` проверяет переполнение позиции и терминатор FString.
- Проверки: `cargo test --workspace`, `cargo clippy --workspace --all-targets`, release `property_scan`, `script_scan`, `schema_scan` с `BL2_INSTALL` — PASS. Реальные данные/декомпиляция остаются в `.research/` и копии установки.

## Сессия 2026-10-22 (Claude Code)

- **T-032a DONE** (E-064, E-065, D-017): `values::Decoder` — все 23 062 322 тега, 0 ошибок, точное расходование payload; бинарные immutable-структуры (флаги 0x20/0x80, порядок `Children`/`Next`), transient сериализуется только у CDO, Bool/Byte/enum/Attribute-элементы, `ObjectProperty`-теги Class/Component. `defaults::Defaults` подключён (`lib.rs`): 3 339 классов, 37 785 эффективных тегов. Исправлен `schema::object_path` для forced export. 104 171 массив свойств без runtime-схемы — `Untyped` (D-017). Cross-level разрешение перенесено в T-037.
- Проверки: `cargo test --workspace` (9 unit), `cargo clippy --workspace --all-targets` — PASS; `value_scan` (class_defaults, every_composite_value) — PASS; повтор `property_scan` (4 751 329 экспортов, 23 062 322 тега, 0 ошибок), `script_scan`, `schema_scan` — PASS (`.research/T-032a-recheck.log`).
- **T-033 DONE** (E-066): `install.rs` — SHA-256 exe (своя реализация), размеры манифеста, регистронезависимый резолвер; полная проверка 13 925 файлов — 0 расхождений; карта загружается за 0.22–0.37 с → кеш не нужен.
- **T-019 DONE** (E-030, D-008 ACCEPTED): импорты — только D3D9; `shader_cache.rs` разбирает `GlobalShaderCache` и `RefShaderCache` до байта: 83 026 + 251 шейдер SM3, 2 Bink-шейдера без байткода; каждый материал с ресурсом связан с картой шейдеров.
- **T-020 DONE** (E-031): `wwise.rs` — 68 AKPK, 284 банка v62; 121 555 медиа Wwise Vorbis, 50 IR свёрточного ревербератора, 34 пустых; `AkEvent`/`AkBank` ↔ HIRC/банки по FNV-1. Декодер — новая задача T-020a.
- **T-021 DONE** (E-032): `swf.rs` (+ `miniz_oxide`, D: `17-dependencies.md`) — 500 роликов, только AS2, 27 тегов, 61 опкод; R-07 уточнён.
- **T-011 DONE** (E-023, D-007 ACCEPTED, R-18): игровая математика SSE single; MXCSR меняют только 3 функции с восстановлением; D3D9 с FPU_PRESERVE; формулы Round/FFloor/FCeil; `RSQRTSS` зависит от CPU.
- **T-012 IN_PROGRESS** (E-024): CRT `rand` MSVCR100 (проверен декомпиляцией DLL и эталоном MSVC), второй LCG ×196314165, сиды QPC / `-FIXEDSEED` = 0; формулы FRand/Rand/RotRand/VRand; 68 скриптовых и 455 нативных потребителей через прямые вызовы; код `bl2-sim/src/rng.rs`.
- **T-013 IN_PROGRESS** (E-025): `UGameEngine::Tick` 0x822c50 (через слот 81 vtable `UEngine`), `UWorld::Tick` 0x59fd00; фазы движка, обработка dt (clamp 0.0005…0.4), порядок 5 групп тика.
- Проверки сессии: `cargo test --workspace` (17 + 5 unit), `cargo clippy --workspace --all-targets` — PASS; ignored-сканы с `BL2_INSTALL`: `value_scan`, `property_scan`, `script_scan`, `schema_scan`, `install_check` (вкл. `full_verification`), `shader_cache_scan`, `wwise_scan`, `gfx_scan` — PASS. Хеш exe и buildid 9218157 перепроверены 2026-10-22.
- REA: импорт exe в Ghidra через `rea-session.mjs --serve` занял 26.8 мин; все пакеты — `.research/rea/queue/t02…t46`, граф вызовов нативов — `.research/rea/T-012-callgraph.json` (13 839 процедур). Новые утилиты: `tools/research/{pe-imports,rea-procs,rea-fpclass,rea-bfs,rea-callers,rng-reach}.mjs`.

## Шаги, записанные 2026-10-22 (T-012/T-013 — остаются в силе)

Текущая работа Codex 2026-10-22: T-015a выбрана как первая TODO с выполненными зависимостями по `12-tasks.md`/`15-handoff.md` §2. REA CLI 6.7.0/Ghidra 12.1.4 healthy; прямой MCP доступен, но без открытого бинарника. Запросы — отдельная очередь `.research/rea/codex-t015a`. На входе обнаружены изменения `lib.rs`, `03-evidence.md` и untracked `level.rs`, `model.rs`, `tests/level_scan.rs`, отсутствовавшие в предыдущей передаче; они сохранены. `cargo test --workspace` PASS (лог `.research/codex-2026-10-22-tests.log`), два предупреждения в `level_scan.rs`.

1. **T-013** (первая незавершённая по порядку с выполненными зависимостями): в `UWorld::Tick` 0x59fd00 (асм `.research/rea/queue/t43.out/asm.json`) определить функции между записями `World+0x15c` (физика 0x873e10/0x873e30/0xb41040, компоненты 0x591c00…, таймеры/латентные действия — искать вызов `UpdateTimers` внутри `AActor::Tick`, сеть до/после групп), порядок акторов в 0x59e080; найти `FEngineLoop::Tick` (вызывающий слот 81 vtable движка: `call [reg+0x144]`) и сглаживание кадра (`bSmoothFrameRate`, `MaxSmoothedFrameRate`). Затем снять HYPOTHESIS в `04-architecture.md` §5.
2. **T-012**: привязать 119 rand-процедур и 35 процедур второго LCG к классам через vtable: (а) `search_bytes` адреса процедуры в `.rdata` → слот vtable; (б) начало vtable (как для `UEngine`, E-025); (в) класс — по соседним слотам/наследникам и заглушкам `PURE_VIRTUAL` или конструкторам. Тест-векторы есть (MSVC `srand(1)`).
3. Далее по порядку: T-015a (таблица GNatives — в `.data` 0x19b9040… найдена непрерывная таблица адресов exec-функций, не индексируемая iNative, E-024 материалы `t03.out/bytes-table2.json`), T-016, T-017 (использовать `-FIXEDSEED`), T-020a, T-021a, T-022, T-034…
4. Windows — первая платформа (D-014); Linux/macOS не трогать.

Незавершённые изменения: весь код и план закоммичены (последний коммит — см. `git log`); вне git остаются только `.codex/config.toml`, `.mcp.json` (изменены до сессии, не мои), `.idea/`, `.mcp.json.tmp` и `tools/research/rea-show.mjs` (untracked, существовал до сессии — используется, но не мой). REA-сервер сессии остановлен файлом `STOP`; новой сессии нужен повторный импорт (≈27 мин): `. .	oolsesearchea-env.ps1; node tools/research/rea-session.mjs --serve .research/rea/queue`, первый пакет начинается с `open_binary` (путь с обратными слэшами — писать JSON из .mjs через `String.raw`, не из bash `node -e`). Сеть, совместимость runtime и Linux/macOS этой сессией не проверялись.

## Сессия 2026-10-23 (Claude Code, Opus 5.5)

- На входе: незакоммиченная работа после коммита `d1ad7c5` — Codex (T-015a IN_PROGRESS, `save.rs`/`save_scan.rs` T-016, тест-векторы RNG) и неоформленный задел T-037 (`level.rs`, `model.rs`, `level_scan.rs` со ссылкой на несуществовавшую E-067). Тесты проходили; задел проверен, исправлен и доведён. Процесс Codex-приложения был запущен, но файлы проекта с 2026-10-22 им не менялись.
- **T-037 DONE** (E-067): нативные данные всех классов уровней разобраны до байта на всей установке; исправлена ветка `FLightMap1D` (сэмплы — `FUntypedBulkData`); добавлены StaticMeshComponent (362 726), DecalComponent, ShadowMap1D, Terrain, TerrainComponent, GBXNavMesh, WiringMesh, «нулевые» хвосты; `Serialize` Terrain/TerrainComponent/GBXNavMesh/FMaterial/FTerrainMaterialResource подтверждены REA; **все 6 011 cross-level ссылок разрешены** (`package.rs::level_guids`, `CrossLevelGuids::import_guid` + unit-тест).
- **T-038 IN_PROGRESS** (E-068): `material.rs` — `FMaterial` и `FStaticParameterSet` у всех 77 359 объектов с нативными данными; Material/DecalMaterial/terrain 100 % с картой шейдеров; 8 105 MIC без карты — открытый вопрос.
- REA 6.7.0/Ghidra 12.1.4 доступен (CLI + `rea-session.mjs`); импорт exe 26.8 мин; очередь `.research/rea/claude-1023/00…37`. Хеш exe `1231B384…4C17` перепроверен 2026-10-23.
- Проверки: `cargo test --workspace` (20 + 6 unit) PASS, `cargo clippy --workspace --all-targets` без предупреждений (исправлены также предупреждения в `save.rs` Codex: `as_chunks`); ignored-сканы с `BL2_INSTALL`: `level_scan` (14 тестов, лог `.research/T-037-level-scan.log`), `shader_cache_scan` (оба), `static_mesh_scan`, `property_scan` — PASS.

## Сессия 2026-10-23 (2) (Claude Code, Opus 5.5)

- **T-038 DONE** (E-068): для 8 105 MIC без карты оригинал компилирует перестановку в рантайме (`FMaterial::CacheShaders` 0x64b620 → виртуальный `Compile`); D-008 дополнен, риск R-19, новая задача T-038a.
- **T-020a DONE** (E-069): `vorbis.rs` — Wwise Vorbis → стандартные пакеты Vorbis, библиотека 598 codebooks из exe (VA 0x19dad68), PCM через `lewton` 0.10.2 (`17-dependencies.md`); 121 555/121 555 медиа; 20 медиа = vgmstream r2117 (`.tools/vgmstream-r2117/`) по длине и до ±2 LSB.
- **T-020b IN_PROGRESS** (E-071): `wwise::ConvolutionIr` — 50/50 IR; алгоритм свёртки и ID параметров восстановлены REA; осталось — параметры FxShareSet, реализация, сверка.
- **T-021a DONE** (E-070): актёры FaceFX в `FaceFXAsset` (архив `FACE` v1740), анимации — в метках cue медиа Wwise (`wwise::{cue_labels, facefx_animation}`, 16 594 медиа); точки влияния на логику перечислены; продолжение T-021b, риск R-20.
- **T-022 IN_PROGRESS** (E-033): классификация PhysX (логика: транспорт, лут, KActor/импульсы, хитбоксы; визуально: ткань, PhysX-частицы, жидкости, поля, APEX); рэгдолл на место лута не влияет; D-009 уточнён (остаётся PROPOSED).
- T-017 по-прежнему не начат: требует запуска оригинала и ручного прохождения сценария L-1 (интерактивный ввод) — для агента без управления вводом заблокирован; T-018 зависит от него.
- REA 6.7.0/Ghidra 12.1.4: импорт 2026-10-23 в `.research/rea/claude-1024/` (очередь 00…29); хеш exe проверен.
- Проверки: `cargo test --workspace` (23 + 6 unit) PASS, `cargo clippy --workspace --all-targets` без предупреждений; ignored-сканы: `vorbis_scan` (`headers_and_packets`, `decode_all` ≈19 мин, `compare_samples`, `cue_labels_and_facefx`, `convolution_irs`), `level_scan::{facefx_objects, physics_objects}`, `shader_cache_scan` — PASS.

## Следующий шаг

1. **T-021b** IN_PROGRESS: архивы FaceFX разбираются до байта (`facefx.rs`, E-072); link-функции 1…5 и интервал анимации реализованы; «играет ли» определяет экземпляр FaceFX, тайминг реплик восстановлен (звук стартует через |start| после `PlayFaceFXAnim`, «играет» — до min(последний ключ, конец звука); `bl2-sim::facefx::Playback`); кривые, link-функции и вычисление графа — `bl2-sim::facefx` (E-072); тайминг сверен с оригиналом побитно на 12 репликах (E-072, реплика длится до min(последний ключ, конец звука)); связь выходов графа с костями/морфами — вместе с T-036 (точно — `12-tasks.md` T-021b), **T-036** IN_PROGRESS (E-073): `anim.rs` декодирует все 100 601 последовательность, сверено с оригиналом побитно (кроме Delta48 — нет в доступных сценах); `SkeletalMesh` (3 911, вершины 9,1 млн), `AnimSet`, `MorphTarget` разобраны (E-074); сборка позы `LocalAtoms → SpaceBases` сверена с оригиналом (E-076: 36/40 поз в пределах 0,01 uu, `bl2-sim::anim`); «последовательность → LocalAtoms» совпала у 85 из 128 компонентов с одной последовательностью (корень — `GetBoneAtom` без инверсии w); следующий шаг T-036 — перехват момента построения позы компонента (чтобы время узла и поза были одного шага) и узлы `AnimTree` персонажей (смешивание, `SkelControl*`).
2. IN_PROGRESS с конкретными шагами: T-020b (E-077: эффект `bl2-audio::convolution::ConvolutionReverb` перенесён целиком для ID 32 = 0 и проверен на данных; следующий шаг — Frida-перехват `Execute` 0x1355050 (буфер до/после, FX+0x70 параметры, FX+0x13c маска) в уровне с реверберацией и побитная сверка; затем — где лежат IR 14 наборов), T-022 (ветки `Behavior_PhysXLevel` — после разбора `BehaviorProviderDefinition`), T-012/T-013 (шаги в разделе 2026-10-22 ниже остаются в силе), T-034 (формат и правило подмены Mancana Remaster — E-075; осталось: когда загружаются таблицы Remaster — `UWillowDownloadableContentManager::execRemasterRefresh`, затем декодирование в `Image` с пиксельной сверкой), T-016 и T-015a (работа Codex — не перехватывать без проверки его очереди `.research/rea/codex-t015a`).
3. **T-017 DONE** (E-028): L-1a записан дважды под Frida (`tools/research/frida-record.mjs`, `compare-runs.mjs`); следующий шаг записи оригинала — **T-017a** IN_PROGRESS: ввод мышью через Frida уже работает (`m dx dy` в `<out>.jsonl.ctl`); позиция/поворот/скорость пешки уже пишутся на каждом кадре (E-028 (9): GObjects/GNames найдены, `Controller.Pawn` 0x190, `Actor.Location` 0x60); осталось — события миссий и решение, как записывать полный L-1 с боем (эталон одного прохождения + повторяемость коротких отрезков?) — записать в `14-decisions.md`, затем прохождение; **T-018** IN_PROGRESS: шаг 0 сделан (E-029: LAN двух экземпляров, обнаружение UDP 14001, игра UDP 7777); следующий шаг — полный захват (переход уровня, сценарии `09-network.md` §5, выход через меню, чтобы сохранить логи) и повтор «A-хост первым» для причины падения первого хоста; T-038a — после T-082.
4. Windows — первая платформа (D-014); Linux/macOS не трогать.

Незавершённые изменения: всё закоммичено (T-020b: новый крейт `bl2-audio`; проверки — `cargo test --workspace`, `cargo clippy --workspace --all-targets`, ignored `bl2-audio --test ir_scan` (оба теста) и `vorbis_scan::{fx_objects, convolution_irs}` с `BL2_INSTALL` — PASS); вне git только `.codex/config.toml`, `.mcp.json` (изменены до сессии, не мои), `.idea/`, `.mcp.json.tmp`. Производные данные — в `.research/` (`T-020a-samples/` с .wem/.wav, логи) и `.tools/vgmstream-r2117/` (оба в .gitignore). REA-сервер очереди `.research/rea/claude-1030` (импорт 2026-10-25, очередь 00…13; декомпиляции свёртки — `.research/T-020b-conv*.c`, `.research/T-020b-kiss.c`; остановлен) останавливается файлом `STOP` в конце сессии; новой сессии нужен повторный импорт (≈27 мин). Сеть, совместимость runtime и Linux/macOS этой сессией не проверялись.
