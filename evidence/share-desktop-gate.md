# Gate C-21 — «Compartir escritorio» con control remoto automático (v0.12.4)

**Fecha**: 2026-11-04 · **Rama**: `feature/share-desktop` · Petición de campo del usuario

## Qué cambia (solo UI de la app — daemon y wire intactos)

1. **Botón renombrado**: `Escuchar (red)` → **`Compartir escritorio`** (el
   hint de error de conexión también cita el nombre nuevo).
2. **Control remoto automático**: al oprimirlo, si «Control remoto» no
   estaba marcado se marca **antes** de `_sendPerms()` → el daemon recibe
   `input:true` al abrir sesión (y el arranque informa `control remoto` en
   los activos). Si ya estaba marcado, no se toca nada. El toggle sigue
   **dinámico en caliente** (D-07): se puede desmarcar durante la sesión.

## Tests

- El botón se localiza y pulsa por el texto nuevo.
- **Nuevo**: tras pulsar, el checkbox «Control remoto» (índice 0) es
  `isTrue` — la activación es del **botón**, previa al arranque (en test no
  hay daemon y el trace «anyremoted.exe no encontrado» sigue comprobándose).
- C-2 intacto: los tres toggles siguen **desactivados por defecto** al
  arrancar la app (solo cambian al oprimir compartir).

## Gates (2026-11-04)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` | **51 suites / 0 fallos** |
| `cargo clippy --workspace --all-targets` | **0 / 0** |
| `cargo fmt --check` | **0** |
| `flutter analyze` / `flutter test` | limpio · **18/18** |

## Pendiente de validación de campo

- Anfitrión: pulsar **Compartir escritorio** sin tocar nada → la visora
  debe poder mover el ratón/teclado **sin marcar «Control remoto»** (y
  desmarcarlo en caliente debe seguir parándolo).
