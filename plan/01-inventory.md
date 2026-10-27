# 01 — Инвентаризация материалов

Дата инвентаризации: 2026-10-21. Машина исследования: Windows 11 Pro 10.0.26300, Intel Xeon E5-2670 v3, AMD Radeon RX 580 2048SP (драйвер 31.0.21925.1001). Источники — `03-evidence.md` (E-номера).

## 1. Репозиторий `D:\Projects\bl2-rs`

| Путь | Что это | Статус |
|---|---|---|
| `Borderlands 2/` | Копия установки оригинала (Steam, AppID 49520). 13 925 файлов (без двух посторонних, см. ниже) | VERIFIED, E-001…E-007 |
| `Borderlands 2/borderlands2-rs.exe`, `borderlands2-rs-smoke.png` | Артефакты прошлой попытки (Bevy 0.17.3, без исходников) | Удалены пользователем 2026-10-21 (E-011) |
| Steam-установка `D:\SteamLibrary\steamapps\common\Borderlands 2` | Та же сборка, запускается через Steam (вне рабочей директории — только чтение и запуск) | VERIFIED, E-045, E-049, D-015 |
| `.research/user-data/2026-10-21/` | Копии конфигов, лога и сохранений оригинала (не коммитятся) | E-048 |
| `.tools/ghidra_12.1.4_PUBLIC/` | Ghidra 12.1.4 | VERIFIED, E-012 |
| `.tools/jdk-21.0.12.1+1/` | JDK 21.0.12.1, 64-bit | VERIFIED, E-012 |
| `.tools/downloads/` | ghidra.zip (569 649 598 Б), jdk.zip (205 073 461 Б) | наличие |
| `.tools/ghidra-smoke.{c,dll,obj}` | Собственный тестовый DLL прошлой сессии (`rea_add_seven`), не код игры | VERIFIED (прочитан `ghidra-smoke.c`) |
| `.tools/rea/` | REA (`rea-agents`) 6.7.0, установлен в этой сессии | VERIFIED, E-012 |
| Исходный код, Cargo workspace, git | **Отсутствуют**. `git` не установлен (`Get-Command git` → не найден) | VERIFIED, E-016 |
| AGENTS.md / CLAUDE.md | До этой сессии отсутствовали; созданы | — |

Инструменты на машине (E-016): rustc 1.99.1, cargo 1.99.1, rustup; Node 22.23.3; Claude Code CLI; git 2.56.0 (установлен пользователем 2026-10-21); Wireshark/TShark 4.6.9 + Npcap 1.89 (E-050). Нет: Python (только заглушка Microsoft Store), gh, Codex CLI в PATH (Codex установлен как приложение: `~/.codex/config.toml`, `~/.cache/codex-runtimes`).

## 2. Оригинал: исполняемые файлы (`Binaries/Win32`)

| Файл | Версия | Назначение (по имени/строкам) | Статус |
|---|---|---|---|
| `Borderlands2.exe` | 1.0.257.2863302, PE32 i386, 27 452 248 Б | Игра (UE3 + Willow) | VERIFIED |
| `steam_api.dll` | 01.55.77.96 | Steamworks API | VERIFIED (версия ресурса) |
| `SteamWrapper.dll`, `EasyHook32.dll` | — | Вероятно, лаунчер/оверлей | HYPOTHESIS |
| `bifrost.dll` | PE32, 2020-03-05 | Клиент сервисов 2K/SHiFT («Spark»): строки `/v1/client/{title}/shift_codes/redeem`, libcurl, OpenSSL, WebRTC `rtc_base` | VERIFIED (строки), назначение HYPOTHESIS |
| `PhysXCore.dll` и др. | PhysX 2.8.4.6 | Физика | VERIFIED (версия ресурса) |
| `APEX_*` | 1.0.0.1 | APEX Clothing/Destructible | VERIFIED |
| `binkw32.dll` / `bink2w32.dll` | 1.99p / 2.7f | Видео Bink 1 / Bink 2 | VERIFIED |
| `cudart*.dll`, `nvtt.dll`, `fbxsdk_*` | — | GPU PhysX, инструменты | наличие |
| `Launcher.exe` + .NET-сборки | 1.0.0.0 | Лаунчер (.NET 4, XNA) | наличие |
| `MMOverlay/` (262 файла) | — | Оверлей (Achievements/Friends/Theme) | наличие |

