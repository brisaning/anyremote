# C-52 · Fase B2 — el objetivo nunca supera lo que el enlace entrega

| Campo | Valor |
|---|---|
| Fecha | 2026-11-24 |
| Rama | `feature/c52-video-regression` (Fase A + Fase B ya en la rama; **sin commit**: lo cierra el orquestador) |
| Alcance | **Fase B2** de C-52: P1 (presupuesto por tasa entregada), D2' (anti-oscilación) y P1-TCP (descarte de frames de vídeo) |
| Semilla | `evidence/c52-field-gate-failed.md` (gate de campo **FALLIDO** del 0.14.4) |
| Diseño | comisión del orquestador con los valores aprobados por el usuario (2026-11-23) |
| Toolchain | Rust **MSVC** `stable-x86_64-pc-windows-msvc` (`-j 2` en el workspace: con más paralelismo aparece `LNK1104` de `msvcrt.lib`, del entorno) |
| Estado | ✅ implementado · ✅ 3 gates nuevos verdes · ✅ workspace **185/0** · ✅ clippy 0 · ✅ fmt 0 · ⏳ gate de campo (Fase C) |

---

## 1. Los tres valores aprobados → cómo quedan

| # | Valor aprobado | Implementación | Dónde |
|---|---|---|---|
| **1** | `objetivo = min(piso_bpp(tamaño), 0,8 × tasa_entregada_EWMA)`; si el piso no cabe, **se baja la resolución**, nunca se pide más bitrate del que hay | `DELIVERY_MARGIN = 0.8` · `budget_bps()` = `0,8 × entregada` acotado por el tope · `target_min(w,h)` = `min(piso_bpp, presupuesto)` · `choose_size()` = el mayor escalón cuyo piso cabe en el presupuesto (si ninguno cabe, el suelo de la escalera y el objetivo por debajo del piso: fail-open del texto, control vivo) | `video.rs:292`, `:439`, `:452`, `:469`, `:489` |
| **1b** | La tasa inicial **ya no es el piso a ciegas**; arranca conservadora y sube con evidencia | `INITIAL_TARGET_BPS = 2 Mbps`; `with_fps` toma `min(piso, tope, 2 Mbps)`; la subida es la de siempre (`recover`: ×1,5 cada 3 muestras buenas) pero **acotada por el presupuesto**, nunca por el tope del usuario a ciegas | `video.rs:299`, `:402-411`, `:574` |
| **2** | Histéresis de **10 s** entre cambios de escala; subida máximo **cada 3 muestras buenas** | `SCALE_HYSTERESIS_MS = 10 000` y `scale_hold_until_ms`: `apply_scale` sella el próximo cambio; `scale_down`/`recover`/`observe_delivery` lo consultan. `NET_RECOVER = 3` intacto y `apply_scale` pone `net_low = 0` (la subida exige muestras **nuevas**). Al recuperar **no se borra `net_high`** (la evidencia de congestión que habilita la oscilación se conserva) | `video.rs:310`, `:503`, `:522-543`, `:548`, `:574`, `:715-745` |
| **3** | En TCP, **descartar** frames de vídeo en vez de encolarlos (máx. 2 en vuelo); el control nunca se descarta | `VIDEO_WRITE_BUDGET_MS = 100` + `VIDEO_DROP_HOLD_MS = 1 000`: una escritura ≥ 100 ms cierra la compuerta 1 s; mientras esté cerrada **no se escribe vídeo** (`continue`), se cuenta `video_dropped_slow` y se emite el evento una vez por episodio. Canal vídeo→sesión a profundidad **2**. Las tramas de control (input/audio/latidos/cursor) no pasan por la compuerta | `video.rs:315`, `:320`, `:869-903` · `lib.rs:1783`, `:2142-2170` |

---

## 2. Qué cambió (archivo:línea)

### Producción

