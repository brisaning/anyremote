# Gate ciclo C3-bis — backend VP9 real (D-06)

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26 |
| Rama | `feature/c3-bis-vp9` (norma D-11) |
| Objetivo | Cerrar el gap de códec de C3 (Deflate=2 fps) con VP9 real y cumplir RNF-02 (≥30 fps @720p) |

## Decisiones de implementación

| Decisión | Detalle |
|---|---|
| **Binding** | `shiguredo_libvpx` 2026.2.0-canary.3 (Apache-2.0): binario libvpx **precompilado** descargado en build + bindings prefijados (`shiguredo_vpx_*`, sin colisiones). Elegido tras evaluar `env-libvpx-sys` (archivado; exige bindings exactos por versión), bindgen (sin libclang) y FFI manual. |
| **Vía A completada** | VS Build Tools 2022 (`C:\BuildTools`, MSVC 14.44 + `link.exe` + SDK 26100) + vcpkg + `libvpx:x64-windows` 1.16.0 instalados y verificados — quedan como base del build Flutter y como librería alternativa de sistema. |
| **Dedupe vcpkg** | Causa raíz resuelta: instances con igual versión `17.14.37824.6` se deduplicaban en vcpkg (SortedVector) sobreviviendo el parcial roto; workarounds: instancia legacy `VS170COMNTOOLS` (vía A final) y registro parcial pendiente de retiro elevado. |
| **GOP** | Keyframe inicial + cada **60 frames** (`force_keyframe`), `error_resilient`, CBR realtime, `cpu_used=6`, bitrate = área×3 bps. |
| **Lag** | Wrapper no admite `lag=0` (solo `NonZero`): usado **`lag=1`** → 1 frame de retardo (~33 ms) — aceptable para RF-02; documentado como límite del wrapper. |
| **Conversión color** | BGRA8↔I420 propia en `vpx_backend` con par BT.601 **rango completo coherente** (ida/vuelta ±3) — subsampling 4:2:0 inherent al VP9. |
| **Feature-gating** | Todo bajo feature `vpx` (`anyremote-codec`/`anyremote-transport`); el workspace compila y testea sin la feature (24 suites ok, 0 fallos). |

## Resultados de medición (release, 1280x720, 240 frames)

| Backend | Encode | Decode | Compresión | Integridad |
|---|---|---|---|---|
| **VP9** | **37 fps** ✅ (objetivo ≥30) | **52 fps** | **0.005x** (~200:1) | 240/240 geometría verificada |
| Deflate | 15 fps | 15 fps | 0.17x | byte a byte ✓ |
| Raw | 409 fps | — | 1.00x | byte a byte ✓ |

- **Debug vs release**: en debug la conversión BGRA→I420 domina (9 fps) — el gate RNF-02 se mide en **release** (modo de producción).
- Pipeline combinado (enc+dec misma máquina) = 22 fps; en despliegue real emisor/receptor son máquinas distintas y cada lado corre a sus tasas por lado (37/52).

## Tests

- `cargo test -p anyremote-codec --features vpx`: **6/6** (conversión plana ±3, lockstep VP9 89/90 con lag=1, roundtrips).
- `cargo test -p anyremote-transport --features vpx`: **10/10** incl. **`e2e_vp9_streaming_over_session`** (120 intentos → conteos coherentes enviado/recibido/decodificado, ≥100 decodificados, dims exactas) sobre TLS loopback ARSP.
- Suite default sin feature: **24 suites, 0 fallos**.
- Debug-run del bench: aserción ≥30 fps falla por diseño en debug (documentado; gate = release).

## Limitaciones / próximos pasos

- Flush de fin de sesión: con `lag=1` el último frame queda en el pipeline hasta `finish()` — añadir cierre de encoder en C7/C8.
- `vpx.dll`/vpx.lib del sistema (vcpkg 1.16) NO se usa en runtime (precompilado Shiguredo vinculado); mantener como alternativa documentada.
- Registro parcial de VS en `ProgramData` (bb5f3515) pendiente de retiro con elevación (no bloquea nada).
