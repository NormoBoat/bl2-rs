# 09 — Сетевой протокол и совместимость

Требование: совместимость с протоколом **зафиксированной сборки** (`00-scope.md` §4). Собственный протокол «только между новыми клиентами» требование не выполняет.

## 1. Что известно (E-008, E-014, E-019)

| Факт | Источник | Статус |
|---|---|---|
| `NetworkDevice=OnlineSubsystemSteamworks.IpNetDriverSteamworks`, соединения `IpNetConnectionSteamworks` | `WillowGame/Config/DefaultEngine.ini` стр. 127–132 | VERIFIED (конфиг) |
| Базовый драйвер `IpDrv.TcpNetDriver` (в UE3 — UDP, несмотря на имя; HYPOTHESIS) | `Engine/Config/BaseEngine.ini` стр. 32–33, 426–443 | VERIFIED (конфиг) |
| `Port=7777` | `BaseEngine.ini` стр. 12 | VERIFIED (конфиг) |
| `[IpDrv.TcpNetDriver]`: ConnectionTimeout 30 (Willow: 30), InitialConnectTimeout 200 (Willow: 60), KeepAliveTime 0.2, MaxClientRate 15000, MaxInternetClientRate 10000, NetServerMaxTickRate 30, LanServerMaxTickRate 35 | `BaseEngine.ini` 426–443, `DefaultEngine.ini` 1265–1267 | VERIFIED (конфиг) |
| LAN-режим в UI: `Network_LAN=LAN`, меню `[FEMPLan]` Host/Join | `Localization/INT/WillowGame.int:868`, `WillowMenu.int:808–822` | VERIFIED (строки); работоспособность LAN без Steam — UNKNOWN |
| `LanQueryTimeout=2.0` | `DefaultEngine.ini` `[OnlineSubsystemSteamworks.OnlineGameInterfaceSteamworks]` | VERIFIED |
| Steam: `SteamAPI_Init`, `SteamNetworking`, `SteamMatchmaking`, `SteamGameServer`; 135 нативных `UOnlineSubsystemSteamworks` | строки exe | VERIFIED (строки); как используется — UNKNOWN |
| Steam QoS: `bEnableSteamQoS=true`, `[QoSHandlerSteamworks]` пробы | `DefaultSteamworks.ini` | VERIFIED |
| Seamless travel: `PostSeamlessTravel`, `HandleSeamlessTravelPlayer`, `GetSeamlessTravelActorList` | строки exe | VERIFIED (строки) |
| `bUseVAC=true` | `DefaultEngine.ini` | VERIFIED (конфиг) |

## 2. Режимы и их совместимость — разделять

| Режим | Транспорт (гипотеза) | Внешние сервисы | Статус для проекта |
|---|---|---|---|
| LAN | UDP напрямую, порт 7777 + широковещательный поиск (UE3 LAN beacon — HYPOTHESIS) | Нет (кроме возможной инициализации Steam оригиналом — проверить) | Реализуемо в рамках ограничений. **Основной путь проверок** |
| Онлайн через Steam | Steam P2P (`ISteamNetworking`) и/или Steam game server | Клиент Steam (установлен и запущен), аккаунт, лицензия BL2 (D-010) | Разрешено D-010 как опциональный режим; замена `steam_api` на Rust подтверждена до получения `SteamNetworking005` (E-055); обмен пакетами — T-095 |
| Прямое подключение по IP (`open <ip>`) | UDP | — | UNKNOWN, доступно ли в этой сборке (консоль?) |

Успешный LAN-тест **не** подтверждает онлайн-режим. Статусы в `status.md` ведутся раздельно.

## 3. Что нужно исследовать (каждое — задача с доказательствами)

