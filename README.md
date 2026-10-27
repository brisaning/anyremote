# anyremote

Aplicativo de **escritorio remoto** (controlador ↔ controlado) construido con **Organic Driven Development**: cada requisito creció desde una semilla validada (evidencia RE) en ciclos cortos con gate, evidencia y tag.

> **Estado del proyecto → sección [Estado](#estado)** · Specs vivos: [`specs/01-requerimientos.md`](specs/01-requerimientos.md) · [`specs/02-plan-implementacion.md`](specs/02-plan-implementacion.md) · [`specs/04-trazabilidad.md`](specs/04-trazabilidad.md) · Evidencia: [`evidence/`](evidence/) · Normas: [`AGENTS.md`](AGENTS.md)

## Estado

_Última actualización: 2026-10-26_

| Campo | Valor |
|---|---|
| Versión actual | **v0.7.3** (`anyremote_app.exe` + `anyremoted.exe`) |
| Ramas | `main` = `2230df1` (PR #13) · `develop` = `2239170` (tag `diag-host-v0.7.3`) |
| Gates actuales | **Rust 41 suites / 0 fallos** · codec+`vpx` verde · `flutter analyze` limpio · `flutter test` 4/4 |
| Validación de campo | ✅ LAN: sesión estable · portapapeles · visor · **control remoto teclado/ratón (v0.5.0)** · vídeo en pantalla estática fix v0.7.2 · **sesión sana verificada con `source=system` (107 frames + cierre limpio)** · 🔴 combo «control remoto + micrófono» (sin audio del sistema) → fuente `[mic]` sin dispositivo y la sesión no produce vídeo → **instrumentación v0.7.3** (`panic.log` + `ui.log`) para capturar la causa |
| PRs integrados | #2 (fases+ciclos+M2 base) · #3 (M2 fix + v0.3.0) · #4 (v0.4.0 UI) · #5 (specs dinámicos) · #6 (v0.5.0 entrada) · #7 (v0.6.0 audio) · #8 (v0.6.1 campo) · #9 (v0.6.2 coords) · #10 (v0.7.0 dinámico) · #11 (v0.7.1 perf) · #12 (v0.7.2 fix vídeo estático) · **#13 (v0.7.3 diagnóstico)** |

### Requisitos

| RF | Estado | Ciclo/evidencia |
|---|---|---|
| RF-01 Sesiones iniciar/aceptar | ✅ | C1 (+C2) `evidence/*` |
| RF-02a Pantalla + códec | ✅ | C3/C3-bis (VP9 37 fps@720p) + M2 visor |
| RF-02a Entrada de teclado/ratón en sesión | ✅ (v0.5.0) — denegada por defecto (D-07); coordenadas **normalizadas 0..65535** (v0.6.2) y **toggle dinámico en caliente** (v0.7.0) | `evidence/input-session-gate.md`, `evidence/control-coords-gate.md`, `evidence/dynamic-perms-gate.md` |
| Audio en sesión (capacidad D-05/D-07) | ✅ (v0.6.1 → **v0.7.0**): **loopback (audio del sistema)** + **micrófono** como fuentes (mezclables con `--audio-source system,mic`), Opus 48 kHz; toggles **dinámicos en caliente** | `evidence/audio-gate.md`, `evidence/dynamic-perms-gate.md` |
| RF-02b Portapapeles | ✅ | C4 (contención de formatos) |
| RF-02c Archivos | ✅ | C5 + reanudación persistente (v0.3.0) |
| RF-03 Conexión por ID/dirección + auth | ✅ | C2 (ed25519 + channel binding + trust) |
| RF-04 Perfiles | ✅ | C6 + selector en UI (v0.4.0) |
| RF-05 Registro + diagnóstico | ✅ | C7 (JSONL + SessionReport) |
| RNF-01..06 | ✅ | C8 (secretos fail-closed, benchmark, auditabilidad) |

### Pendientes

1. **Audio**: re-verificar en LAN (v0.7.x: loopback + micrófono + toggles + gate de silencio ya implementado); resampler de mayor orden.
2. **Acuse de inyección al visor**: hoy `input_injected`/`input_denied` solo los audita el anfitrión; el visor cuenta únicamente lo que envía.
3. **Identidad persistente + trust por dispositivo**: cada arranque genera un ID nuevo (`peer_trusted` siempre false) → persistir la clave en el directorio de config y confiar por ID conocido.
4. **WAN**: firewall/UPnP/NAT (hoy LAN + regla `netsh` manual documentada).
5. **Rendimiento (informe `evidence/perf-audit.md`)**: quick wins en v0.7.1 (frames sin cambios, gate de silencio, tope de bitrate, tuning libvpx); quedan **aceleración por hardware, adaptación por red, UDP/QUIC y cursor separado** (§5 del informe, requieren decisión de alcance).
6. Gates de CI (`clippy`/`fmt`) y build reproducible.
7. Render a alta resolución → posible paso a widget `Texture` si el fps de render limita.
8. Tabla de entrada: teclas F1..F12, Insert, PageUp/Down, caps/num lock.

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
En la app: las tres casillas **"Control remoto", "Audio del sistema" y
"Micrófono"** son **dinámicas** — se activan/desactivan **durante la sesión** y el
anfitrión las aplica en caliente (D-07 en caliente: manda quien comparte). La
**calidad de vídeo** (720p por defecto · 1080p · Nativa) se elige antes de
*Escuchar (red)*. El visor envía ratón (mover/clic/arrastre/rueda) y teclado por
stdin al proceso `connect`; el audio (sistema y/o micrófono, **mezclados** si
ambos) viaja **Opus 48 kHz mono** hasta el controlador (indicador `audio ✓` /
`audio: sin señal` en la cabecera del visor según el RMS).
Build: `cargo build --release -p anyremote-daemon --bin anyremoted` · `flutter build windows --release` (ambos artefactos se sirven desde `app/build/windows/x64/runner/Release/`).
**Requisito de build**: `opus` compila libopus con **CMake** — si hay que recompilar
(`cargo clean`, nueva máquina), añadir al `PATH` `C:\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin` (VS Build Tools).

## Diagnóstico de campo (v0.7.3)

Si una sesión falla, copia `%APPDATA%\anyremote\` (contiene `audit/`,
`panic.log` del daemon y `ui.log` de la app con los `args` exactos y el código
de salida). El anfitrión **no muestra su propia pantalla**: solo el registro (el
visor es exclusivo del controlador).

## Optimización de consumo (recomendaciones de campo)

Además de los quick wins del código (v0.7.1: frames sin cambios, gate de
silencio, tope de bitrate, tuning de libvpx — ver `evidence/perf-audit.md`):

- **En el anfitrión**: fondo de escritorio sólido, desactivar transparencias y
  animaciones de ventanas, cerrar navegadores/procesos pesados y desactivar el
  inicio automático de programas innecesarios.
- **En la sesión**: elegir **720p** (no Nativa) si la red va justa, `--audio off`
  si no hace falta oír el equipo remoto y `--clipboard off` si no se usa el
  portapapeles.
- **Red**: cable ethernet de preferencia (el RTT de 57-782 ms medido en v0.6.1
  era de Wi-Fi); `--max-bitrate <kbps>` para acotar el consumo.
- **Pantalla del anfitrión**: DXGI solo envía frames cuando algo cambia — en
  pantalla estática el visor refresca a ~1 fps (v0.7.2); interactuar en el
  anfitrión devuelve los fps completos.

## Ingeniería inversa de referencia (semillas)

- **AnyDesk 9.8.2** (SHA256 `2a85e3bc…`) analizado con REA+Ghidra → binario **Themida/WinLicense**: superficie protegida, vacíos aceptados (`evidence/rea/`).
- **rustdesk `59f2953c`** (AGPL-3.0) indexado con codebase-memory → patrones H-RD-01..11 (`evidence/codebase-memory/`) — **clean-room D-01: sin copia de código**.