| Archivo | Línea | Qué |
|---|---|---|
| `core/daemon/src/video.rs` | `:283-320` | Constantes nuevas: `DELIVERY_MARGIN`, `INITIAL_TARGET_BPS`, `MIN_TARGET_BPS`, `SCALE_HYSTERESIS_MS`, `VIDEO_WRITE_BUDGET_MS`, `VIDEO_DROP_HOLD_MS` |
| | `:338` | `even()`: la escalera produce dimensiones **pares** (4:2:0; el campo generaba 1387×780 y 1001×563) |
| | `:353-382` | `AdaptState` + campos `delivered_bps`, `now_ms`, `scale_hold_until_ms` |
| | `:402-411` | Tasa inicial conservadora (el piso ya no se pide a ciegas) |
| | `:439-461` | `budget_bps()`, `target_min()`, `floor_bps()` |
| | `:469-500` | `ladder_sizes()` (pares, de mayor a menor) y `choose_size()` (el mayor que cabe en el presupuesto) |
| | `:503-543` | `scale_hold_expired()` y `apply_scale(size, reason, raising)` (histéresis + `net_low = 0`; al bajar **nunca se sube la tasa**) |
| | `:548-568` | `scale_down()` recorre la escalera (un peldaño) |
| | `:574-598` | `recover()`: salta al mayor tamaño que cabe y luego sube la tasa hasta el **presupuesto** |
| | `:607-653` | `observe_delivery()`: techo duro + tamaño + piso del tamaño activo |
| | `:655-712` | `observe()` (CPU): reloj de la política (+4 s por decisión) y presupuesto en la presión |
| | `:715-745` | `observe_net()`: reloj (+1 s por muestra), `target_min` en la congestión y `recover` acotado |
| | `:774` | `decision_needs_refresh` incluye `presupuesto_sube` (el IDR al recuperar se conserva) |
| | `:812-857` | `DeliveryMeter`: capacidad medida **solo** con escrituras bloqueantes, solo baja, publica cambios ≥ 10 % |
| | `:869-903` | `VideoGate`: `allows` / `on_written` / `on_dropped` / `close_episode` |
| | `:968-980` | `spawn_video_task(..., delivery_rx)`: el hilo de vídeo recibe la capacidad medida |
| | `:1051-1072` | La línea `[video] fps (5 s)` añade **`objetivo=` y `entregada=`** (el gate de campo compara ambos) |
| | `:1203-1284` | Aplicación de la decisión: presupuesto → red → CPU; `video_adapt` con `delivered` y `budget` |
| `core/daemon/src/lib.rs` | `:34` | Import de `DeliveryMeter`/`VideoGate` |
| | `:1783` | Canal vídeo→sesión a **2** (máximo de frames en vuelo) |
| | `:1841-1847` | `watch` de la entrega + `DeliveryMeter` + `VideoGate` + reloj del bucle |
| | `:2142-2155` | Compuerta: si no se puede escribir, **se descarta** (`video_dropped_slow`, `frames_dropped_slow`) y se informa el episodio una vez |
| | `:2156-2170` | Escritura medida: `on_written` (cierra la compuerta si tardó ≥ 100 ms) y `delivery.observe` → `watch` |
| `core/telemetry/src/lib.rs` | `:155-161`, `:179`, `:205-209` | `frames_dropped_slow` en el informe de sesión (`record_frame_dropped_slow`) |

### Arnés de tests (mismo archivo, `video.rs`)