1. **Обнаружение сессий**: LAN-поиск (формат запроса/ответа, порт, частота, `LanQueryTimeout`), содержание объявления сессии (имя хоста, миссия, уровень, кол-во игроков, версия). → T-094.
2. **Handshake и версия**: управляющие сообщения канала 0 (в UE3: Hello/Welcome/Upgrade/Challenge/Login/Join/Netspeed/Failure — HYPOTHESIS по UE3; licensee-изменения Gearbox — UNKNOWN), согласование версии (`EngineVersion`/`MinNetVersion`/package GUID-ы), список пакетов, которые должны совпадать у клиента и хоста (GUID пакетов — E-005 сохраняет GUID каждого пакета). → T-091.
3. **Транспорт**: формат пакета (битовый поток, номер пакета, ack/nak), bunch-и (канал, reliable, sequence, open/close, partial), фрагментация, надёжность и порядок. → T-090.
4. **Репликация**: каналы акторов, идентификация объектов (`NetIndex`/GUID), порядок и условия репликации свойств (из `replication`-блоков классов и `RepNotify`), сериализация типов (вектора с квантованием, ротаторы с компрессией, имена, ссылки на объекты), приоритеты/частоты (`NetUpdateFrequency`, `NetPriority`), relevancy. → T-092.
5. **RPC**: server/client/reliable функции, кодирование параметров. → T-093.
6. **Авторитет хоста и коррекция**: client-side prediction движения (`ServerMove`/`ClientAdjustPosition` в UE3 — HYPOTHESIS), допуски коррекции, временные метки. → T-092/T-053.
7. **Тайминг**: tick rate 30/35, NetSpeed, bandwidth limiting, timeouts, keepalive. → T-090.
8. **Переходы уровней**: seamless travel с клиентами, загрузка у клиентов, синхронизация. → T-069.
9. **Подключение к текущей игре и отключение**: join-in-progress, синхронизация состояния миссий/инвентаря, обработка ухода хоста. → T-096.
10. **Внешние сервисы**: что делает Steam в LAN-режиме; можно ли оригиналу играть LAN без входа в Steam. → T-018, T-095.

Методы: (а) захват трафика оригинал↔оригинал в LAN (M-3) с Wireshark/TShark + разбор через REA PCAP-анализ; (б) декомпиляция `UNetConnection`/`UChannel`/`UActorChannel`/`FBitReader`-эквивалентов в exe через REA; (в) дамп `replication`-условий из скриптовых пакетов (T-015); (г) активные эксперименты: собственный Rust-клиент-«зонд» отправляет минимальные сообщения оригинальному хосту и логирует ответы.

## 4. Реализация (bl2-net)

- Общая для трёх ОС: `std::net::UdpSocket` (нет зависимости, Ponytail) с неблокирующим режимом, опрос в системе `NetReceive`.
- Слои: пакет/битовый поток → каналы/bunch → управляющий канал → акторные каналы/репликация → RPC → игровой код (`bl2-sim`).
- Таблица реплицируемых свойств строится из тех же данных классов, что использует оригинал (порядок, условия) — без ручного дублирования там, где данные можно прочитать из пакетов.
- Headless-хост (`apps/bl2-headless`) — полноценный хост для проверок.

## 5. Проверка

Матрица (каждая ячейка — отдельный статус: «подключение», «сценарии», «нарушения сети»):

| # | Хост | Клиент | Режим |
|---|---|---|---|
| N-1 | новый | новый | LAN, каждая пара ОС (Win↔Win, Win↔Linux, Win↔macOS, Linux↔macOS) |
| N-2 | новый | оригинальный (Windows) | LAN |
| N-3 | оригинальный (Windows) | новый | LAN |
| N-4 | смешанная сессия: 4 игрока, ≥1 оригинал и ≥2 новых на разных ОС | LAN |
| N-5 | N-1…N-4 через Steam (клиент Steam запущен) | — | Steam |
| N-6 | Новая реализация **без** клиента Steam (не установлен или не запущен): онлайн-режим недоступен, LAN работает, ошибок нет | — | LAN |

Сценарии в каждой ячейке: подключение; движение; бой (урон, смерть, «Fight for Your Life», воскрешение); подбор/выброс/торговля предметами; общий прогресс миссий; переход между уровнями; fast travel; транспорт; join-in-progress; выход клиента/хоста; сохранение прогресса у всех участников; восстановление после задержки 200 мс, джиттера 50 мс, потерь 2 % и 10 %, разрыва на 5 с и 40 с (ожидаемое поведение — как у оригинала в тех же условиях, записанное заранее).

Средство внесения нарушений: на Linux — `tc netem`; на Windows — clumsy/WinDivert (инструмент исследования, не runtime) — выбор в T-096.
