# Matriz de cobertura funcional — Fase 2 (RF ↔ hallazgos ↔ estado)

| RF | Superficie requerida | Hallazgo H-AD | Estado | Fuente alternativa para Fase 3/4 |
|---|---|---|---|---|
| RF-01 Sesiones iniciar/aceptar | Flujos de conexión, handshake | H-AD-03, H-AD-08 | ❌ No observable (protección) | **H-RD**: `handshake()` (src/client.rs:5840), `identity_handshake()` (src/server.rs:264), `RendezvousMediator` + diseño propio |
| RF-02 Pantalla/entrada/portapapeles/archivos | Capas captura, códecs, FS | H-AD-02 (UI propia), manifiesto (screen sharing) | ⚠️ Solo declarado | **H-RD**: scrap/Encoder·Decoder, connection.file_transfer, libs/clipboard |
| RF-03 Conexión por ID/dirección + auth | Modelo de identificadores, pairing | H-AD-08(b) | ❌ Unknown | **H-RD**: pairing/rendezvous + IDs propios anyremote (L-01) |
| RF-04 Perfiles persistentes | Artefactos de configuración | H-AD-02b (asInvoker), H-AD-06 (sin config pre-EULA) | ⚠️ Acotado | **H-RD**: `libs/base/src/config/keys.rs` (KEYS_*/OPTION_*) |
| RF-05 Registro de sesiones + diagnóstico | Logs, artefactos de diagnóstico | — | ❌ Sin hallazgo | Diseño propio (gap explícito) |
| RNF-01 Seguridad | Cifrado E2E, auth mutua | H-AD-02b (asInvoker: sin UAC) | ⚠️ Parcial | Diseño propio + patrones handshake H-RD |
| RNF-02 Rendimiento | Latencia/FPS vs referencia | — | ❌ No medible (no se ejecutó sesión) | Medir en C3 contra objetivo declarado; benchmark RustDesk como referencia |
| RNF-03 Portabilidad | Plataformas | H-AD-02b (Win7–11, x86) | ✅ Observado | Windows primero (D-02) |
| RNF-05 Auditabilidad | Trazabilidad | Esta matriz + reporte | ✅ | — |

## Aceptación de vacíos (Fase 3, actividad 4)

Los vacíos H-AD-08(a–d) se aceptan como **límite del artefacto protegido**, no como fallo de análisis:
1. La ingeniería inversa de AnyDesk se agotó en todas las vías REA disponibles en este host (estática completa, dinámica no soportada en Windows).
2. El diseño de anyremote opera con **H-RD (patrones) + diseño propio**, cumpliendo D-01 clean-room y evitando cualquier dependencia de comportamiento propietario no observable.
3. R-03 (sesgo de versión) mitigado: baseline fija AnyDesk 9.8.2 + rustdesk commit `59f2953c`.

**Requiere**: aprobación del arquitecto (usuario) de esta aceptación de vacíos antes de la Fase 4.