| Bloque | Línea | Qué afirma ahora |
|---|---|---|
| `mod c52_ladder` | `:1696` | Replay de los números de campo (RTT y 21 decisiones) **conservado**; cambia lo que la política hace con ellos |
| `field_ladder_starts_conservative_and_climbs_with_delivery` | `:1808` | Arranque 2 Mbps (no 13,99), enlace de 1 Mbps → 800 kbps y suelo, enlace de 20 Mbps → piso de bpp → tope |
| `ladder_preserves_bpp` | `:1859` | Con entrega suficiente, ningún escalón baja del piso; con 1 Mbps el objetivo queda **exactamente** en el presupuesto y en el suelo |
| `c52_session2_replay_keeps_the_bpp_floor` | `:1910` | El replay del colapso conserva el piso de bpp (suelo ahora **1000×562**, par) |
| **`video_target_never_exceeds_delivered_rate`** | `:2013` | **Gate D1'** (ver §3) |
| **`ladder_does_not_oscillate`** | `:2079` | **Gate D2'** (ver §3) |
| `mod c52_tcp` | `:2366` | Arnés del canal TCP compartido (bucle secuencial + socket FIFO + política real) |
| **`control_frames_are_not_starved_by_video`** | `:2543` | **Gate P1** (ver §3) |
| `field_session_converges_to_the_delivered_budget` | `:2629` | Lazo cerrado: sobre 1 Mbps el objetivo converge a **800 000 bps**, el tamaño al suelo y no oscila (habría cazado el techo auto-referencial) |
| `video_gate_closes_on_slow_writes_and_reopens` | `:2685` | La compuerta se cierra con escrituras lentas, se reabre sola y el episodio se informa una vez |
| `delivery_meter_reads_the_link_and_not_the_target` | `:2713` | Solo una escritura bloqueante mide; las rápidas no publican; la capacidad no se reinfla |

---

## 3. Gates nuevos (invocación exacta y salida real)

```
cd core && cargo test -p anyremote-daemon --lib video_target_never_exceeds_delivered_rate
```
```
running 1 test
test video::c52_ladder::video_target_never_exceeds_delivered_rate ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 27 filtered out; finished in 0.00s
```

```
cd core && cargo test -p anyremote-daemon --lib ladder_does_not_oscillate
```
```
running 1 test
test video::c52_ladder::ladder_does_not_oscillate ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 27 filtered out; finished in 0.00s
```

```
cd core && cargo test -p anyremote-daemon --lib control_frames_are_not_starved_by_video
```
```
running 1 test
test video::c52_tcp::control_frames_are_not_starved_by_video ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 27 filtered out; finished in 0.00s
```

### Qué exige cada gate (falsable)

| Gate | Afirma |
|---|---|
| `video_target_never_exceeds_delivered_rate` | Con los bloques de 5 s del gate de campo fallido (**2802 / 507 / 3134 / 383 kbps** y ~1 Mbps de sesión) y las tres resoluciones probadas (720p, 1080p y **nativa 4K**), tras cada medida: `bps ≤ 0,8 × entregada` y `bps ≤ presupuesto`. Con 1 Mbps el objetivo es **800 000 bps** en las tres resoluciones y el tamaño queda en el suelo (en la Fase B eran 13,8 Mbps a 1080p y **56 Mbps** en 4K). Contraste: con 20 Mbps entregados el objetivo llega al piso de bpp de 1080p (13 996 800) y el tamaño no baja |
| `ladder_does_not_oscillate` | (a) Con los números de campo el tamaño cambia **≤ 1 vez**. (b) Con un enlace que cruza el escalón de 1178×663 (10 Mbps ↔ 1 Mbps) hay **≤ 4 cambios en 30 s** y **≥ 10 s entre cambios** (el campo hizo 6 en 30 s). (c) Un salto de subida exige **3 muestras buenas nuevas** (con 1 o 2 no hay decisión) |
| `control_frames_are_not_starved_by_video` | Enlace 1 Mbps real, frame de campo 131,5 KB, cola del socket de **2 frames** y buffer vacío al arrancar: (a) la compuerta **descarta** vídeo (`dropped > 0`, `written < attempted`); (b) la latencia del control queda **por debajo de 1 s** (media **y** peor caso); (c) sin la política B2 (Fase B: encoder al piso a ciegas y sin compuerta) la latencia **crece ≥ 1 s** y es **≥ 4×** peor; (d) **mecanismo**: con B2 el bucle pasa **< 50 %** del tiempo bloqueado escribiendo vídeo; en la Fase B, **> 90 %**; (e) los 240 eventos de control de la ventana se entregan (el control **nunca** se descarta); (f) el presupuesto no oscila (`scale_changes ≤ 2`) |

