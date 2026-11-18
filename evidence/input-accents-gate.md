# Gate C-37 — acentos (dead key) y teclas de edición/bloqueo (v0.13.10)

**Fecha**: 2026-11-18 · Rama: `feature/c37-accents` · Origen: **BUG conocido**
(registrado el 15/11, pospuesto): al teclear `´` + vocal el anfitrión escribía
`´o` en vez de `ó`.

## Causa

Dos problemas encadenados:

1. **App (cliente)**: la dead key viajaba como un carácter más (`Char('´')`) y la
   vocal llegaba en la siguiente pulsación. El anfitrión recibía **dos**
   caracteres: `´` y `o` — nunca `ó`.
2. **Backend Windows**: todo carácter se inyectaba por `VkKeyScanW` + VK. Para un
   carácter ya compuesto (`ó`), `VkKeyScanW` depende del **layout del anfitrión**
   y puede devolver `-1` (fallo) o exigir una combinación de teclas que no
   reproduce el carácter seguro.

## Cambio

| Capa | Detalle |
|---|---|
| **App** | `isDeadAccent()` + `composeDeadAccent()`: las dead keys (`´` `` ` `` `^` `¨` `~`) **no se envían**; se guardan y se componen con la siguiente pulsación (`´`+`o` → `ó`, `¨`+`u` → `ü`, `~`+`n` → `ñ`, mayúsculas incluidas). Doble pulsación (`´´`) envía el acento suelto. Si la pareja no compone (`´x`), se envían ambos por separado, como el SO. Una dead key pendiente se vuelca antes de cualquier tecla no-carácter (flechas, F5…) para no perderla. |
| **Backend Rust** | `target_for()`: ASCII → VK (los atajos `Ctrl+C`/`Ctrl+V` siguen siendo atajos reales); **no ASCII → `KEYEVENTF_UNICODE`** con el codepoint exacto, sin depender del layout del anfitrión. |
| **Teclas nuevas** | `Insert` (12), `PageUp` (13), `PageDown` (14), `CapsLock` (23), `NumLock` (24), `ScrollLock` (25): modelo `Key`, tabla wire (`arsp.rs`), `keyCodeFor` de la app y VK del backend (con `EXTENDEDKEY` donde corresponde). |

## Tests

| Test | Qué fija |
|---|---|
| Dart `C-37: dead key + vocal compone el carácter` | `´+o→ó`, `¨+u→ü`, `~+n→ñ`, `´+O→Ó`, `~+N→Ñ`, pares que no componen → `null`, y `isDeadAccent` |
| Rust `non_ascii_chars_use_unicode` | `á`, `ñ`, `ü`, `€` → `Target::Text` |
| Rust `ascii_chars_keep_the_virtual_key` | `a` → VK 0x41 sin shift; `A` → con shift |
| Rust `editing_and_lock_keys_have_vk` | VK 0x2D/0x21/0x22/0x14/0x90/0x91 |
| Rust `editing_and_lock_keys_map_from_wire_codes` | códigos 12/13/14/23/24/25 → `Key` (y 15 → `None`) |

## Gates (2026-11-18)

| Gate | Resultado |
|---|---|
| `cargo test --workspace` | **159 / 0** |
| `cargo clippy --workspace --all-targets` | **0 avisos** |
| `cargo fmt --all -- --check` | **0 diffs** |
| `flutter analyze` | limpio |
| `flutter test` | **28 / 28** |

## Verificación de campo pendiente

Julie → Brian: teclear `acento + vocal` en un editor del anfitrión debe escribir
`ó`/`ü`/`ñ` una sola vez; `Insert`/`RePág`/`AvPág` y los bloqueos deben responder.
Probar también `´x` (acento suelto + x) y `´´` (acento suelto).
