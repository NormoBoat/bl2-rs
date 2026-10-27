# 17 — Зависимости: версии, обоснование, аудит

## 1. Проверка перед добавлением зависимости (обязательна, записывается в §2)

1. Нужна ли библиотека для полного выполнения требований (а не «удобно»)? Ponytail: не добавлять зависимость ради нескольких строк.
2. Можно ли решить средствами std/Rust/Bevy или уже подключённых крейтов?
3. Поддерживает ли Windows, Linux, macOS (x86_64 и arm64)?
4. Какие транзитивные и платформенные зависимости добавляет (`cargo tree -e normal,build -p <крейт>`)?
5. Лицензия совместима (предпочтительно MIT/Apache-2.0/BSD/Zlib; copyleft — только отдельным решением)?
6. **Реализована ли сама библиотека на Rust**: нет `-sys`-крейтов с C/C++, нет `cc`/`cmake`/`bindgen` в build-зависимостях, нет вендорного C-кода. Rust-интерфейс к C-библиотеке не считается Rust-реализацией. Исключения — системные API ОС/драйверы/графические API (инфраструктура, `00-scope.md` §3) и **только один** не-ОС код: опциональный клиент Steam Valve (`steamclient64`), установленный у пользователя и загружаемый в рантайме нашим Rust-кодом вместо `steam_api` (D-010). Других исключений нет.

## 2. Реестр зависимостей

| Крейт | Версия | Где | Зачем | Альтернатива | Rust-реализация | ОС | Лицензия | Статус |
|---|---|---|---|---|---|---|---|---|
| bevy | =0.20.0 | `bl2-sim`, apps, рендер | Обязателен (R-2) | — | Rust; транзитивно — wgpu, winit, cpal и др. (аудит T-050) | Win/Linux/macOS | MIT OR Apache-2.0 | PROPOSED (D-003) |
| LZO1X-декомпрессия | — | `bl2-formats` | Сжатие пакетов (LZO1X подтверждён, E-040) | Собственная реализация | — | все | — | **Рекомендация: без зависимости.** Декодер занимает ≈50 строк (рабочий эталон — `tools/research/lzo-chunk-check.mjs`, проверен на 11 пакетах), Ponytail запрещает зависимость ради нескольких строк. Компрессор нужен только если сохранения требуют LZO (T-016) — тогда решать отдельно. Альтернатива `lzokay-native` 0.1.0 (pure Rust, MIT, 2023-08-11) — не нужна |
| `steam_api` (замена) | — | `bl2-net` (`steam.rs`) | Онлайн через Steam (D-010) | Свой Rust-код (~150 строк, эталон `tools/research/steam_probe.rs`, E-055) | Да | Windows проверен; Linux/macOS после D-014 | — | ACCEPTED |
| Клиент Steam Valve (`steamclient64.dll`, установлен у пользователя) | тот, что установлен (на 2026-10-21: 11.12.30.07) | загружается `steam.rs` в рантайме | Транспорт Steam P2P/лобби | — (протокол Steam недоступен иначе) | **Нет — код Valve. Единственное исключение (D-010)**; опционально, не распространяется, не линкуется | Windows; аналоги на Linux/macOS — исследовать | проприетарный клиент Steam пользователя | ACCEPTED как исключение |
| Загрузка DLL/реестр | — | `steam.rs` | Нужна только для Steam | Прямые `extern "system"` объявления `LoadLibraryExW`/`GetProcAddress`/`RegGetValueW` (как в пробе) — без крейта; `libloading`/`windows-sys` не нужны (Ponytail) | Rust поверх API ОС | Windows | — | ACCEPTED |
| SHA-256 | — | `bl2-formats` (`install.rs`) | Проверка сборки и манифеста (`06-assets.md` §4, T-033) | `sha2` (RustCrypto) | Собственная реализация FIPS 180-4, ≈60 строк, тест-векторы NIST + хеш exe (E-066) | все | — | ACCEPTED: без зависимости (Ponytail) |
| miniz_oxide | 0.8.9 (+ adler2 2.0.1) | `bl2-formats` (`swf.rs`) | zlib-распаковка роликов `CFX` (E-032) | собственный inflate (~200 строк) | Да: чистый Rust, без build-скриптов и C (`cargo tree` — только adler2) | все | MIT OR Zlib OR Apache-2.0 | ACCEPTED (2026-10-22): тот же крейт подтягивает Bevy через png — в итоговом дереве новых крейтов не добавится (проверить в аудите T-050) |
| lewton | =0.10.2, `default-features = false` (+ byteorder 1.5.0, tinyvec 1.13.3) | `bl2-formats` (`vorbis.rs`) | Декодер Vorbis для 121 555 медиа Wwise Vorbis (T-020a, E-069): Wwise-пакеты перестраиваются в стандартные пакеты Vorbis нашим кодом, lewton декодирует их без Ogg | Свой декодер Vorbis I (MDCT, floor1, residue, ≈1 500–2 000 строк) — Ponytail: крейт уже чисто-Rust и тот же, что использует аудиостек Bevy (rodio/vorbis) | Да: чистый Rust, без build-скриптов и C (`cargo tree -p bl2-formats`: lewton → byteorder, tinyvec), feature `ogg` отключена | все | MIT OR Apache-2.0 (byteorder Unlicense OR MIT, tinyvec Zlib OR Apache-2.0 OR MIT) | ACCEPTED (2026-10-23) |
| остальное | — | — | Добавлять по задачам | — | — | — | — | — |