**Verificación por mutación** (hecha y revertida): sustituir `if policy == Policy::FaseB ||
gate.allows(t)` por `if true` (es decir, volver a escribir el vídeo aunque la compuerta esté
cerrada) hace **fallar** el gate en la primera aserción:

```
thread 'video::c52_tcp::control_frames_are_not_starved_by_video' panicked at daemon\src\video.rs:2565:9:   (nº de línea de la versión mutada)
la compuerta tiene que descartar: Stats { frames_attempted: 180, frames_written: 180, frames_dropped: 0, ... }
test result: FAILED. 0 passed; 1 failed; ...
```

### Números medidos del arnés (instrumentación temporal, revertida)

| Política | Latencia media del control | Peor caso | Frames escritos/ intentados | Descartados | Tiempo del bucle bloqueado por vídeo | Cambios de escala |
|---|---|---|---|---|---|---|
| **B2** (producción) | **22 ms** | **54 ms** | 165 / 180 | **15** | **3 %** | 1 |
| Fase B (0.14.4) | **111 061 ms** | 177 502 ms | 180 / 180 | 0 | **100 %** | — |

La Fase B reproduce el campo: la cola de vídeo crece sin límite y el control espera
detrás de ella (los 111 s son la deriva del backlog; en campo se vio como «no se
puede seleccionar ni mover una ventana» durante minutos). Con B2 la medida de
entrega detecta el enlace en ~1 frame, el presupuesto baja a 0,8 Mbps, el frame a
~6,6 KB y la compuerta descarta el segundo de vídeo que sobraba.

---

## 4. Gates globales (salida real)

```
cd core && cargo fmt --all -- --check          # → sin diferencias (exit 0)
```
```
cd core && cargo clippy --workspace --all-targets -- -D warnings
```
```
    Checking anyremote-daemon v0.14.4 (C:\proyectos\fixed\anyremote\core\daemon)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.45s
```

```
cd core && cargo test --workspace -j 2
```
```
TOTAL passed=185 failed=0 ignored=0   (42 binarios de test + 12 doc-tests, todos `test result: ok`)
```

Salida por binario (extracto literal de la corrida): `anyremote-daemon` lib
`ok. 28 passed; 0 failed` (0,01 s) · `anyremote-codec` `ok. 20 passed; 0 failed` (6,65 s) ·
`anyremote-transport` `ok. 20 passed; 0 failed` (0,94 s) · `anyremote-capture`
`ok. 5 passed; 0 failed` · `daemon_net_e2e` `ok. 1 passed; 0 failed` (6,02 s) ·
`daemon_video_e2e` `ok. 1 passed; 0 failed` (4,48 s) · `daemon_h264_e2e`
`ok. 1 passed; 0 failed` (7,48 s) · `daemon_quic_e2e` `ok. 3 passed; 0 failed` (7,52 s).

> `-j 2` no es un capricho: con el paralelismo por defecto el enlazador falla
> con `LNK1104: no se puede abrir el archivo ...\msvcrt.lib` (el fichero existe;
> es contención del entorno, no del código). Con `-j 2` el workspace está verde.
>
> Regresión de los ciclos anteriores: C-27/C-29/C-32/C-35/C-36 (y el e2e
> `controller_rtt_hint_makes_the_host_adapt_bitrate`) verdes, incluido el
> `daemon_net_e2e` que comprueba que la pista de RTT del controlador sigue
> moviendo la política del anfitrión.

---

## 5. Recalibración del arnés `c52_tcp` (qué cambió del modelo y por qué)