Встроены в `Borderlands2.exe` (строки, E-010; лог, E-046): FaceFX 1.7.4 (лицевая анимация), Scaleform GFx (`GFx_DefineBitsJpeg2Loader`, `gfxpixelshader.usf`), Wwise (`AWwiseSoundGroupexec…`), D3D9-рендер (`GetD3D9CubeFace`), Bink-импорты, SteamAPI (`SteamAPI_Init`, `SteamNetworking`, `SteamMatchmaking`, `SteamGameServer`). Найдено 6 893 уникальных имён нативных функций вида `<Класс>exec<Функция>` (`evidence/raw/exe-native-exec-names.txt`, E-014).

## 3. Оригинал: данные

Сводка по расширениям (весь каталог установки):

| Тип | Кол-во | Объём | Что это |
|---|---|---|---|
| `.upk` | 2 010 | 2.63 GB | UE3-пакеты (v832/L46). 11 «полностью сжатых» с `.uncompressed_size` (Core, Engine, GameFramework, GearboxFramework, GFxUI, IpDrv, OnlineSubsystemSteamworks, AkAudio, WillowGame, Startup, Startup_LOC_INT) — содержат скриптовые классы |
| `.tfc` | 75 | 12.96 GB | Texture File Cache (Textures, CharTextures, Lighting, DLC, Mancana 4K) |
| `.bik` | 279 | 5.49 GB | Видео Bink (в т.ч. `*_4K.bik` Mancana) |
| `.pck` | 68 | 3.41 GB | Аудио-паки (`Audio_Banks.pck`, `Audio_Streaming.pck`, DLC) — Wwise, формат проверить (T-020) |
| `.usf` | 163 | 1 MB | **Исходники шейдеров UE3** в `Engine/Shaders` (включая Gearbox-специфичные `GBXShadowProjection*`, `Grendel_*`, `EdgeDetectionPostProcessBlend*`, `ItemCardThumbnail*`) |
| `.bin` | 231 | 1 MB | `Engine/Shaders/Binaries` (163), `GlobalShaderCache-PC-D3D-SM3.bin`, `PersistentCookerShaderData.bin`, лицензии DLC, `contver.bin` |
| Локализация | ≈11 800 | 46 MB | `.int/.deu/.esn/.fra/.ita/.jpn` (+ `.kor/.twn` в DLC). Каталоги KOR/TWN в `WillowGame/Localization` пусты |
| `.ini` | 63 | — | `Engine/Config/Base*.ini`, `WillowGame/Config/Default*.ini`, `DLC/*/WillowDlc.ini` |
| Шрифты | `.ttf` 3, `.otf` 3 | — | |

`WillowGame/CookedPCConsole`: 914 `.upk` (37 с суффиксом `_P` — persistent-уровни), подкаталог `English(US)` (2 файла, 516 MB — вероятно, локализованное аудио).

DLC (PackageId из `WillowDlc.ini`, E-009):

| Каталог | PackageId | Файлов | GB | | Каталог | PackageId | Файлов | GB |
|---|---|---|---|---|---|---|---|---|
| Orchid | 1 | 1233 | 0.94 | | Lilac | 6 | 219 | 0.20 |
| Tulip | 2 | 180 | 0.15 | | Gardenia01 | 7 | 52 | 0.07 |
| Iris | 3 | 948 | 0.90 | | Gladiolus | 8 | 53 | 0.00 |
| Sage | 4 | 1449 | 0.69 | | Aster | 9 | 1919 | 1.51 |
| Flax | 5 | 249 | 0.26 | | Lobelia | 10 | 402 | 0.26 |
| PN2 | 12 | 46 | 0.02 | | LootChest2 | 14 | 47 | 0.01 |
| Gardenia04 | 16 | 42 | 0.05 | | Gardenia05 | 17 | 31 | 0.03 |
| CommDay2013 | 18 | 58 | 0.05 | | Allium | 19 | 486 | 0.51 |
| Nasturtium | 20 | 610 | 0.57 | | Peony | 21 | 58 | 0.05 |
| Anemone | 22 | 1479 | 3.61 | | Mancana | 23 | 28 | 9.01 |
| Anecana | 24 | 4 | 0.00 | | POCollectors/POGamestop/POPremierClub | — (UnlockId 3/1/2) | 3 | 0 |

