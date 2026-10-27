# 02 — REA и инструменты исследования

REA обязателен для всех агентов (Claude, ChatGPT/GPT/Codex). Не выдумывайте команды, имена MCP-инструментов, адреса и результаты: используйте только то, что подтверждено `--help`, `rea capabilities`, `docs/mcp-contracts.md` закреплённой версии или фактическим вызовом.

## 1. Закреплённые версии (проверено 2026-10-21, E-012)

| Компонент | Версия | Где | Источник |
|---|---|---|---|
| REA (`rea-agents`, npm) | **6.7.0** (npm latest на 2026-10-21) | `.tools/rea/node_modules/rea-agents` | `rea --version` → `6.7.0`; npm `time.modified` 2026-10-18 |
| REA репозиторий | main на момент чтения документации: `d8a9673fc44fde4bd30ff98c241886e6683580ca` (2026-10-21) | github.com/morluto/rea | GitHub API; документация читалась с `main`, npm-пакет 6.7.0 — исполняемый эталон |
| REA каталог | 153 CLI-команды, 207 MCP-инструментов, 6 MCP-prompt; `combined_sha256 96d8f6e6…441c` | — | `rea doctor` → `identity.catalog` (`evidence/raw/rea-doctor-ghidra.json`) |
| Провайдер | Ghidra **12.1.4** | `.tools/ghidra_12.1.4_PUBLIC` | doctor: `ghidra-version 12.1.4` |
| JDK | 21.0.12.1, 64-bit full JDK | `.tools/jdk-21.0.12.1+1` | doctor: `ghidra-java` |
| Node.js | 22.23.3 (требование REA: 22.x ≥ 22.19) | PATH | doctor: `node` |

Недоступные провайдеры (doctor): Hopper (macOS), IDA (не настроен), JADX, Kuna — не нужны для текущих целей. SQLite-инспекция требует Node ≥ 24.15 — сейчас недоступна (не нужна).

Ограничение REA на Windows (docs/windows-ghidra-p0.md): только чтение; анализ — не песочница; холодный импорт больших PE долгий; известна проблема с анализатором `WindowsResourceReference` (при таймауте задать `REA_GHIDRA_DISABLED_ANALYZERS=WindowsResourceReference` и зафиксировать это в доказательстве).

## 2. Подключение

### CLI (работает всегда, основной путь для воспроизводимости)

```powershell
. .\tools\research\rea-env.ps1          # задаёт GHIDRA_INSTALL_DIR, JAVA_HOME, REA_ANALYSIS_PROVIDER=ghidra, таймаут
rea doctor --provider ghidra --json      # здоровье провайдера
rea providers --json
rea capabilities --json
```

Установка на новой машине (Windows): распаковать Ghidra 12.1.4 и JDK 21 в `.tools/`, затем
`npm install --prefix .tools/rea rea-agents@6.7.0`. На Linux/macOS — те же версии; `rea-env.ps1` не подходит, задать переменные вручную (см. `docs/installation.md` REA).

### MCP

- **Claude Code**: проектный `.mcp.json` (сервер `rea`, node + `.tools/rea/.../scripts/rea.mjs mcp`, переменные Ghidra). При первом запуске Claude Code спросит разрешение на проектный сервер.
- **Codex**: проектный `.codex/config.toml` (тот же сервер). Загружается ли проектный конфиг вашей версией Codex — **HYPOTHESIS**, проверить в T-005. Глобальный `~/.codex/config.toml` пользователя содержит устаревшую регистрацию REA 6.5.0 (`C:\Users\normo\Documents\Codex\REA`) без переменных Ghidra (E-013) — не менять без согласия пользователя; если проектный конфиг не подхватывается, использовать CLI.
- Пути в `.mcp.json`/`.codex/config.toml` абсолютные для этой машины; на другой машине поправить.
- Холодный импорт `Borderlands2.exe` длится десятки минут: в MCP сначала открыть бинарник с `snapshot_path`, чтобы последующие сессии переиспользовали анализ.

