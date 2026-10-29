# Matriz de trazabilidad completa — RF ↔ hallazgos ↔ componente ↔ prueba (Fase 4)

| RF | H-AD | H-RD | Componente | Prueba de gate | Ciclo |
|---|---|---|---|---|---|
| RF-01 Iniciar/aceptar sesión | H-AD-08(a)* | H-RD-01, H-RD-04, H-RD-11 | `core/session`, `core/transport` | `e2e_session_loopback` | S0, C1 |
| RF-03 Conexión por ID/dirección + auth | H-AD-08(b)*, H-AD-06 | H-RD-02, H-RD-03, H-RD-04 | `core/identity`, `core/session` | `e2e_auth_mutua`, `t_rechazo_firma_invalida`, `t_límite_intentos` | C2 |
| RF-02a Pantalla + entrada | H-AD-02 | H-RD-05, H-RD-06, H-RD-03 | `core/capture`, `core/codec`, `core/input` | `e2e_video_loopback`, `bench_fps_latencia` | C3 |
| RF-02b Portapapeles | — | H-RD-07 | `core/clipboard` | `e2e_clipboard` | C4 |
| RF-02c Archivos | — | H-RD-08 | `core/filexfer` | `e2e_filexfer_hash` | C5 |
| RF-04 Perfiles | H-AD-02b, H-AD-06 | H-RD-09 | `core/profiles` | `e2e_perfil_aplicado` | C6 |
| RF-05 Registro + diagnóstico | — (gap) | H-RD-10 (parcial) | `core/telemetry` | `e2e_auditoría_eventos` | C7 |
| RNF-01 Seguridad | H-AD-02b | H-RD-02/03/04 | transversal | `t_seguridad_*` (sección 6 de arquitectura) | C2, C8 |
| RNF-02 Rendimiento | — | H-RD-05 | `core/codec` | `bench_fps_latencia` vs objetivo | C3, C8 |
| RNF-03 Portabilidad | H-AD-02b | H-RD-05/06/07 | workspace | builds Windows (+ backends futuros) | todo |
| RNF-05 Auditabilidad | — | — | `evidence/` + vault | check-list CA por fase | todo |

`*` = vacío de superficie aceptado por el arquitecto (2026-10-26); traza a patrón abierto + diseño propio (D-03/D-04).

## Reglas de mantenimiento

1. Un hallazgo nuevo (H-AD/H-RD) ⇒ fila actualizada aquí antes de su uso en un ciclo.
2. Un componente nuevo ⇒ su prueba de gate definida en el mismo PR (D-07 pruebas por ciclo).
3. Cada gate cerrado ⇒ evidencia del test + tag del ciclo en git; estado reflejado en la bóveda (`Memoria del proyecto.md`).

## Estado de ciclos (2026-10-26)

