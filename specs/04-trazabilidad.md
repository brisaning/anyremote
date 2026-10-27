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

**PRs a `main`**: #2 (fases+ciclos+base M2) · #3 (M2 fix+v0.3.0) · #4 (v0.4.0) · #5 (specs dinámicos) · #6 (v0.5.0 entrada) · #7 (v0.6.0 audio) — `main` = `c5792b8`.

## Pendientes (estado actual)

1. Audio v0.6.0: prueba de campo en LAN, **audio del sistema (loopback WASAPI)**, VAD/silencio y resampler de mayor orden.
2. **Acuse de inyección al controlador** (hoy `input_injected`/`input_denied` solo los audita el anfitrión; el visor solo cuenta lo enviado).
3. Identidad persistente + trust por dispositivo (hoy: ID efímero, `peer_trusted` siempre false).
4. WAN (firewall/UPnP/NAT) y gates de CI (`clippy`/`fmt`).
5. Teclas F1..F12, Insert, PageUp/Down y bloqueos en la tabla de entrada.
6. Ver [README § Pendientes](../README.md#pendientes) para el panel completo.