## 3. Проверка доступности в начале сессии

1. MCP: вызвать инструмент `binary_session` сервера `rea` (существование подтверждено `rea mcp doctor`, E-012); если MCP-инструменты `rea` не видны — шаг 2. Сам сервер можно проверить CLI-командой `rea mcp doctor --json` (ожидается `healthy:true`, 207 инструментов).
2. CLI: `. .\tools\research\rea-env.ps1; rea --version; rea doctor --provider ghidra --json`. Ожидается `"healthy":true`, `ghidra-version 12.1.4`.
3. Результат (доступен MCP / только CLI / недоступен) записать в `status.md` → «Состояние инструментов».

## 4. Протокол исследования (каждое исследование)

1. Сначала поискать в `03-evidence.md`: не исследовано ли уже. Повтор — только с причиной (сомнение, новая версия инструмента, расхождение).
2. Сформулировать вопрос и ожидаемый тип ответа; указать целевой файл и его SHA-256 из манифеста.
3. Выполнить вызов REA (CLI с `--json` или MCP). Сохранить сырой вывод в `plan/evidence/raw/<ID>-<кратко>.json|txt`, если он не содержит крупных фрагментов кода оригинала; иначе — в `.research/` (не коммитится), а в `evidence/raw` — краткую выписку.
4. Записать в `03-evidence.md`: ID, утверждение, статус (VERIFIED/HYPOTHESIS/UNKNOWN), сборка (хеш), инструмент и версия, точная команда/вызов, адреса/смещения, ограничения, ссылка на сырой вывод.
5. Если результат опровергает гипотезу, на которой стоят задачи/решения, — исправить их сразу (`15-handoff.md` §5).

## 5. Стоимость запросов, сессии и снапшоты (важно, E-017)

- Холодный импорт + автоанализ `Borderlands2.exe` в Ghidra через REA занимает **≈27 минут** (E-017; 109 710 процедур).
- Импорт **эфемерный**: база Ghidra живёт, пока жива сессия провайдера, и удаляется при её закрытии. Каждый новый процесс `rea …` (CLI) импортирует бинарник заново.
- Снапшот `.research/rea/Borderlands2.exe.snapshot.json` хранит evidence и привязку профиля, а не базу Ghidra; повторный CLI-вызов со снапшотом снова длится ≈27 мин (E-044).
- Поэтому: **серии запросов делать в одной долгоживущей MCP-сессии** (Claude Code/Codex держат процесс `rea mcp` живым; `open_binary` один раз, затем `list_procedures`/`xrefs`/`procedure_pseudo_code`/`read_bytes`…). CLI — для одиночных проверок и воспроизводимых команд в доказательствах. В `rea-agents` 6.7.0 **нет** `rea mcp --http` (команда отклоняется: у `rea mcp` только `add` и `doctor`), хотя `docs/cli.md` на `main` её описывает; вместо этого — режим очереди `rea-session.mjs --serve` (ниже).
- Таймаут MCP-клиента должен покрывать первый импорт (в `.codex/config.toml` задан `tool_timeout_sec = 7200`). **Отмена запроса по таймауту заставляет REA удалить базу Ghidra** — следующий запрос снова импортирует 27 мин.
- **Пакетный режим без MCP-клиента агента** (проверено, E-022): `. .\tools\research\rea-env.ps1; node tools/research/rea-session.mjs <calls.json> <out-dir>` — запускает `rea mcp` один раз и выполняет список вызовов `[{ "id", "tool", "args" }]` (первый — `open_binary` с `path` и `provider_id: "ghidra"`); первый запрос ≈27 мин, остальные — десятки мс. Список инструментов и схем: `node tools/research/rea-session.mjs --list-tools plan/evidence/raw/rea-mcp-tools.json`.
- **Итеративная работа**: `node tools/research/rea-session.mjs --serve .research/rea/queue` (в фоне) держит сессию; каждый файл-пакет `<имя>.json` в очереди выполняется, результаты — в `<имя>.out/`, файл переименовывается в `<имя>.done`; файл `STOP` завершает сервер. Первый пакет должен начинаться с `open_binary`.
- Снапшот не коммитится (производный от оригинала).

