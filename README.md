# anyremote

Aplicativo de **escritorio remoto** (controlador ↔ controlado) construido con **Organic Driven Development**: cada requisito creció desde una semilla validada (evidencia RE) en ciclos cortos con gate, evidencia y tag.

> **Estado del proyecto → sección [Estado](#estado)** · Specs vivos: [`specs/01-requerimientos.md`](specs/01-requerimientos.md) · [`specs/02-plan-implementacion.md`](specs/02-plan-implementacion.md) · [`specs/04-trazabilidad.md`](specs/04-trazabilidad.md) · Evidencia: [`evidence/`](evidence/) · Normas: [`AGENTS.md`](AGENTS.md)

## Estado

_Última actualización: 2026-10-26_

| Campo | Valor |
|---|---|
| Versión actual | **v0.4.0** (`anyremote_app.exe` + `anyremoted.exe`) |
| Ramas | `main` = `97530d3` (PR #4) · `develop` = `1f30f13` (tag `ui-v0.4.0`) |
| Gates actuales | **Rust 36 suites / 0 fallos** · codec+`vpx` verde · `flutter analyze` limpio · `flutter test` 1/1 |
| Validación de campo | ✅ LAN: sesión estable · portapapeles bidireccional · **visor de pantalla remota** · cierre ordenado · reanudación (v0.3.0+) · resolución nativa + perfiles (v0.4.0) |
| PRs integrados | #2 (fases+ciclos+M2 base) · #3 (M2 fix + v0.3.0) · #4 (v0.4.0 UI) |

### Requisitos

| RF | Estado | Ciclo/evidencia |
|---|---|---|
| RF-01 Sesiones iniciar/aceptar | ✅ | C1 (+C2) `evidence/*` |
| RF-02a Pantalla + códec | ✅ | C3/C3-bis (VP9 37 fps@720p) + M2 visor |
| RF-02a Inyección de teclado/ratón | 🟡 core listo (`core/input`, SendInput) — **pendiente cablear en la sesión** | C3 |
| RF-02b Portapapeles | ✅ | C4 (contención de formatos) |
| RF-02c Archivos | ✅ | C5 + reanudación persistente (v0.3.0) |
| RF-03 Conexión por ID/dirección + auth | ✅ | C2 (ed25519 + channel binding + trust) |
| RF-04 Perfiles | ✅ | C6 + selector en UI (v0.4.0) |
| RF-05 Registro + diagnóstico | ✅ | C7 (JSONL + SessionReport) |
| RNF-01..06 | ✅ | C8 (secretos fail-closed, benchmark, auditabilidad) |

### Pendientes

1. **Audio en sesión** (micrófono/sistema + codec Opus — semilla H-RD `audio_capture`).
2. **Cablear `core/input` en la sesión**: hoy los permisos se conceden pero la sesión no transporta `InputEvent` — es lo que falta para el **control remoto real** de teclado/ratón.
3. **Identidad persistente + trust por dispositivo**: cada arranque genera un ID nuevo (`peer_trusted` siempre false) → persistir la clave en el directorio de config y confiar por ID conocido.
4. **WAN**: firewall/UPnP/NAT (hoy LAN + regla `netsh` manual documentada).
5. Gates de CI (`clippy`/`fmt`) y build reproducible.
6. Render a alta resolución → posible paso a widget `Texture` si el fps de render limita.

## Qué es

- **`core/`** (workspace Rust): `transport` (TLS 1.3 + ARSP), `identity` (ed25519/IDs), `session`, `capture` (DXGI), `codec` (VP9/Deflate/Raw), `input` (SendInput), `clipboard`, `filexfer` (SHA-256 + reanudación), `profiles` (TOML), `telemetry` (JSONL) y **`daemon`** (`anyremoted`: listen/connect/profiles + vídeo + auditoría).
- **`app/`** (Flutter desktop Windows): conexión con perfiles, visor remoto en vivo (fps de render), registro JSONL, *Escuchar (red)* autocontenido.

## Puesta en marcha

```powershell
# Anfitrión (comparte su pantalla — resolución nativa):
anyremoted listen --bind 0.0.0.0:21118 --video on --video-size native --clipboard win
# Invitado (o la app: destino IP:puerto o alias de perfil):
anyremote connect <ip>:21118 --clipboard win [--send-file archivo] [--one-way-files]
# Perfiles:
anyremote profiles --add casa,192.168.1.10,21118
```
Build: `cargo build --release -p anyremote-daemon --bin anyremoted` · `flutter build windows --release` (ambos artefactos se sirven desde `app/build/windows/x64/runner/Release/`).

## Ingeniería inversa de referencia (semillas)

- **AnyDesk 9.8.2** (SHA256 `2a85e3bc…`) analizado con REA+Ghidra → binario **Themida/WinLicense**: superficie protegida, vacíos aceptados (`evidence/rea/`).
- **rustdesk `59f2953c`** (AGPL-3.0) indexado con codebase-memory → patrones H-RD-01..11 (`evidence/codebase-memory/`) — **clean-room D-01: sin copia de código**.
