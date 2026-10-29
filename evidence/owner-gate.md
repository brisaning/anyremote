# Gate C-12 — dueño ausente: el listener ya no queda huérfano · v0.8.6

| Campo | Valor |
|---|---|
| Fecha | 2026-10-29 |
| Rama | `feature/owner-exit` (D-11) · tag `owner-exit-v0.8.6` |
| Disparador | Bug de campo: *«si se activa Escuchar (red) y se cierra el programa, se mantiene la escucha de red; debería detenerse»* |
| Gates | Rust **46 suites / 0 fallos** · **clippy 0 avisos / 0 errores** · **fmt 0 diffs** · `flutter analyze` limpio · `flutter test` **9/9** |

## Causa raíz

1. **Cerrar la ventana NO ejecuta `dispose()` de la app Flutter en Windows**
   (el motor se destruye sin pasar por el árbol de widgets) → el
   `_listener?.kill(...)` de `dispose` **nunca se ejecutaba**.
2. En Windows el hijo **sobrevive a su padre** (no hay SIGHUP) y con
   `--max-sessions 0` (C-8) la escucha era **indefinida** → huérfano
   persistente. (Esta limitación estaba documentada en
   `evidence/listener-repeat-gate.md` desde C-8; ahora queda resuelta.)

## Diseño: el canal de manda del daemon es su dueño

El daemon recibe órdenes por **stdin** (la app mantiene el pipe abierto
toda su vida). Si el pipe muere **con líneas ya leídas** (el gestor
demostró existir — la app escribe `{"perm"...}` nada más lanzar la escucha),
el dueño desapareció → se envía una **línea sintética**
`dueño ausente (stdin cerrado)` que recorre los caminos EXISTENTES:

| Momento del EOF | Camino | Efecto |
|---|---|---|
| Escucha (idle) | `Idle::Stop` (C-8) | auditoría + `closed` + **el listener termina** (código 0) |
| Sesión activa (anfitrión) | brazo de stdin de `active_controlled` | cierra la sesión con `Bye` + `ctl.stop` → **la escucha termina también** (el par ve el motivo) |
| Sesión activa (visor) | brazo de stdin de `active_controller` | cierra la sesión con ese motivo |

- **Discriminador (clave para no romper nada)**: la sintética solo se envía
  si `lines_read > 0`. EOF **sin líneas** = stdin nulo/despreocupado
  (modo desatendido, tests — que nunca escriben en su propio stdin) → **NO
  detiene nada**, igual que antes.
- **App (lado visor)**: escribe `{"ui":"ready"}` al conectar — JSON sin
  `input`, **ignorado** por el daemon (también por versiones antiguas: sin
  romper nada) → el controlador también sabe desde el primer instante que
  hay dueño (cubre cerrar el visor antes de usar el ratón).

Por qué no un Job Object de Windows (FFI): la regla de stdin es
suficiente, sin dependencias nuevas, aplicable también en CLI y testeable de
punta a punta.

## Tests — `daemon_owner_e2e.rs` (3)

| Test | Qué cubre |
|---|---|
| `listener_process_stops_when_owner_closes_stdin` | **La regresión exacta**: proceso real, `--max-sessions 0`, el «dueño» escribe su línea y cierra el pipe → **sale solo con código 0** ≤6 s y audita `dueño ausente (stdin cerrado)` (sin fix: viviría hasta `--max-secs 30` y el test fallaría) |
| `detached_stdin_eof_does_not_stop_the_listener` | EOF **sin líneas** → **no** detiene: termina por `listen_timeout` y **no** contiene el motivo de dueño ausente (protege el modo desatendido y los tests) |
| `owner_loss_during_session_closes_it_and_the_repeat_listener` | Pérdida del dueño **durante sesión** con repetidor (`max_sessions=0`): la sesión cierra con el motivo, **la escucha NO re-acepta** y el par recibe el `Bye` con `dueño ausente (stdin cerrado)` |

## Regresión

Rust **46/0** · clippy **0/0** · fmt **0 diffs** · Flutter **analyze limpio +
9/9** — todos los e2e previos (reconexión C-8, cursor C-11, CLI id, modo
visor) intactos.

## Limitaciones

- Ventana cerrada en los **primeros ~100 ms** tras pulsar Escuchar (antes de
  que llegue la línea inicial de la app): sin gestor visto → no se detecta
  (ventana mínima; `dispose` tampoco corre).
- Un visor que **nunca** escribe nada (ni `{"ui":"ready"}` por versión
  antigua de la app) con la sesión abierta → el controlador se cierra por su
  propio `max_secs` (≤600 s, acotado, comportamiento previo).
