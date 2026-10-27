# anyremote

Aplicativo de **escritorio remoto** (controlador ↔ controlado) construido con **Organic Driven Development**: cada requisito creció desde una semilla validada (evidencia RE) en ciclos cortos con gate, evidencia y tag.

> **Estado del proyecto → sección [Estado](#estado)** · Specs vivos: [`specs/01-requerimientos.md`](specs/01-requerimientos.md) · [`specs/02-plan-implementacion.md`](specs/02-plan-implementacion.md) · [`specs/04-trazabilidad.md`](specs/04-trazabilidad.md) · Evidencia: [`evidence/`](evidence/) · Normas: [`AGENTS.md`](AGENTS.md)

## Estado

_Última actualización: 2026-10-26_

| Campo | Valor |
|---|---|
| Versión actual | **v0.6.1** (`anyremote_app.exe` + `anyremoted.exe`) |
| Ramas | `main` = `f4c9970` (PR #8) · `develop` = `921ef54` (tag `campo-v0.6.1`) |
| Gates actuales | **Rust 40 suites / 0 fallos** · codec+`vpx` verde · `flutter analyze` limpio · `flutter test` 3/3 |
| Validación de campo | ✅ LAN: sesión estable · portapapeles · visor · cierre/reanudación · perfiles (v0.4.0) · **control remoto teclado/ratón (v0.5.0)** · 🟡 **audio: diagnosticado en campo → loopback v0.6.1 (verificado local, pendiente LAN)** · lentitud diagnosticada → calidad 720p por defecto (v0.6.1) |
| PRs integrados | #2 (fases+ciclos+M2 base) · #3 (M2 fix + v0.3.0) · #4 (v0.4.0 UI) · #5 (specs dinámicos) · #6 (v0.5.0 entrada) · #7 (v0.6.0 audio) · **#8 (v0.6.1 campo)** |

### Requisitos

| RF | Estado | Ciclo/evidencia |
|---|---|---|
| RF-01 Sesiones iniciar/aceptar | ✅ | C1 (+C2) `evidence/*` |
| RF-02a Pantalla + códec | ✅ | C3/C3-bis (VP9 37 fps@720p) + M2 visor |
| RF-02a Entrada de teclado/ratón en sesión | ✅ (v0.5.0) — denegada por defecto (D-07, `--allow-input` / casilla "Aceptar control remoto") | `evidence/input-session-gate.md` |
| Audio en sesión (capacidad D-05/D-07) | ✅ (v0.6.0 → **v0.6.1**): **loopback (audio del sistema)** por defecto + micrófono opcional, Opus 48 kHz; opt-in con `--audio on` / casilla "Compartir audio" | `evidence/audio-gate.md` |
| RF-02b Portapapeles | ✅ | C4 (contención de formatos) |
| RF-02c Archivos | ✅ | C5 + reanudación persistente (v0.3.0) |
| RF-03 Conexión por ID/dirección + auth | ✅ | C2 (ed25519 + channel binding + trust) |
| RF-04 Perfiles | ✅ | C6 + selector en UI (v0.4.0) |
| RF-05 Registro + diagnóstico | ✅ | C7 (JSONL + SessionReport) |
| RNF-01..06 | ✅ | C8 (secretos fail-closed, benchmark, auditabilidad) |

### Pendientes

1. **Audio**: re-verificar en LAN con v0.6.1 (loopback); VAD/silencio y resampler de mayor orden.
2. **Acuse de inyección al visor**: hoy `input_injected`/`input_denied` solo los audita el anfitrión; el visor cuenta únicamente lo que envía.
3. **Identidad persistente + trust por dispositivo**: cada arranque genera un ID nuevo (`peer_trusted` siempre false) → persistir la clave en el directorio de config y confiar por ID conocido.
4. **WAN**: firewall/UPnP/NAT (hoy LAN + regla `netsh` manual documentada).
5. Gates de CI (`clippy`/`fmt`) y build reproducible.
6. Render a alta resolución → posible paso a widget `Texture` si el fps de render limita.
7. Tabla de entrada: teclas F1..F12, Insert, PageUp/Down, caps/num lock.

## Qué es

- **`core/`** (workspace Rust): `transport` (TLS 1.3 + ARSP), `identity` (ed25519/IDs), `session`, `capture` (DXGI), `codec` (VP9/Deflate/Raw), `input` (SendInput), `audio` (WASAPI + Opus 48 kHz), `clipboard`, `filexfer` (SHA-256 + reanudación), `profiles` (TOML), `telemetry` (JSONL) y **`daemon`** (`anyremoted`: listen/connect/profiles + vídeo + audio + auditoría).
- **`app/`** (Flutter desktop Windows): conexión con perfiles, visor remoto en vivo (fps de render) con ratón/teclado, registro JSONL, *Escuchar (red)* autocontenido (control remoto y audio opt-in).

## Puesta en marcha

```powershell
# Anfitrión (la app usa 720p por defecto; --audio on comparte el AUDIO DEL SISTEMA vía loopback):
anyremoted listen --bind 0.0.0.0:21118 --video on --video-size 1280x720 --clipboard win --allow-input --audio on
# ... --audio-source mic para compartir el micrófono en lugar de la salida del sistema.
# Invitado (o la app: destino IP:puerto o alias de perfil):
anyremote connect <ip>:21118 --clipboard win [--send-file archivo] [--one-way-files]
# Perfiles:
anyremote profiles --add casa,192.168.1.10,21118
```
En la app: marca **"Aceptar control remoto"** y/o **"Compartir audio"** antes de
*Escuchar (red)* (D-07: sin marcar, la entrada y el audio se deniegan) y elige la
**calidad de vídeo** (720p por defecto · 1080p · Nativa). El visor envía ratón
(mover/clic/arrastre/rueda) y teclado por stdin al proceso `connect`; el audio se
captura del **sistema (loopback)** y viaja **Opus 48 kHz mono** hasta el controlador
(indicador `audio ✓` / `audio: sin señal` en la cabecera del visor según el RMS).
Build: `cargo build --release -p anyremote-daemon --bin anyremoted` · `flutter build windows --release` (ambos artefactos se sirven desde `app/build/windows/x64/runner/Release/`).
**Requisito de build**: `opus` compila libopus con **CMake** — si hay que recompilar
(`cargo clean`, nueva máquina), añadir al `PATH` `C:\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin` (VS Build Tools).

## Ingeniería inversa de referencia (semillas)

- **AnyDesk 9.8.2** (SHA256 `2a85e3bc…`) analizado con REA+Ghidra → binario **Themida/WinLicense**: superficie protegida, vacíos aceptados (`evidence/rea/`).
- **rustdesk `59f2953c`** (AGPL-3.0) indexado con codebase-memory → patrones H-RD-01..11 (`evidence/codebase-memory/`) — **clean-room D-01: sin copia de código**.
