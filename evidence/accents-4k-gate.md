# Gate C-38/C-39 — acentos duplicados, navegación sin Shift y 4K en GPU (v0.13.11)

**Fecha**: 2026-11-18 · Rama: `feature/c38-c39-input-4k` · Origen: prueba de campo
de C-37: «reconoce el acento, pero pone una tilde antes de la letra» y «RePág/
AvPág o las teclas de navegación seleccionan el texto»; más el 4K pendiente.

## C-38 — dos bugs de entrada

### 1. Acento duplicado (`´ó`)

Windows **ya compone** el carácter: con una dead key pendiente, la pulsación de
`o` llega a Flutter como `ó`, no como `o`. La lógica de C-37 intentaba componer
`´`+`ó` (no existe), así que enviaba el acento suelto **y** el carácter.

Fix (`resolveDeadKey` en la app): si el carácter que llega **no es ASCII** se
considera ya compuesto → se envía solo él y se descarta la dead key. Si es ASCII
y no compone (`´x`), se envían ambos, como el SO. Doble pulsación sigue enviando
el acento suelto.

| Caso | Antes | Ahora |
|---|---|---|
| `´` + `o` (SO compone → `ó`) | `´ó` | **`ó`** |
| `´` + `o` (llega `o` sin componer) | `ó` | `ó` |
| `´` + `x` | `´x` | `´x` |
| `´` + `´` | `` ´ `` | `` ´ `` |

### 2. Navegación que seleccionaba texto

`vk_for` devolvía `(vk, shift)` y las teclas de navegación estaban marcadas con
**`shift = true`**: cada flecha, `Home`, `End`, `RePág`, `AvPág`, `Insert` y
`Bloq Num` se inyectaba con **Shift pulsado** → el anfitrión seleccionaba.

Fix: `shift = false` en todas. Test `editing_and_lock_keys_have_vk_without_shift`
cubre 13 teclas (VK + "sin Shift").

## C-39 — 4K con NVENC (y el falso límite de C-35)

C-35 concluyó que el MFT asíncrono «se atasca a 4K». **No era el MFT**: la sonda
llamaba a `encode()` en bucle cerrado. Con la configuración de producción y un
bucle de eventos manual, el mismo MFT codificaba 4K sin problema (30/30), y con
**cadencia real (20 ms entre frames)** el flujo de producción también:

| Resolución | Emitidos | KB/frame | ms/frame |
|---|---|---|---|
| 1080p @ 2 Mbps | 24/24 | 10,6 | 11,8 |
| 1080p @ 6 Mbps | 24/24 | 40,1 | 12,1 |
| 1080p @ 15 Mbps | 24/24 | 100,4 | 12,8 |
| **4K @ 25 Mbps** | **24/24** | **212,2** | **25,5** |

(4K antes: 70–394 ms con el MFT de software; ahora 25,5 ms con tasa real.)

Cambios:

- Se retira la guarda «async solo hasta 1080p».
- **Muestras en vuelo** (`in_flight`): la muestra enviada se mantiene viva hasta
  que llega su salida (un MFT asíncrono puede retenerla; a 4K tarda más).
- **Detección de atasco real**: 4 entradas sin ninguna salida → se cae al MFT
  síncrono (`async_failed`) en vez de congelar el vídeo.
- El camino asíncrono ya no reserva el slot de entrada de 12 MB que no usa.
- Sonda `video_quality_probe` con **cadencia de 15 fps** (el error de método que
  provocó el falso límite).

## Gates (2026-11-18)

| Gate | Resultado |
|---|---|
| `cargo test --workspace` | **159 / 0** |
| `cargo clippy --workspace --all-targets` | **0 avisos** |
| `cargo fmt --all -- --check` | **0 diffs** |
| `flutter analyze` | limpio |
| `flutter test` | **29 / 29** |

## Verificación de campo pendiente

- `acento + vocal` → **una sola** letra (sin tilde delante).
- Flechas/`RePág`/`AvPág`/`Home`/`End` **mueven** el cursor sin seleccionar.
- «Nativa» (4K) en el visor: el log debe mostrar `'NVIDIA H.264 Encoder MFT'`,
  `encode_ms` ~25 ms y `escala_ms` ~13 ms (sin escalado a 4K nativo).
