# Gate C-31 — escalado separable + tiempos por etapa (v0.13.4)

**Fecha**: 2026-11-17 · Rama: `perf/scale-separable` · Disparador: campo
Brian ↔ Julie con **LAN directa en ambos sentidos** (rtt 3–4 ms): Julie→Brian
es menos fluido que Brian→Julie aunque la red sea idéntica.

## Medición (logs 17/11, 15:30–15:33, 720p@15 en ambos anfitriones)

| | Brian cliente ← Julie host | Julie cliente ← Brian host |
|---|---|---|
| Emisión del anfitrión | **5–9 fps** (captura 14–45 por 5 s) | **0,8–5,2 fps** (captura 1–26 por 5 s) |
| fps render del cliente | mediana **~13** | mediana **~4,4** |
| frames pintados | 195 en 37 s | 141 en 64 s |

- Transporte y códec descartados: LAN directa (rtt 3–4 ms) y mismo 720p@15.
- El cliente pinta ≈ lo que produce el anfitrión → el cuello es el **bucle de
  vídeo del anfitrión** (`captura → escala → encode`, serie).
- Diferencia entre máquinas: Brian tiene escritorio **4K (3840×2160)**; Julie 1080p.
  El bucle captura a resolución nativa y **escala a 720p en CPU** con
  `scale_frame` (bilineal por píxel: 4 lecturas dispersas + 3 canales en `f32`,
  buffer nuevo por frame). De ahí la producción menor.

## Qué se implementa

| Pieza | Detalle |
|---|---|
| **C-31 escala** | `scale_frame` **separable** (horizontal a un buffer intermedio + vertical a la salida) con **pesos enteros 16.16**, lecturas de fila contiguas y sin `f32` por píxel. Misma semántica: bilineal, alfa = 255. |
| **C-31 diagnóstico** | El log de tasa añade tiempos por etapa: `· captura_ms=… escala_ms=… encode_ms=…` (promedio de la ventana; captura sobre frames nuevos, escala/encode por frame producido). |
| **Test** | `bilinear_scale_averages_2x2_block` (2×2 → 1×1 = media exacta 85, alfa 255); el test previo del gradiente 4×4→2×2 sigue en 42. |

## Gates (2026-11-17)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` | **51 suites / 0 fallos** |
| `cargo clippy --workspace --all-targets` | **0 / 0** |
| `cargo fmt --check` | **0** |
| `flutter analyze` / `flutter test` | limpio · **25/25** |

## Pendiente de validación de campo

- Repetir **Julie → Brian** y leer el log del anfitrión: debe verse
  `escala_ms` bajo y `captura_ms` como coste real; si `captura_ms` domina,
  el siguiente paso es readback 4K/GPU (DXGI video processor).
- Si la fluidez no mejora, comparar con el escritorio de Brian a 1920×1080
  (prueba de 2 minutos) para confirmar el coste del 4K.