| Ciclo | RF | Gate | Tag | Evidencia |
|---|---|---|---|---|
| S0 Tronco | — | 9 tests + demo TLS loopback | `ciclo-s0` | — |
| C1 | RF-01 | `e2e_session_establishment_with_metrics` | `ciclo-c1` | — |
| C2 | RF-03/RNF-01 | `e2e_mutual_auth_with_trust_store` + rechazos + límite | `ciclo-c2` | — |
| C3 | RF-02a | DXGI real + e2e vídeo + bench | `ciclo-c3` | `evidence/c3-gate.md` |
| C3-bis | RNF-02 | VP9 release ≥30 fps + e2e VP9 | `ciclo-c3-bis` | `evidence/c3-bis-gate.md` |
| C4 | RF-02b | `e2e_clipboard` (bidireccional+contención) | `ciclo-c4` | `evidence/c4-gate.md` |
| C5 | RF-02c | `e2e_filexfer` (hash+reanudación+límites) | `ciclo-c5` | `evidence/c5-gate.md` |
| C6 | RF-04 | `profile_applies_to_a_new_session` (6/6) | `ciclo-c6` | `evidence/c6-gate.md` |
| C7 | RF-05 | `e2e_audit_events_and_diagnostic_report` | `ciclo-c7` | `evidence/c7-gate.md` |
| C8 | RNF-01/02/05 | auditoría de secretos + benchmark final + checklist | `ciclo-c8` | `evidence/c8-checklist.md` |
| Integración v0.1.x | daemon↔app | e2e daemon + fixes cross-device (pins, listener resiliente, recv cancellation-safe, portapapeles no fatal) | `integracion-v0.1.3`…`v0.1.6` | `evidence/integration-gate.md` |
| Hito M2 | RF-02a (visor) | captura→VP9→sesión→preview→render (validado en campo) | `m2-video-v0.2.0` | `evidence/m2-video-gate.md` |
| Lote calidad v0.3.0 | robustez + RNF-02 | bye gracioso, `one_way`, `FileReady`+persistencia, bilineal, adaptativo | `calidad-v0.3.0` · PR #3 | `evidence/v030-quality-batch-gate.md` |
| Lote UI v0.4.0 | RF-04 (UI) + resolución nativa | dropdown de perfiles + `--video-size native` | `ui-v0.4.0` · PR #4 | `evidence/ui-batch-gate.md` |
| Entrada en sesión v0.5.0 | RF-02a (entrada) | wire `Key.ch` + mapeo owner en transport · `--allow-input` (D-07) · permiso por evento (`keyboard`/`mouse`) · visor con ratón (clic/arrastre/rueda) y teclado | `entrada-v0.5.0` | `evidence/input-session-gate.md` |
| Audio en sesión v0.6.0 | Capacidad D-05/D-07 | crate `audio` (WASAPI+Opus 48 kHz mono, modo `Test`) · `AudioFrame` validado en transport · permiso `audio` opt-in (`--audio on` / casilla "Compartir audio") · `audio_stats` + `SessionReport.audio_*` | `audio-v0.6.0` | `evidence/audio-gate.md` |
| Ajustes de campo v0.6.1 | Audio + rendimiento | **loopback (audio del sistema)** como fuente por defecto (`--audio-source system\|mic`) · `audio_stats.rms` + indicador en el visor · calidad de vídeo 720p por defecto (nativo daba 4.6 fps y RTT 57–782 ms) | `campo-v0.6.1` | `evidence/audio-gate.md` §Diagnóstico |
| Fix control 720p/1080p v0.6.2 | RF-02a (entrada) | coordenadas **absolutas normalizadas 0..65535** (app → wire → `SendInput` sin normalizar por pantalla) | `coords-v0.6.2` | `evidence/control-coords-gate.md` |
| C-2 permisos dinámicos v0.7.0 | RF-02a + audio | tres toggles en caliente por stdin (`{"perm":...}`) · fuente **micrófono** + **mezcla** sistema+mic · drenaje sin carrera inicial · reproductor del controlador siempre preparado | `perm-dinamicas-v0.7.0` | `evidence/dynamic-perms-gate.md` |
| C-3 rendimiento v0.7.1 | RNF (eficiencia) | QW-1 frames sin cambios (DXGI) · QW-2 gate de silencio + DTX/48 kbps · QW-3 `--max-bitrate` (app: 3/6/8 Mbps) · QW-4 libvpx (fps real, hilos, Screen) · informe RustDesk vs nosotros | `perf-v0.7.1` | `evidence/perf-audit.md` |
| C-4 fix vídeo estático v0.7.2 | Campo (sin vídeo en LAN) | captura/encode en **hilo propio** (los bloqueos de 500 ms de DXGI ya no estrangulan el bucle de sesión) + **reenvío del último frame a 1 fps** en pantalla estática | `video-estatico-v0.7.2` | `evidence/video-static-gate.md` |
| C-5 diagnóstico anfitrión v0.7.3 | Campo (combo «control remoto+micrófono» sin vídeo) | `panic.log` (hook global), `ui.log` persistente de la app con `args`/código de salida, **un solo spawn de captura por sesión**, **el anfitrión no muestra su propia pantalla** | `diag-host-v0.7.3` | `evidence/host-debug-gate.md` |
| C-6 identidad + trust v0.8.0 | RF-03 (D-03) | `identity.key` persistente (mismo `NodeId` entre arranques) · `trusted.ids` con **TOFU** (`peer_trusted=true` en la 2ª sesión, ambos roles) · `session_active` con `peer_trusted` visible en la app | `identidad-v0.8.0` | `evidence/identity-gate.md` |
| C-7 menores + CI v0.8.1 | RF-02a + RNF | **acuse `InputAck`** (el visor cuenta inyectadas y ve rechazos) · **F1..F12** (30..41) · clippy a 0 + rustfmt del workspace + workflow CI (fmt/clippy/test en windows-latest) | `menores-v0.8.1` | `evidence/menores-gate.md` |
| C-8 listener repetidor v0.8.2 | RF-01 (reporte de campo) | **`--max-sessions 0`**: el `bye` de RED del invitado libera la escucha y se vuelve a aceptar · **fix del cuelgue** (lector de stdin en hilo propio: el proceso termina solo) · `bye` por stdin detiene la escucha también sin sesión · estado vuelve a «Escuchando (red)» · resumen sin `Nonems` | `repeat-v0.8.2` | `evidence/listener-repeat-gate.md` |
| C-9 visor completo + ID visible v0.8.3 | RF-02a pantalla + RF-03 (2 reportes de campo) | Visor: tarjeta `Expanded` + marco `Center(AspectRatio)` → **sin desbordes** a cualquier tamaño (regresión de widget test con **control negativo verificado**) · ID: subcomanda **`anyremoted id`** (mismo `identity.key` que `listen`), fila **«ID de este equipo»** con copiar, refresco desde `listening` y aviso al escribir un ID en el campo de destino | `visor-id-v0.8.3` | `evidence/viewer-id-gate.md` |
| C-10 modo visor (cliente) v0.8.4 | RF-02a pantalla (UX de campo) | `viewerVisible`: **Conexión y Registro ocultos** con el visor visible · **`Detener` en `AppBar` a la izquierda del estado** · toggles C-2 en la **cabecera del visor** (helper `_permToggles`, única copia) · las secciones **vuelven** al cerrar (`closed` + `onDone`/`onError` del preview) | `visor-mode-v0.8.4` | `evidence/viewer-mode-gate.md` |
| C-11 cursor remoto visible (overlay) v0.8.5 | RF-02a pantalla (decisión de backlog) | Hilo de sondeo 50 ms con `GetCursorPos`/`GetCursorInfo` → **solo cambios** por `watch` · wire `CursorState` (campo 25, 0..65535 = métrica del inyector → el overlay cae donde los clics) · controlador emite `cursor` **sin auditoría** · visor: `Stack` + flecha `_RemoteCursorPainter` (dibujo propio) | `cursor-v0.8.5` | `evidence/cursor-gate.md` |