## 6. Основные CLI-команды REA для этого проекта (подтверждены `docs/cli.md` / `--help` 6.7.0)

```powershell
$exe='D:\Projects\bl2-rs\Borderlands 2\Binaries\Win32\Borderlands2.exe'
$snap='D:\Projects\bl2-rs\.research\rea\Borderlands2.exe.snapshot.json'
rea analyze  $exe --provider ghidra --snapshot $snap --format json          # обзор
rea search   $exe "AWillowPawnexecTakeDamage" --provider ghidra --json      # поиск строк/символов
rea function $exe 0xADDR --provider ghidra --json                           # досье функции
rea decompile $exe 0xADDR --provider ghidra --json                          # псевдокод
rea xrefs    $exe 0xADDR --provider ghidra --json                           # ссылки
rea trace    $exe "search" --provider ghidra --json
rea compare  left.json right.json                                           # сравнение evidence
```

MCP-инструменты, упомянутые в документации провайдера Ghidra: `binary_overview`, `list_procedures`, `list_strings`, `list_names`, `procedure_pseudo_code`, `xrefs`, `read_bytes`, `search_bytes`, `read_function_instructions`, `trace_native_values`, `open_binary`/`close_binary` (с `snapshot_path`). Полный каталог — `rea capabilities --json` (`evidence/raw/rea-capabilities.json`) и `docs/mcp-contracts.md` версии 6.7.0. Прочие имена инструментов использовать только после проверки в каталоге.

Дополнительно доступны в REA и пригодятся: `rea inspect-managed-members` (.NET лаунчер — низкий приоритет), `capture-process`/`compare-process-captures` (поведенческие захваты оригинала, требуют M-1), анализ PCAP через TShark (`docs/pcap-analysis.md`, TShark не установлен — T-018), Frida-инструментирование (`docs/frida-instrumentation.md`, Frida не установлена).

## 7. Дополнительные инструменты (вне REA)

| Инструмент | Назначение | Почему не хватает REA | Статус |
|---|---|---|---|
| `tools/research/upk-header-survey.mjs` (Node) | Массовый обзор заголовков 2 010 пакетов | Форматы UE3-пакетов не входят в REA | Используется (E-005) |
| PowerShell (`Get-FileHash`, разбор PE-заголовка, поиск строк) | Манифест хешей, версии, строки | Простые операции без запуска Ghidra | Используется (E-001…E-014) |
| Wireshark/TShark 4.6.9 + Npcap 1.89 | Захват и разбор трафика оригинала (LAN/Steam), в т.ч. loopback | REA анализирует PCAP через TShark (`rea inspect-pcap-capture`, MCP `inspect_pcap_capture`), но сам трафик нужно записать | Установлены (E-050); переменные `REA_TSHARK_COMMAND`/`REA_CAPINFOS_COMMAND` — в `rea-env.ps1`, `.mcp.json`, `.codex/config.toml` |
| Frida (через REA) или собственный DLL-логгер | Запись тиков/RNG/событий оригинала | Статический анализ не даёт трасс поведения | Не установлен — T-017 |
| RenderDoc / PIX | Захват кадров D3D9 оригинала (порядок проходов, константы шейдеров) | REA не захватывает GPU-кадры | Не установлен — T-080. RenderDoc не поддерживает D3D9 — **проверить**; альтернатива PIX/apitrace (apitrace поддерживает D3D9) |
| Исследовательские Rust-утилиты в workspace (`tools/` бинарники) | Дамп пакетов/скриптов/шейдеров на основе тех же крейтов, что и runtime | Переиспользование кода парсеров | После T-031 |
