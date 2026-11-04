# anyremote

[![CI](https://github.com/brisaning/anyremote/actions/workflows/ci.yml/badge.svg)](https://github.com/brisaning/anyremote/actions/workflows/ci.yml)

Aplicativo de **escritorio remoto** (controlador ↔ controlado) construido con **Organic Driven Development**: cada requisito creció desde una semilla validada (evidencia RE) en ciclos cortos con gate, evidencia y tag.

> **Estado del proyecto → sección [Estado](#estado)** · Specs vivos: [`specs/01-requerimientos.md`](specs/01-requerimientos.md) · [`specs/02-plan-implementacion.md`](specs/02-plan-implementacion.md) · [`specs/04-trazabilidad.md`](specs/04-trazabilidad.md) · Evidencia: [`evidence/`](evidence/) · Normas: [`AGENTS.md`](AGENTS.md)

## Estado

_Última actualización: 2026-10-26_

| Campo | Valor |
|---|---|
| Versión actual | **v0.12.3** (`anyremote_app.exe` + `anyremoted.exe` + `serveremote` en el repo hermano; [**release**](https://github.com/brisaning/anyremote/releases/tag/udp-rdv-defaults-v0.12.3) con el zip Windows) |
| Ramas | `main` = `5341935` (PRs #25–#37) · `develop` = `686c85b` (tags `quic-transport-v0.10.x`, `id-resolve-v0.11.x`, `rdv-relay-v0.12.0`, `resampler-v0.12.1`, `render-high-v0.12.2`, `udp-rdv-defaults-v0.12.3`) · **repo hermano: [serveremote](https://github.com/brisaning/serveremote)** (servidor WAN con Docker) |
| Gates actuales | **Rust 48 suites / 0 fallos** · **clippy 0 avisos** (`-D warnings`) · **fmt 0 diffs** · `flutter analyze` limpio · `flutter test` 9/9 · CI en `.github/workflows/ci.yml` |
| Validación de campo | ✅ LAN: sesión estable · portapapeles · visor · **control remoto teclado/ratón (v0.5.0)** · vídeo en pantalla estática fix v0.7.2 · sesión sana con `source=system` (107 frames + `bye`) · **✅ combo «control remoto + micrófono» verificada sin error con v0.7.3** (PR #13) · **✅ reconexión sin reiniciar la escucha tras cerrar desde el invitado (v0.8.2)** · 🟡 pendiente revalidación audio/720p/Nativa · visor/ID v0.8.3 · **modo visor v0.8.4** · **✅ cursor v0.8.5 (probado en campo)** · **✅ v0.9.1 en campo: `mf_h264` SIN fallback + adaptación por red actuando (3M→750k con rtt 40–1402 ms) + `dueño ausente` auto-detenido al cerrar la app** · **✅ QUIC en campo (2026-11-03, v0.10.1)**: dos sesiones con `--transport udp` en ambos lados, **cero `quic_fallback`**, auditoría `session_established.transport:"quic"` en ambas, vídeo `mf_h264` + control (499 inyecciones) + C-12 cerrando limpio sobre QUIC · **compatibilidad** (cliente TCP → listener dual) también probada en campo · **✅ conexión por ID en campo (2026-11-03, v0.11.2)**: dos sesiones tecleando solo `ONDH6IU4RU` — `resolve_ok` por **broadcast** (343 ms, primera vez) y por **caché** (37 ms), auditoría con `transport:"quic"` y `transport:"tcp"`, **pin por ID** pineado, sin fallback · tras dos hotfixes de campo (v0.11.1 fan-out de subredes, v0.11.2 anti-ICMP-poison del /32 WARP) | |
| PRs integrados | #2 (fases+ciclos+M2 base) · #3 (M2 fix + v0.3.0) · #4 (v0.4.0 UI) · #5 (specs dinámicos) · #6 (v0.5.0 entrada) · #7 (v0.6.0 audio) · #8 (v0.6.1 campo) · #9 (v0.6.2 coords) · #10 (v0.7.0 dinámico) · #11 (v0.7.1 perf) · #12 (v0.7.2 fix vídeo estático) · #13 (v0.7.3 diagnóstico) · #14 (v0.8.0 identidad) · #15 (v0.8.1 menores+CI) · #16 (docs+CI) · #17 (v0.8.2 listener repetidor) · #18 (v0.8.3 visor+ID) · #19 (v0.8.4 modo visor) · #20 (v0.8.5 cursor) · #21 (v0.8.6 dueño ausente) · #22 (v0.8.7 adaptación por red) · #23 (v0.9.0 H.264/MF) · #24 (v0.9.1 hotfix H.264) · #25 (v0.10.0 UDP/QUIC) · #26 (fmt e2e QUIC) · #27 (v0.10.1 auditoría transport) · #28 (v0.11.0 conexión por ID) · #29 (v0.11.1 fan-out broadcasts) · #30 (v0.11.2 fix ICMP-poison) · #31 (v0.12.0 WAN rendezvous+relay) · #32 (fix layout campo encuentro) · #33 (servidor → repo serveremote) · #34 (fix flake puertos e2e) · #35 (v0.12.1 resampler rubato) · #36 (v0.12.2 render alto sin nativo) · #37 (v0.12.3 defaults UDP+rdv + primer release) |

### Requisitos

| RF | Estado | Ciclo/evidencia |
|---|---|---|
| RF-01 Sesiones iniciar/aceptar | ✅ · **v0.8.2 listener repetidor**: al cerrar el invitado la escucha se libera y acepta reconexiones sin reiniciar; el proceso termina solo al detenerse (fix de cuelgue) · **v0.8.6 dueño ausente**: cerrar la app **detiene la escucha/sesión** (EOF del stdin con gestor visto) · **v0.10.0 transporte UDP/QUIC**: `--transport udp` (listen ata TCP+UDP en el mismo puerto; connect con fallback `quic_fallback`) — **vídeo por stream dedicado** (su pérdida no bloquea control/audio/heartbeats) y autenticación **C2 con el MISMO binding** exportado por QUIC | C1 (+C2) `evidence/listener-repeat-gate.md`, `evidence/owner-gate.md`, `evidence/quic-gate.md` |
| RF-02a Pantalla + códec | ✅ · **visor sin desbordes (v0.8.3)**: la pantalla del anfitrión se ve **completa** a cualquier tamaño de ventana · **modo visor (v0.8.4)**: Conexión/Registro ocultos, `Detener` en la barra superior a la izquierda del estado y toggles en la cabecera del visor · **cursor remoto visible (v0.8.5)**: flecha de overlay con la posición del cursor del anfitrión (DXGI no lo incluye en los frames) · **adaptación por red (v0.8.7)**: el RTT del controlador baja bitrate/escala en el anfitrión · **H.264 por hardware/MF (v0.9.0 · hotfix v0.9.1)**: encoder Media Foundation (con fallback VP9 **visible** en el registro) + decoder OpenH264 — menos CPU que VP9; el hotfix dimensiona el buffer de salida por `cbSize` y reasigna tasas por reconstrucción (fix de `MF_E_BUFFERTOOSMALL` a 720p) | C3/C3-bis (VP9 37 fps@720p) + M2 visor · `evidence/viewer-id-gate.md`, `evidence/viewer-mode-gate.md`, `evidence/cursor-gate.md` |
| RF-02a Entrada de teclado/ratón en sesión | ✅ (v0.5.0) — D-07 por defecto; coordenadas **normalizadas 0..65535** (v0.6.2); **toggles dinámicos** (v0.7.0); **acuse de inyección** al visor y **F1..F12** (v0.8.1) | `evidence/input-session-gate.md`, `evidence/control-coords-gate.md`, `evidence/dynamic-perms-gate.md`, `evidence/menores-gate.md` |
| Audio en sesión (capacidad D-05/D-07) | ✅ (v0.6.1 → **v0.7.0**): **loopback (audio del sistema)** + **micrófono** como fuentes (mezclables con `--audio-source system,mic`), Opus 48 kHz; toggles **dinámicos en caliente** · **v0.12.1**: conversión de tasa con **`rubato`** (windowed-sinc) en ambos sentidos — sin alias al bajar48→44.1 | `evidence/audio-gate.md`, `evidence/dynamic-perms-gate.md` |
| RF-02b Portapapeles | ✅ | C4 (contención de formatos) |
| RF-02c Archivos | ✅ | C5 + reanudación persistente (v0.3.0) |
| RF-03 Conexión por ID/dirección + auth | ✅ · **identidad persistente + trust TOFU (v0.8.0)** → mismo `NodeId` entre arranques y `peer_trusted=true` en la 2ª sesión · **ID visible + copiable y CLI `anyremoted id` (v0.8.3)** · **conexión por ID (v0.11.0 · hotfixes v0.11.1–v0.11.2)**: descubrimiento LAN sin servidor (`WHO/HERE` por UDP 21119 + caché): v0.11.1 abanica a las broadcasts de subred (`255.255.255.255` salía por la interfaz equivocada) y v0.11.2 elimina /31//32 de los destinos (el /32 de WARP generaba un unicast-poison que mataba la query con ICMP al instante) — se teclea el ID y la sesión verifica tras C2 que el par sea ese ID · **WAN (v0.12.0, D-04)**: servidor propio **`anyremote-rdv`** (registro ID+PK firmado, resolución ID→endpoint público, **relay por tickets** como respaldo sin reenvío) con `--rdv`/fichero `rdv` — las sesiones por relay siguen siendo **E2E** (TLS entre pares; el relay solo transporta cifrado) | C2 (ed25519 + binding) · `evidence/identity-gate.md`, `evidence/viewer-id-gate.md`, `evidence/id-connect-gate.md` |
| RF-04 Perfiles | ✅ | C6 + selector en UI (v0.4.0) |
| RF-05 Registro + diagnóstico | ✅ | C7 (JSONL + SessionReport) |
| RNF-01..06 | ✅ | C8 (secretos fail-closed, benchmark, auditabilidad) |

### Pendientes

1. **Audio**: **resampler ✅ v0.12.1** (`rubato` windowed-sinc en captura y reproducción, el lineal se retiró — `evidence/resampler-gate.md`); queda **re-verificar en campo el audio completo** (loopback + micrófono + toggles + gate de silencio con la cadena nueva).
2. **WAN ✅ verificado en campo (v0.12.0)**: servidor propio `serveremote` desplegado en el VPS del usuario (`51.222.81.81:21120`, Docker) — **2 sesiones reales por relay** (`resolve_ok` + `relay_fallback` + auditoría con `peer_addr` del VPS): registro, resolución, pipeline ciego E2E con C2, vídeo con C-13 adaptando a un enlace de 454 ms de RTT **sin morir** (guía: `evidence/rdv-relay-gate.md`). Con reenvío manual de21118 habría conexión directa.
3. **Rendimiento**: quick wins en v0.7.1 · **cursor visible ✅ v0.8.5** · **adaptación por red ✅ v0.8.7** (RTT del receptor → el host baja bitrate/escala con confirmación+cooldown); **aceleración por hardware ✅ v0.9.0** (H.264/MF con fallback VP9 + decoder OpenH264); **UDP/QUIC ✅ v0.10.0 (C-15)** — el backlog §7 queda **sin pendientes** (futuros: datagrams con mini-ARQ, VP8, libyuv, WAN).
4. **Render alto ✅ v0.12.2 (C-19, sin nativo)**: ensamblador de frames con buffer amortizado (antes: recopiar el buffer completo por chunk de TCP → **cuadrático**, ~200 copias por frame de 720p), visor con `ValueNotifier` (solo repinta el visor, no la página; cabecera a 2 Hz) y disposal de los recursos de decode por frame — `evidence/render-gate.md`. **`Texture` nativo queda CONDICIONADO a la medición de campo**: anotar el `fps render` de la cabecera a 720p/1080p/Nativa y pasar a nativo solo si no llega a los fps del stream.
5. Tabla de entrada: Insert, PageUp/Down y bloqueos (F1..F12 ✅ v0.8.1).

## Qué es

- **`core/`** (workspace Rust): `transport` (TLS 1.3 + ARSP + **QUIC** vía quinn), `identity` (ed25519/IDs), `session`, `capture` (DXGI), `codec` (VP9/Deflate/Raw), `input` (SendInput), `audio` (WASAPI + Opus 48 kHz), `clipboard`, `filexfer` (SHA-256 + reanudación), `profiles` (TOML), `telemetry` (JSONL), **`rendezvous`** (dep. git → repo **[serveremote](https://github.com/brisaning/serveremote)**: rendezvous + relay con Docker) y **`daemon`** (`anyremoted`: listen/connect/profiles + vídeo + audio + auditoría).
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
perfil (la conexión por ID se resuelve por descubrimiento LAN desde v0.11.0).

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
silencio, tope de bitrate, tuning libvpx) y de la **adaptación por red
automática (v0.8.7)** — el RTT que mide el controlador se devuelve al
anfitrión (`NetHint`) y, con 3 confirmaciones y 20 s de cooldown, este
baja bitrate o escala solo cuando la red está realmente congesta:

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
- **Códec (v0.9.0)**: la app usa **H.264/MF** por defecto (menos CPU que
  VP9); el registro lo confirma con `códec de vídeo: mf_h264` (o `vpx_vp9`
  cuando la máquina no tiene un MFT utilizable — fallback automático).

## Ingeniería inversa de referencia (semillas)

- **AnyDesk 9.8.2** (SHA256 `2a85e3bc…`) analizado con REA+Ghidra → binario **Themida/WinLicense**: superficie protegida, vacíos aceptados (`evidence/rea/`).
- **rustdesk `59f2953c`** (AGPL-3.0) indexado con codebase-memory → patrones H-RD-01..11 (`evidence/codebase-memory/`) — **clean-room D-01: sin copia de código**.