**PRs a `main`**: #2 (fases+ciclos+base M2) · #3 (M2 fix+v0.3.0) · #4 (v0.4.0) · #5 (specs dinámicos) · #6 (v0.5.0 entrada) · #7 (v0.6.0 audio) · #8 (v0.6.1 campo) · #9 (coords) · #10 (dinámico) · #11 (perf) · #12 (fix vídeo estático) · #13 (diagnóstico) · #14 (identidad) · #15 (menores+CI) · #16 (docs+CI) · #17 (v0.8.2 listener repetidor) · #18 (v0.8.3 visor+ID) · #19 (v0.8.4 modo visor) · #20 (v0.8.5 cursor) — `main` = `ad2c5a4`.

## Pendientes (estado actual)

1. Audio: re-validar en LAN la cadena completa con v0.7.3+ (loopback + micrófono + toggles ya verificados por separado); resampler de mayor orden.
2. ~~Acuse de inyección al controlador~~ ✅ resuelto en v0.8.1 (acuse `InputAck` por evento + contador real de inyectadas en la app).
3. ~~Identidad persistente + trust por dispositivo~~ ✅ resuelto en v0.8.0 (C-6); el ID ya es visible/copiable (v0.8.3, C-9) — queda el uso futuro: **conectar** por ID estable (resolutor).
4. WAN (firewall/UPnP/NAT) — los gates de CI quedaron ✅ en v0.8.1 (`.github/workflows/ci.yml`).
5. ~~Teclas F1..F12~~ ✅ resueltas en v0.8.1 (códigos 30..41); quedan Insert, PageUp/Down y bloqueos.
6. Ver [README § Pendientes](../README.md#pendientes) para el panel completo.
