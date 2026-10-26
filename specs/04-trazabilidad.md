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
