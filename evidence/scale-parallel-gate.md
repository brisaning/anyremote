# Gate C-36 — escalado 4K→1080p paralelo (v0.13.9)

**Fecha**: 2026-11-18 · Rama: `feature/c36-scale-parallel` · Origen: validación de
campo de C-35 (18/11, 11:04–11:08, Julie 1080p ← Brian 3840×2160@125 %).

## Validación de campo de C-35 (lo que dijeron los logs)

| Dato | 10:04 (v0.13.7) | 11:05 (v0.13.8) |
|---|---|---|
| MFT | sin GPU (software) | **`NVIDIA H.264 Encoder MFT`** |
| `encode_ms` | 16,9–21,6 | **8,4–8,9** |
| tasa real (5 s) | 265–989 kbps | **1,3–5,6 Mbps** (bpp 0,19–0,46) |
| auto-reset del decoder | 3 en la sesión | **0** |
| bitrate adaptado | — | sube a 15 Mbps y `red_bitrate` baja a 7,5 / 3,75 Mbps (`rtt_min` 4 ms) |

Conclusión: el encoder de GPU hace lo que debía (tasa real, la mitad de latencia
de encode) y la desincronización del decoder desaparece con el rate-control.

**Nuevo cuello**: `escala_ms` **32–48 ms** (4K→1080p) frente a 8 ms de encode. El
escalado se había quedado serie y con dos buffers nuevos por frame (16,6 MB de
`scratch` + 8,3 MB de salida, con sus page faults).

## Cambio

`FrameScaler` en `core/daemon/src/video.rs`:

- **Tablas cacheadas** (`xtab`/`ytab`) mientras no cambien las dimensiones.
- **`scratch` reutilizado** entre frames (sin allocar 16,6 MB por frame).
- **Dos pasadas por bandas de filas con `std::thread::scope`** (hasta 8 hilos;
  serie por debajo de 480 filas, donde el `spawn` no compensa).
- `scale_frame()` se conserva como envoltorio de un solo uso (tests y sondas).

## Medición (release, 3840×2160 → …, 20 iteraciones)

| Destino | Serie | Paralelo | Mejora |
|---|---|---|---|
| 1920×1080 | 36,6 ms | **13,5 ms** | 2,7× |
| 1280×720 | 21,9 ms | **8,9 ms** | 2,5× |
| 1632×918 | 29,0 ms | **10,1 ms** | 2,9× |

La mejora está limitada por el ancho de banda de memoria (la pasada horizontal
lee los 33 MB del frame 4K). Con encode 8 ms, el frame completo queda en
**~22 ms** (presupuesto de 66 ms a 15 fps).

## Corrección

Test `parallel_scale_matches_reference`: el resultado paralelo es **byte a byte
idéntico** al de la versión serie (640×360 → 320×180, 427×240, 640×360, 160×90).
Sonda ignorada `probe_scale_cost` para repetir la medida.

## Gates

| Gate | Resultado |
|---|---|
| `cargo test --workspace` | **155 / 0** |
| `cargo clippy --workspace --all-targets` | **0 avisos** |
| `cargo fmt --all -- --check` | **0 diffs** |
| `flutter analyze` / `flutter test` | limpio · **27 / 27** |

## Pendiente

- Campo: `escala_ms` debe caer de 32–48 ms a ~13 ms en el log de 5 s.
- 4K asíncrono (D3D11) sigue pendiente de C-35; el 4K de hoy es síncrono.