Каждая добавленная зависимость получает строку здесь в той же задаче.

## 3. Аудит «только Rust» (выполнять в T-050 и при каждом изменении `Cargo.lock`)

1. `cargo tree -e normal,build --target all` → список всех крейтов.
2. Отметить крейты `*-sys`, крейты с build-зависимостями `cc`, `cmake`, `bindgen`, `pkg-config`.
3. Для каждого отмеченного: это (а) системный API/драйвер ОС (`windows-sys`, `objc2`-фреймворки Apple, `libc`, `alsa-sys` → libasound системы, `wayland-sys`, `x11-dl`, Vulkan loader, Metal) — **разрешено как инфраструктура**, записать; или (б) вендорный C/C++ код библиотеки (`zstd-sys`, `basis-universal`, `libz-sys`, `shaderc` и т.п.) — **запрещено**, отключить feature/заменить.
4. Известные места риска в Bevy (проверить для 0.20 по `Cargo.toml` Bevy): features сжатия (zstd: выбирать чисто-Rust вариант), `basis-universal` (C++), декодеры изображений/аудио (по умолчанию в Bevy — Rust-реализации, проверить), `bevy_gltf` (не нужен). Использовать `default-features = false` и явный список features.
5. Результат — `plan/evidence/raw/dep-audit-<дата>.txt` + запись E-номером.

## 4. Инструменты исследования (не runtime)

| Инструмент | Версия | Назначение |
|---|---|---|
| REA (`rea-agents`) | 6.7.0 | Обязательный инструмент RE (MCP/CLI) |
| Ghidra | 12.1.4 | Провайдер REA |
| JDK | 21.0.12.1 | Для Ghidra |
| Node.js | 22.23.3 | REA, скрипты `tools/research/*.mjs` |
| Rust toolchain | rustc/cargo 1.99.1 (stable) | Сборка; MSRV проекта = MSRV Bevy (1.97.1) |
| Wireshark / TShark / capinfos | 4.6.9 (winget `WiresharkFoundation.Wireshark`) | Захват и разбор трафика оригинала; провайдер PCAP для REA |
| Npcap | 1.89 (npcap.com, подпись Nmap Software LLC проверена; поддержка loopback включена) | Драйвер захвата для Wireshark |
| Frida (npm `frida`) | 17.x (`.tools/frida/`, `npm install frida@17`) | Запуск и инструментирование оригинала (T-017): перехват API, запись событий; не runtime |
| vgmstream-cli | r2117 (GitHub release `vgmstream-win64.zip`, SHA-256 `6c4a8a3813864fefed081bbd337dbc0ad93bf88e0b92f5db98d7ab258b22dc6c`), в `.tools/vgmstream-r2117/` | Независимая сверка декодирования Wwise Vorbis (свои codebooks + libvorbis), T-020a; не runtime |
