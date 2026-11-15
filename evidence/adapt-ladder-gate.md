# Gate C-29 — adaptación: recuperación alcanzable + escalera acotada (v0.13.2)

**Fecha**: 2026-11-11 · **Rama**: `feature/adapt-ladder` · Disparador: pruebas del usuario (10 Mbps WAN /150 Mbps LAN): «en WAN el vídeo se degrada con el tiempo hasta que el texto deja de ser legible; el movimiento sigue fluido». **A1+A2 implementados; el BUG de acentos queda REGISTRADO como pendiente.**

## Diagnóstico (logs del15/11, sesión WAN/relay11:01:41–11:04:09, rtt_min274 ms)

```
11:02:07 red_bitrate1080p @3.110.400 → … → @200.000 (suelo en21 s)
11:02:31 red_escala →1632x918 → @2.247.264 → … → @200.000
11:02:53 red_escala →1387x780 → @1.622.790 → … → @200.000
11:03:13 red_escala →1178x663 → …   ·   11:03:43 →1001x563 → …
11:04:08 red_escala →850x478 (cierre de sesión, SIN NINGUNA recuperación)
```
- **6 escalones de escala en2 min,0 `red_recupera*`** →850×478@200 kbps = texto ilegible. Sesión LAN comparable: **sí recuperó** (583k→2.952k) → la lógica funciona solo en enlaces estables.
- **Por qué el movimiento seguía fluido**: la adaptación no toca los fps (solo bitrate/escala); movimiento = contenido barato de baja frecuencia; texto = alta frecuencia espacial → primera víctima de la escala (×0,44) + cuantización + croma4:2:0.
- **Causas raíz**: **A1** — la recuperación exigía exceso≤100 ms ×10 seguidos (+cooldown20) y la zona neutral **borraba** `net_low` → con jitter real era inalcanzable. **A2** — cada `red_escala` re-inflaba el bitrate a `w×h×3` (1632×918→4,47 M ≫ capacidad) → re-congestión → cascada → otra escala: bucle monótono a la baja; sin tope de legibilidad.

## Qué se implementa

| Pieza | Detalle |
|---|---|
| **A1** | `NET_RECOVER` **10→5** · la **zona neutral (100–200 ms) ahora DEGRADA `net_low`** (saturating_sub) en vez de borrarlo (congestión ≥200 sigue limpiando; `net_high` intacto) → la recuperación es alcanzable con jitter real. |
| **A2** | `size_bps(w,h)` = **1 bit/píxel** acotado a `[4×suelo, tope]` → sustituye a `w×h×3` en **los4 sitios** (escala CPU/red + recuperación CPU/red) — ya no se re-infla en cada escalón · **tope de legibilidad**: la escalera de escala **no baja del50 % del tamaño inicial** (1080p→mín≈960×540; Nativa4K→mín1920×1080) — solo bitrate/recuperación por debajo. |
| **BUG (pendiente)** | 🐛 **Acentos**: la dead key viaja como carácter literal (`Char('´')` ×16 en auditoría) → «´o» en vez de «ó». Fix previsto (NO implementado): precomposición en la app + fallback `KEYEVENTF_UNICODE` en el backend. Registrado en README § Pendientes y specs. |

## Tests (+2 →51 suites Rust /25 Flutter)

- `net_low_survives_neutral_jitter`: congestión → cooldown → sanas →
  neutral **decae (5→4, assert directo, no borra)** → recupera al agotar
  cooldown.
- `scale_ladder_is_capped_and_does_not_reinflate`:150 pasos de congestión
  → escalera ≤6 pasos, **nunca <50 % del inicial**, y `bps` tras cada
  escala ≤ `max(w×h,4×suelo)` (la fórmula vieja daba `w×h×3`).
- Los tests de C-13/C-24 existentes **pasan sin cambios** (política
  original preservada donde importa).

## Gates (2026-11-11)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` | **51 suites / 0 fallos** |
| `cargo clippy --workspace --all-targets` | **0 / 0** |
| `cargo fmt --check` | **0** |
| `flutter analyze` / `flutter test` | limpio · **25/25** |

## Pendiente de validación de campo

- Repetir la sesión **WAN** (~10 Mbps): ahora debe verse
  `red_recupera_*` y la escala **subir**; el texto debe mantenerse
  legible (mínimo ≈50 % de la resolución inicial).
- BUG acentos (arriba) cuando se aborde.
