# anyremote

[![CI](https://github.com/brisaning/anyremote/actions/workflows/ci.yml/badge.svg)](https://github.com/brisaning/anyremote/actions/workflows/ci.yml)

Aplicativo de **escritorio remoto** (controlador ↔ controlado) construido con **Organic Driven Development**: cada requisito creció desde una semilla validada (evidencia RE) en ciclos cortos con gate, evidencia y tag.

> **Estado del proyecto → sección [Estado](#estado)** · Specs vivos: [`specs/01-requerimientos.md`](specs/01-requerimientos.md) · [`specs/02-plan-implementacion.md`](specs/02-plan-implementacion.md) · [`specs/04-trazabilidad.md`](specs/04-trazabilidad.md) · Evidencia: [`evidence/`](evidence/) · Normas: [`AGENTS.md`](AGENTS.md)

## Estado

_Última actualización: 2026-10-26_

| Campo | Valor |
|---|---|
| Versión actual | **v0.8.5** (`anyremote_app.exe` + `anyremoted.exe`) |
| Ramas | `main` = `75aae72` (PR #19) · `develop` = `f6c972f` (tag `visor-mode-v0.8.4`) |
| Gates actuales | **Rust 45 suites / 0 fallos** · **clippy 0 avisos** (`-D warnings`) · **fmt 0 diffs** · `flutter analyze` limpio · `flutter test` 9/9 · CI en `.github/workflows/ci.yml` |
| Validación de campo | ✅ LAN: sesión estable · portapapeles · visor · **control remoto teclado/ratón (v0.5.0)** · vídeo en pantalla estática fix v0.7.2 · sesión sana con `source=system` (107 frames + `bye`) · **✅ combo «control remoto + micrófono» verificada sin error con v0.7.3** (PR #13) · **✅ reconexión sin reiniciar la escucha tras cerrar desde el invitado (v0.8.2)** · 🟡 pendiente revalidación audio/720p/Nativa · visor/ID v0.8.3 · **modo visor v0.8.4** |
| PRs integrados | #2 (fases+ciclos+M2 base) · #3 (M2 fix + v0.3.0) · #4 (v0.4.0 UI) · #5 (specs dinámicos) · #6 (v0.5.0 entrada) · #7 (v0.6.0 audio) · #8 (v0.6.1 campo) · #9 (v0.6.2 coords) · #10 (v0.7.0 dinámico) · #11 (v0.7.1 perf) · #12 (v0.7.2 fix vídeo estático) · #13 (v0.7.3 diagnóstico) · #14 (v0.8.0 identidad) · #15 (v0.8.1 menores+CI) · #16 (docs+CI) · #17 (v0.8.2 listener repetidor) · #18 (v0.8.3 visor+ID) · **#19 (v0.8.4 modo visor)** |

### Requisitos

| RF | Estado | Ciclo/evidencia |
|---|---|---|
| RF-01 Sesiones iniciar/aceptar | ✅ · **v0.8.2 listener repetidor**: al cerrar el invitado la escucha se libera y acepta reconexiones sin reiniciar; el proceso termina solo al detenerse (fix de cuelgue) | C1 (+C2) `evidence/listener-repeat-gate.md` |
| RF-02a Pantalla + códec | ✅ · **visor sin desbordes (v0.8.3)**: la pantalla del anfitrión se ve **completa** a cualquier tamaño de ventana · **modo visor (v0.8.4)**: Conexión/Registro ocultos, `Detener` en la barra superior a la izquierda del estado y toggles en la cabecera del visor · **cursor remoto visible (v0.8.5)**: flecha de overlay con la posición del cursor del anfitrión (DXGI no lo incluye en los frames) | C3/C3-bis (VP9 37 fps@720p) + M2 visor · `evidence/viewer-id-gate.md`, `evidence/viewer-mode-gate.md`, `evidence/cursor-gate.md` |
| RF-02a Entrada de teclado/ratón en sesión | ✅ (v0.5.0) — D-07 por defecto; coordenadas **normalizadas 0..65535** (v0.6.2); **toggles dinámicos** (v0.7.0); **acuse de inyección** al visor y **F1..F12** (v0.8.1) | `evidence/input-session-gate.md`, `evidence/control-coords-gate.md`, `evidence/dynamic-perms-gate.md`, `evidence/menores-gate.md` |
| Audio en sesión (capacidad D-05/D-07) | ✅ (v0.6.1 → **v0.7.0**): **loopback (audio del sistema)** + **micrófono** como fuentes (mezclables con `--audio-source system,mic`), Opus 48 kHz; toggles **dinámicos en caliente** | `evidence/audio-gate.md`, `evidence/dynamic-perms-gate.md` |
| RF-02b Portapapeles | ✅ | C4 (contención de formatos) |
| RF-02c Archivos | ✅ | C5 + reanudación persistente (v0.3.0) |
| RF-03 Conexión por ID/dirección + auth | ✅ · **identidad persistente + trust TOFU (v0.8.0)** → mismo `NodeId` entre arranques y `peer_trusted=true` en la 2ª sesión · **ID visible + copiable y CLI `anyremoted id` (v0.8.3)** (conectar por ID aún pendiente de resolutor) | C2 (ed25519 + binding) · `evidence/identity-gate.md`, `evidence/viewer-id-gate.md` |
| RF-04 Perfiles | ✅ | C6 + selector en UI (v0.4.0) |
| RF-05 Registro + diagnóstico | ✅ | C7 (JSONL + SessionReport) |
| RNF-01..06 | ✅ | C8 (secretos fail-closed, benchmark, auditabilidad) |

### Pendientes

1. **Audio**: re-verificar en LAN (v0.7.x: loopback + micrófono + toggles + gate de silencio ya implementados); **resampler de mayor orden** (único menor: plan definido — `rubato`, esfuerzo M, pendiente de que el campo lo pida).
2. **WAN**: firewall/UPnP/NAT (hoy LAN + regla `netsh` manual documentada).
3. **Rendimiento**: quick wins aplicados en v0.7.1 (frames sin cambios, gate de silencio, tope de bitrate, tuning libvpx); el backlog de **aceleración por hardware, adaptación por red, UDP/QUIC y cursor separado** está en **`specs/02` §7** (requieren decisión de alcance).
4. Render a alta resolución → posible paso a widget `Texture` si el fps de render limita.
5. Tabla de entrada: Insert, PageUp/Down y bloqueos (F1..F12 ✅ v0.8.1).

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
# ID estable de este equipo (v0.8.3):
anyremote id
```
En la app: las tres casillas **"Control remoto", "Audio del sistema" y
"Micrófono"** son **dinámicas** — se activan/desactivan **durante la sesión** y el
anfitrión las aplica en caliente (D-07 en caliente: manda quien comparte). La
**calidad de vídeo** (720p por defecto · 1080p · Nativa) se elige antes de
*Escuchar (red)*. El visor envía ratón (mover/clic/arrastre/rueda) y teclado por
stdin al proceso `connect`; el audio (sistema y/o micrófono, **mezclados** si
ambos) viaja **Opus 48 kHz mono** hasta el controlador (indicador `audio ✓` /
`audio: sin señal` en la cabecera del visor según el RMS).

**IDs de equipo (v0.8.3)**: el ID es el *nombre* del equipo (Base32 de 10
caracteres, estable — `anyremote id` o la fila **«ID de este equipo»** con
botón de copiar en la tarjeta Conexión). El ID **del anfitrión (remoto)**
se ve en SU equipo en esa misma fila y, durante la sesión, en **«SESIÓN
ACTIVA con …»** (con `✓ verificado` si ya se autenticó antes). Hoy el ID
**identifica pero no resuelve**: la conexión sigue siendo `ip:puerto` o
perfil (el conector por ID queda pendiente de un resolutor).

**Modo visor (v0.8.4)**: mientras el visor remoto está visible, las
secciones **Conexión** y **Registro** quedan ocultas (el visor ocupa la
ventana) y el botón **Detener** vive en la barra superior, justo a la
izquierda del estado. Los tres toggles dinámicos (Control remoto · Audio
del sistema · Micrófono) se muestran en la **cabecera del visor** y
siguen actuando en caliente; al desconectar, todo vuelve a la vista.

**Cursor remoto (v0.8.5)**: el visor dibuja una flecha con la posición
del cursor del anfitrión (DXGI no lo captura). Se normaliza **igual que
los clics** (0..65535 del monitor primario) → aparece exactamente donde
caen; viaja **solo en cambios** (sondeo a 50 ms) y solo se dibuja con
visor visible.
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