PackageId 11, 13, 15 в установке отсутствуют — **UNKNOWN**: это не установленные/не купленные DLC или неиспользуемые ID (T-006, вопрос пользователю + исследование `DefaultDLCManager.ini`).

Полный манифест с SHA-256 каждого файла: `plan/evidence/raw/install-manifest.sha256.csv` (13 925 строк; генерация — команда в `03-evidence.md` E-007). Это эталон целостности для всех дальнейших исследований.

Символы и скрипты: PDB нет (в exe только ссылка на `ShippingPC-WillowGame-PCConsole.pdb`, E-042); скрипты — байткод в 11 полностью сжатых пакетах (распаковка подтверждена, E-040); исходников UnrealScript — UNKNOWN (T-015).

## 4. Окружение оригинала (обновлено 2026-10-21 после ответа пользователя)

- Оригинал установлен через Steam в библиотеку `D:\SteamLibrary` (buildid 9218157) и запускается (E-045, E-046). Ранее (E-015) установка не была видна: проверялась только основная библиотека Steam.
- «Документы» перенаправлены в OneDrive: данные игры — `C:\Users\normo\OneDrive\Documents\My Games\Borderlands 2` (конфиги `WillowGame\Config\Willow*.ini`, лог `WillowGame\Logs\Launch.log`, сохранения `WillowGame\SaveData\<SteamID64>\`). Копии — `.research/user-data/2026-10-21/` (E-048).
- Сохранения: `profile.bin` + 6 персонажей (`Save0001…0006.sav` и `.bak`), даты 2018–2026; синхронизация Steam Cloud. Оболочка формата подтверждена (E-047). К целевой сборке достоверно относятся только записанные после 2026-10-21 20:12 (`Save0006`, `profile.bin`).
- Для работы новой реализации важно: путь к «Документам» брать через системный API известных папок (не строить `%USERPROFILE%\Documents` вручную) — `16-platforms.md` §3.

## 5. Чего не хватает (входные данные от пользователя)

| ID | Что нужно | Зачем | Состояние (2026-10-21) |
|---|---|---|---|
| M-1 | Возможность запускать оригинал | Эталонные записи поведения, графики, логов | **Закрыт**: запуск через Steam работает (E-046) |
| M-2 | Реальные сохранения этой сборки | Формат сохранений | **Частично**: 6 персонажей есть (E-047), но только `Save0006`/`profile.bin` записаны целевой сборкой; для эталонов записи — создавать новые сохранения этой сборкой в ходе T-017 |
| M-3 | Второй экземпляр игры | Сетевые эксперименты | **Подход выбран** (D-016): два экземпляра на этой машине; осуществимость — E-UNK-14, T-018 |
| M-4 | Машины Linux и macOS | Проверка платформ | **Отложен решением пользователя** до готовности Windows (D-014) |
| M-5 | Решение о Steam (D-010) | Онлайн-совместимость | **Закрыт**: Steam API — единственное опциональное исключение; без него — только LAN. `steam_api` заменяется своим Rust-кодом поверх установленного `steamclient64` — проба работает (E-055) |
| M-6 | Подтверждение немодифицированности установки | Исключить смешение сборок | **Закрыт**: копия побайтно равна свежей Steam-установке (E-049) |
| M-7 | Удаление посторонних файлов из копии | Чистота эталона | **Закрыт**: удалены пользователем |
| M-8 | Сведения о DLC PackageId 11/13/15 | Полнота контента | Открыт; Steam-манифест (41 depot) — исходные данные для T-006 |
| M-9 | Git | Версионирование | **Закрыт**: git 2.56.0 |
| M-10 | Wireshark/Npcap | Захват трафика оригинала | **Закрыт**: установлены 4.6.9 / 1.89 (E-050) |
