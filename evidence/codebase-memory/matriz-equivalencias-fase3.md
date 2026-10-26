# Matriz de equivalencias — Fase 3

*Comportamiento AnyDesk (H-AD)* ↔ *patrón RustDesk (H-RD)* ↔ *componente anyremote* ↔ *ciclo ODD*.

| RF | Comportamiento referencia (H-AD) | Patrón abierto (H-RD) | Componente anyremote | Ciclo | Estado |
|---|---|---|---|---|---|
| RF-01 Iniciar/aceptar sesión | H-AD-08(a) unknown (protegido) | H-RD-01 (handshake + rendezvous), H-RD-04 (límites) | `core/session`, `core/transport` | S0→C1 | ✅ patrón |
| RF-03 Conexión por ID/dirección | H-AD-08(b) unknown; H-AD-06 (proceso raíz+hijos) | H-RD-02 (id_pk + DTLS + registro PK), H-RD-03 (permisos), H-RD-04 | `core/identity`, `core/session` | C2 | ✅ patrón (IDs propios anyremote) |
| RF-02a Pantalla + entrada | H-AD-02 (UI propia), manifiesto "screen sharing" | H-RD-05 (captura→códec), H-RD-06 (entrada), H-RD-03 (permiso de control) | `core/capture`, `core/codec`, `core/input` | C3 | ✅ patrón |
| RF-02b Portapapeles | — (no observable) | H-RD-07 (cliprdr) | `core/clipboard` | C4 | ✅ patrón |
| RF-02c Archivos | — (no observable) | H-RD-08 (file_transfer + opciones) | `core/filexfer` | C5 | ✅ patrón |
| RF-04 Perfiles persistentes | H-AD-02b (asInvoker), H-AD-06 (sin config pre-EULA) | H-RD-09 (KEYS_*/OPTION_*) | `core/profiles` | C6 | ✅ patrón |
| RF-05 Registro + diagnóstico | — | H-RD-10 parcial | `core/telemetry` | C7 | ⚠️ gap aceptado → diseño propio |
| RNF-01 Seguridad | H-AD-02b (asInvoker) | H-RD-02 (DTLS+PK), H-RD-03, H-RD-04 | transversal | C2/C8 | ✅ patrón + endurecimiento C8 |
| RNF-02 Rendimiento | no medible | H-RD-05 (benchmark scrap) | `core/codec` | C3/C8 | ⚠️ medir vs objetivo propio |
| RNF-03 Portabilidad | H-AD-02b (Win7–11, x86) | H-RD-05/06/07 (backends por SO) | workspace | todo | ✅ |

## Aceptación de vacíos (registrada)

- Aceptada por el arquitecto (usuario) el 2026-10-26 al autorizar "Continúa con las siguientes fases" tras la presentación de `matriz-cobertura-fase2.md`.
- Implicación: RF-01/RF-03/RF-05 y endpoints de red se diseñan sobre patrones H-RD + diseño propio; ninguna decisión depende de comportamiento propietario no observable de AnyDesk (consistente con D-01).