La primera versión del arnés estaba mal calibrada por un **error de unidades**: usaba
`LINK_BPS / 1000.0` como bytes por milisegundo, así que el «enlace de 1 Mbps» era en
realidad de 8 Mbps y no se saturaba nunca. Además el modelo de socket descontaba mal
los bytes que drenan durante el bloqueo y el buffer arrancaba con contenido heredado.

Cambios (todos en `video.rs`, bloque `mod c52_tcp`, `:2366-2541`):

| Pieza | Antes | Ahora |
|---|---|---|
| Unidades | `bps/1000` (bits/ms) ≈ 8 Mbps | `LINK_BYTES_PER_MS = LINK_BPS / 8000.0` = **125 B/ms = 1 Mbit/s** |
| Buffer del socket | 512 KB fijos, podía arrancar «lleno» | **2 frames en vuelo** (`IN_FLIGHT_FRAMES`), **vacío** al inicio, capacidad en función del frame del momento |
| Semántica de la escritura | descontaba el bloqueo de la cola, no del frame | FIFO correcta: `block = max(0, cola + bytes − cap)/velocidad`; entrega = último byte por el cable |
| Ventana medida | 6 s de calentamiento excluidos | **toda la ventana (12 s)**: el criterio es absoluto |
| Contrafactual | «compuerta off» con el mismo encoder | **`Policy::FaseB`**: encoder al piso de bpp a ciegas (131,5 KB/frame, lo que hacía el 0.14.4) **y** sin compuerta — el defecto real, no una variante del arreglo |
| Medida del mecanismo | no existía | `blocked_fraction`: fracción del tiempo de sesión que el bucle pasa **dentro** de la escritura de vídeo |

El criterio quedó **explícito en el test** (no en un comentario suelto): latencia de
control **< 1 000 ms** (media y máximo) con B2; **≥ 1 000 ms** y **4×** peor sin ella;
descarte de vídeo **> 0**; bucle bloqueado **< 50 %** con B2 y **> 90 %** sin ella.

---

## 6. Desviaciones declaradas (del diseño recibido)

1. **La capacidad medida solo baja** (`DeliveryMeter` hace `min`). Motivo: una
   escritura bloqueante mide *cuánto drena* el enlace; una instantánea solo prueba
   que el frame cabía. Subir la capacidad con «lo que ofrecemos» crea un lazo
   cerrado perverso (0,8 × nuestra propia oferta hunde el objetivo un 20 % por
   ventana) — está cubierto por `field_session_converges_to_the_delivered_budget`.
   **Coste**: un enlace que **mejora** a mitad de sesión no se aprovecha hasta que
   se vuelve a saturar (ver §7).
2. **`MIN_TARGET_BPS = 150 kbps`** es la única excepción a
   `objetivo ≤ 0,8 × entregada`: por debajo de ~187 kbps el encoder se quedaría sin
   suelo operativo. Se prefiere un objetivo mínimo funcional a un vídeo a cero con
   el control vivo pero sin imagen.
3. **Escalera de dimensiones pares** (`even()`, `video.rs:338`). No estaba en el
   diseño; el replay del campo producía 1387×780 y 1001×563 y el pipeline 4:2:0
   (`codec/src/conv.rs:9-12`) las rechaza — en debug era un `debug_assert` que
   mataba el hilo de vídeo. El suelo pasa de 1001×563 a **1000×562**.
4. **`recover()` sube de un salto** al mayor tamaño que cabe en el presupuesto (no
   un 85 % por escalón): con 10 s de histéresis, subir de uno en uno tardaría un
   minuto. La bajada sí es de un peldaño (la escalera probada).
5. **Nativa 4K sobre un enlace de 1 Mbps → 1920×1080**. El suelo de la escalera es
   el **50 % del inicial** o 640×360 *el que sea mayor* (regla aprobada en la Fase
   B), así que una sesión 4K no puede bajar de 1920×1080: a 0,8 Mbps eso es
   **0,013 bpp** (texto ilegible, control vivo). La regla no se toca en B2 (ver §7
   y C-58).
