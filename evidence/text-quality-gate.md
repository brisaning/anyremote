# Gate C-32 — legibilidad de texto: presets de bitrate (v0.13.5)

**Fecha**: 2026-11-17 · Rama: `fix/text-bitrate` · Disparador: campo (Brian anfitrión,
Julie cliente 1080p): **texto ilegible en las tres calidades** (720p, 1080p, nativa).

## Medición (logs 17/11, con los tiempos por etapa de C-31)

| Prueba | `encode_ms` | `escala_ms` | `captura_ms` | fps host | fps render Julie |
|---|---|---|---|---|---|
| **720p @ 3 000 kbps** | **6–9** | 9–35 | 23–335 | 0,4–9,4 | 5,8–20 |
| **1080p @ 6 000 kbps** | **14–23** | 15–52 | 51–349 | 0,4–6,8 | 4–8 |
| **Nativa 4K @ 8 000 kbps** | **70–394** | 15–30 | 40–240 | 0,8–5,2 | 1,8–7,8 |

- `escala_ms` (9–52 ms) y `encode_ms` a 720p/1080p (6–23 ms) descartan el
  escalado y el códec como cuello de la nitidez; `captura_ms` alto es la
  **espera a que cambie la pantalla** (DXGI), no coste de calidad.
- El problema es **presupuesto de bits** (bits por píxel y frame):
  - 720p@15 (13,8 Mpx/s) a 3 Mbps → **0,22** bpp/frame
  - 1080p@15 (31,1 Mpx/s) a 6 Mbps → **0,19** bpp/frame
  - 4K@15 (124,4 Mpx/s) a 8 Mbps → **0,064** bpp/frame
  El texto de escritorio necesita ~0,4–0,5 bpp/frame (más en 4K).
- Además, con anfitrión 4K y cliente 1080p, 720p/1080p **reducen el texto** a
  1/3 y 1/2 de tamaño: para leer a 1:1 hay que emitir 1080p y ampliar la
  ventana del visor (o bajar el escritorio del anfitrión a 1080p).
- 4K sigue limitado por `encode_ms` (70–394 ms > 66 ms de presupuesto a 15 fps):
  no es viable a 15 fps en este anfitrión; 1080p es el punto dulce.

## Qué se implementa

| Pieza | Detalle |
|---|---|
| **Presets** | `720p: 3 000 → 6 000` · `1080p: 6 000 → 15 000` · `Nativa: 8 000 → 25 000` kbps. El tope solo fija el techo: en WAN el adaptativo sigue bajando; en LAN/relay con margen, la recuperación sube hasta él. |
| **Nota** | Documentado en el código y en README § Pendientes/uso: para texto en LAN, 1080p + 15 Mbps y visor maximizado (1:1). |

## Gates (2026-11-17)

| Gate | Resultado |
|---|---|
| `flutter analyze` / `flutter test` | limpio · **25/25** |
| Rust (sin cambios) | 51/0 (último gate) |

## Pendiente de validación de campo

- Repetir **Julie → Brian** con **1080p** y leer el texto con el visor maximizado:
  debe ser legible. En 4K el texto se ve a tamaño completo pero el bitrate y el
  encode aún lo limitan; en 720p el texto sigue reducido a 1/3.
- Si con 15 Mbps el texto aún no es nítido, subir 1080p a 20–25 Mbps (LAN
  directa lo soporta) o bajar el escritorio del anfitrión a 1920×1080.
