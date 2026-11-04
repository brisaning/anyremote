# Gate C-22 — campo rdv vacío con default efectivo + auto-detección LAN/WAN visible (v0.12.5)

**Fecha**: 2026-11-04 · **Rama**: `feature/lan-wan-auto` · Petición de campo del usuario

## Qué cambia (app + visibilidad; el daemon ya resolvía solo)

| Pedido | Estado | Detalle |
|---|---|---|
| «Detener listen» → **«Detener compartir»** | ✅ | label del botón (y comentario del test asociado). |
| Campo servidor de encuentro **en blanco** | ✅ | el controller arranca `''` (hint «vacío = VPS»); el **valor efectivo** lo fija `effectiveRdv()`: vacío → `51.222.81.81:21120` (VPS `serveremote`), escrito → ese. `--rdv` se pasa **siempre** (connect y listen) y la línea de arranque del listener lo informa (`— encuentro …`). |
| **Auto-detección LAN/WAN del ID** | ✅ (ya existía + visible) | `resolve_id` (C-16/C-17): **broadcast LAN (3 × 900 ms) → servidor de encuentro → caché** (la caché va DESPUÉS para no tapar respuestas frescas). La app ahora pinta el evento `resolve_ok`: **`ID resuelto → LAN (broadcast) \| WAN (servidor de encuentro) \| caché — LAN/WAN`** (clasificando la dirección con `isLanAddress`) y `rdv_error` con la causa real. |

**Por qué la LAN no depende del VPS**: el broadcast va PRIMERO y en campo
resolvió en 343 ms; el rdv solo se consulta si aquél falla (ID WAN: ≤2,7 s
de sonda LAN → rdv). La reserva de relay solo existe cuando el ID se resolvió
por el rdv (camino WAN); las conexiones por IP directa no la llevan.

## Tests (+3 → **21**)

- `effectiveRdv`: vacío/espacios → `51.222.81.81:21120`; escrito → se
  respeta (con y sin espacios).
- `isLanAddress`: RFC1918 (10/8, 172.16/12, 192.168/16), loopback, ULA v6
  (`fd…`) y link-local → LAN; públicos y v6 global → WAN (negativos:
  `172.32.x`, `192.169.x`).
- `resolveViaLabel`: `broadcast`/`rdv`/`cache` → etiqueta correcta.
- Widget: el campo arranca **vacío** (invierte la aserción de C-20).

## Gates (2026-11-04)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` | **51 suites / 0 fallos** |
| `cargo clippy --workspace --all-targets` | **0 / 0** |
| `cargo fmt --check` | **0** |
| `flutter analyze` / `flutter test` | limpio · **21/21** |

## Pendiente de validación de campo

- ID con el anfitrión en **LAN** → `ID resuelto → LAN (broadcast)` y sesión
  directa (sin tocar el VPS).
- ID con el anfitrión en **otra red** → `ID resuelto → WAN (servidor de
  encuentro)`.
- Ver `— encuentro 51.222.81.81:21120` en la línea de arranque del listener
  con el campo vacío.