6. **El evento `video_dropped_slow`** se emite una vez por episodio (no por frame) y
   el contador va también al informe de sesión (`frames_dropped_slow`). La app lo
   pinta por su rama `default` sin cambios de contrato.

---

## 7. Riesgos abiertos y qué debe medir el gate de campo

### Riesgos

| Riesgo | Detalle |
|---|---|
| **Capacidad pegajosa** | Si el enlace mejora a mitad de sesión, el presupuesto no sube (solo lo hace por la recuperación de red, que está acotada por el presupuesto). Mitigación posible (fuera de alcance): una sonda lenta de subida cuando no hay saturación durante N segundos |
| **Nativa 4K en enlaces lentos** | El suelo de 50 % deja 1920×1080 a 0,013 bpp. Sin bajar el fps (**C-58**, ciclo propio ya documentado en `specs/02` §13.8) no hay forma de tener 0,45 bpp a 0,8 Mbps |
| **QUIC no alimenta la medida** | En el stream dedicado el envío es `try_send` no bloqueante; no hay «escritura lenta» y por tanto el presupuesto se queda en el tope del usuario. El vídeo de QUIC conserva el comportamiento anterior (no se toca, por diseño de la comisión) |
| **Primer segundo** | La primera medida necesita un frame que bloquee; hasta entonces el objetivo es 2 Mbps. Sobre un enlace de 1 Mbps eso son ~1 s de vídeo de más (el arnés mide 15 frames descartados en ese tránsito) |
| **`MIN_TARGET_BPS`** | Por debajo de ~187 kbps entregados el objetivo puede superar `0,8 × entregada` (suelo del encoder) |

### Guion de campo (2 min) y qué debe medir el usuario

**Anfitrión** (`ui.log` / `[listener-err]`):

1. En **cada** bloque `[video] fps (5 s)` leer `objetivo=` y `entregada=`: debe cumplirse
   **`objetivo ≤ 0,8 × entregada`** (o `objetivo = 150 kbps` si el enlace es peor que eso).
   En el 0.14.4 el objetivo era 14–25 Mbps con ~1 Mbps entregados.
2. Que cuando el enlace no da para el piso de bpp **baja la resolución** (`(WxH)` del bloque
   cambia) en vez de que el bitrate se quede pidiendo de más; y que la resolución **no**
   cambia más de una vez cada 10 s.
3. Que existen eventos `video_adapt` con `reason` `presupuesto_*` y con `delivered`/`budget`.
4. Que aparece `video_dropped_slow` (vídeo descartado) **solo** cuando hay saturación, y que
   el informe de cierre trae `frames_dropped_slow`.
5. Que el anfitrión sigue aceptando la entrada (auditoría `input_injected`) **durante** los
   bloques de vídeo.

**Cliente**:

1. `visor: fps render`: debe subir de los **0,3 fps** del gate fallido (con ~1 Mbps y 640×360
   a 15 fps el techo real es ~15 fps; comparar con el `recv`).
2. **Latencia del control** (lo que falló): abrir el menú de inicio y **arrastrar** una
   ventana mientras hay movimiento en el escritorio remoto; cronometrar el clic → reacción.
   Criterio: respuesta perceptible (< ~300 ms en LAN/relay), sin los segundos del 0.14.4.
3. Sesión por **relay** y en **Nativa (4K)**: confirmar que la app **no** se queda sin
   responder aunque la imagen quede borrosa (fail-open documentado) y que el `bpp` del
   cliente acompaña a la resolución elegida por el anfitrión.

**Reproducción mínima** (la del gate fallido, §5 de `c52-field-gate-failed.md`): host en
Nativa, cliente 1080p, relay, 60 s; leer 3 bloques de `[video] fps (5 s)` del anfitrión y 3
líneas de `visor: fps render` del cliente, **más** el conteo de frames descartados y la
latencia de un clic.
