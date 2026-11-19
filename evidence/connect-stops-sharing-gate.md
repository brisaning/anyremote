# Gate C-43 — «Conectar» deja de compartir el escritorio (v0.13.15)

**Fecha**: 2026-11-19 · Rama: `feature/c43-connect-stops-share` · Origen: petición
de uso real.

## Comportamiento anterior

Con el escritorio compartido («Compartir escritorio» activo, listener local en
`0.0.0.0:21118`), pulsar **Conectar** lanzaba una segunda instancia del daemon
(`connect`) sin detener la escucha: el mismo equipo quedaba como **anfitrión y
cliente a la vez**, con dos daemons compitiendo por el puerto y por el vídeo.

## Cambio

| Caso | Ahora |
|---|---|
| Compartiendo + **Conectar** | Se registra `dejando de compartir el escritorio antes de conectar…`, se envía `bye` al listener y **se espera su salida** (máx. 3 s; luego `kill`) antes de lanzar `connect`. |
| Sesión de cliente activa + **Compartir escritorio** | Se rechaza con `hay una sesión activa — pulsa Detener antes de compartir` (simétrico, evita el mismo conflicto al revés). |

El listener se cierra de forma **ordenada** (`bye` por stdin) y su handler ya
dejaba `_listener = null` y actualizaba el estado al salir, así que la UI
(cabecera y botón) refleja el cambio sin pasos extra.

## Gates

| Gate | Resultado |
|---|---|
| `flutter analyze` | limpio |
| `flutter test` | **31 / 31** |
| Rust | sin cambios (159 / 0 del gate C-42) |

## Verificación manual

1. «Compartir escritorio» → la cabecera muestra el listener y el botón pasa a
   «Detener compartir».
2. Escribir un ID/destino y pulsar **Conectar**: el registro muestra la línea de
   «dejando de compartir…», el listener termina (código 0) y la sesión de
   cliente arranca después.
3. Con una sesión activa, pulsar «Compartir escritorio»: se avisa y no se abre
   la escucha.
