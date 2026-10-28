# Gate C-8 — listener repetidor + proceso que no terminaba · v0.8.2

| Campo | Valor |
|---|---|
| Fecha | 2026-10-28 |
| Rama | `feature/listener-repeat` (D-11) · tag `repeat-v0.8.2` |
| Disparador | Reporte de campo (anfitrión `E:\audiany\anyremote`) |
| Gates | Rust **43 suites / 0 fallos** · **clippy 0 avisos / 0 errores** · **rustfmt 0 diffs** · `flutter analyze` limpio · `flutter test` **5/5** |

## Reporte

> «En el anfitrión, cuando desde el invitado se detiene la conexión, queda el
> estado de *Escuchar (red)* activo, pero no es posible volver a conectarse
> hasta detener la escucha en el anfitrión y volver a activarla.»

## Diagnóstico (dos causas, una encima de otra)

Evidencia en `E:\audiany\anyremote\ui.log`:

```
07:00:49 [daemon] sesión cerrada: bye          ← el invitado cerró
07:00:49 [listener-err] sesión cerrada: heartbeats=0 rtt_min=Nonems ...
        (proceso NO terminó: ningún «listener finalizado»)
07:01:40 [ui] deteniendo listener (bye ordenado)…
07:01:40 [ui] listener finalizado (código 0)    ← solo salió al pulsar Detener (31 s después)
```

1. **`run_listen` era one-shot**: hacía `return` tras la **primera** sesión
   completada → el listener dejaba de `accept()` pero el proceso seguía «vivo»
   y la UI seguía mostrando «Escuchar (red)» → **no aceptaba reconexiones**.
2. **El proceso además se colgaba al salir**: el lector de stdin era una tarea
   de tokio dentro del bloqueo del runtime; al salir de `main` el derrumbe del
   runtime **espera a las tareas bloqueantes en curso** y esa tarea estaba
   bloqueada en `read` con el pipe de la app abierto. **Reproducido con el
   binario v0.8.1**: proceso vivo ≥6 s tras `--max-secs 1` con stdin abierto
   (terminaba solo si alguien escribía algo en stdin — de ahí el «código 0»
   justo tras el `bye` del botón Detener).

## Qué se entregó (v0.8.2)

1. **`--max-sessions N`** (en `DaemonConfig` + CLI): **1 por defecto = one-shot
   histórico** (tests y CLI intactos); **0 = ilimitado** (modo de la app).
   - Tras cerrar una sesión con sesiones restantes y sin pedido de parada, el
     listener **vuelve a `accept()`**.
   - El **`bye` que llega por la RED** (el invitado cierra) **NO** detiene el
     listener → *éste es el fix del reporte*.
   - El **`bye` por STDIN** (botón «Detener listen») o **ctrl+c** cierran la
     sesión **y** detienen el listener (`ctl.stop`).
   - Vida del listener: `--max-secs` (histórico), salvo con `--max-sessions 0`
     donde espera hasta `bye`/señal. Cada **sesión** sigue limitada por
     `max_secs` (600 s por defecto: red de seguridad ante pares que desaparecen
     sin cerrar TCP).
   - **Informe/auditoría por sesión** (un `SessionReport` por sesión, cada una
     registra su `disconnected`).
2. **Espera idle con `select!`**: `accept` + stdin + ctrl+c. Los JSON
   `{"perm":...}` recibidos sin sesión se **encolan** y se aplican al abrirla
   (misma semántica C-2 de antes); `bye` sin sesión ahora **sí** detiene la
   escucha (antes esa línea se quedaba encolada y el botón Detener tenía que
   recurrir al kill).
3. **Fix del cuelgue**: el lector de stdin pasó a un **hilo normal**
   (`read_line` + `blocking_send`) que no retiene el runtime → el proceso
   termina solo cuando `run_listen` devuelve.
4. **App**: `buildListenArgs` (extraído, testable) pasa **`--max-sessions 0`**;
   al cerrarse una sesión con el listener vivo, el estado vuelve a
   **«Escuchando (red)»** (antes: «Desconectado»).
5. **Cosmético**: el resumen imprime `rtt_min=-` (antes `rtt_min=Nonems`).

## Tests nuevos — `daemon_repeat_e2e.rs` (4)

| Test | Qué cubre |
|---|---|
| `listener_serves_a_second_session_after_the_invited_closed_the_first` | **Regresión del reporte**: `--max-sessions 2`; la sesión 1 se cierra con `bye` de RED → sesión 2 completa (`session_active` ×2) sin reiniciar |
| `stdin_bye_stops_the_listener_while_waiting_for_a_client` | `--max-sessions 0`, sin sesión: `bye` por stdin → el listener termina en <3 s |
| `listener_process_exits_after_timeout_with_open_stdin` | **Regresión del cuelgue**: proceso real con stdin ABIERTO y `--max-secs 1` → sale con código 0 en ≤8 s (antes: colgado) |
| `app_flag_max_sessions_zero_stops_on_stdin_bye` | Configuración REAL de la app (`--max-sessions 0`) a nivel de proceso: evento `listening`, `bye` → **exit 0**, y el resumen no contiene `Nonems` |

## Validación

- Rust **43 suites / 0 fallos** (42 previas + `daemon_repeat_e2e`) · clippy
  **0/0** · fmt **0 diffs** · Flutter **analyze limpio + 5/5** (test
  `buildListenArgs: el listener de la app es repetidor`).
- Comportamiento CLI por defecto **sin cambios** (one-shot): todos los e2e
  existentes pasan sin tocarlos.

## Limitaciones

- La **reconexión real entre equipos** con v0.8.2 queda pendiente de
  validación en campo (los tests usan loopback); el escenario reproducido es
  el mismo (cierre del invitado por `bye` de red).
- `--max-sessions 0` deja la vida del listener en manos de `bye`/señal: si la
  app muere sin matar al daemon, el listener seguiría esperando (antes moría
  con ella — pero colgado). El kill de respaldo de la app (`_stopLocalListen`
  con timeout de 3 s) cubre ambos casos.
