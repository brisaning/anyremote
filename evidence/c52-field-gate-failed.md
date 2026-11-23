# C-52 · Gate de campo FALLIDO (2026-11-23) — el 0.14.4 dejó la app inutilizable

| Campo | Valor |
|---|---|
| Binario probado | **v0.14.4** (C-52 Fase B) en el anfitrión; cliente 0.14.1 |
| Escenario | Sesión Julie (`ONDH6IU4RU`) → Brian (`GLV5Q76BAQ`), red local + relay (VPS `51.222.81.81:21120`) |
| Resoluciones probadas | 720p · 1080p · **Nativa (4K)** |
| Logs | Anfitrión `C:\Users\wnuke\AppData\Roaming\anyremote` · Cliente `E:\audiany\anyremote` |
| Veredicto | **FALLIDO** — el campo no valida la Fase B |

## 1. Síntomas reportados por el usuario

1. «no se puede seleccionar una ventana ni moverla» (ni prueba de escritura).
2. «tarda mucho en conectar».
3. El diálogo «Sesión entrante» **vuelve a aparecer** aunque ya se haya elegido
   «Permitir y confiar».
4. Probado en 720p, 1080p y Nativa.

## 2. Evidencia

### 2.1 El diálogo reaparece: un prompt por **intento de conexión**

El cliente abre **varios** intentos (QUIC → TCP → relay; `quic_fallback` +
`relay_fallback`) y **cada intento** abre su propio `session_pending` en el
anfitrión → un diálogo por intento:

```
11:31:00 [daemon] sesión entrante de ONDH6IU4RU — esperando tu decisión
11:31:23 [ui]    decisión para ONDH6IU4RU: trust · con control
11:33:05 [daemon] sesión entrante de ONDH6IU4RU — esperando tu decisión
11:33:30 [ui]    decisión para ONDH6IU4RU: trust · con control
11:37:22 [daemon] sesión entrante de ONDH6IU4RU — esperando tu decisión
11:37:30 [daemon] cliente rechazado desde 192.168.6.150:59953: handshake_timeout (45s)
11:37:38 [daemon] sesión entrante de ONDH6IU4RU — esperando tu decisión
11:37:59 [ui]    decisión para ONDH6IU4RU: trust · con control
```

El timeout de 45 s = `handshake_timeout()` = 15 s (handshake) + 30 s
(`--accept-timeout`). El cliente **ya había agotado su cadena de intentos**
antes de que el usuario decidiera → el diálogo correspondía a un intento
muerto.

### 2.2 El vídeo ahoga el enlace y con él el control

Anfitrión (5 s por bloque, `entregados` = framesWrite canal de control):

```
[video] fps (5 s): captura=10 entraron=13 emitidos=13 entregados=5  · 2802 kbps 131.5 KB/frame 0.477 bpp
[video] fps (5 s): captura=6  entraron=10 emitidos=10 entregados=0  ·  507 kbps  31.0 KB/frame 0.112 bpp
[video] fps (5 s): captura=10 entraron=14 emitidos=14 entregados=14 · 3134 kbps 136.6 KB/frame 0.496 bpp
[video] fps (5 s): captura=2  entraron=7  emitidos=7  entregados=0  ·  383 kbps  33.4 KB/frame 0.121 bpp
```

Cliente (su propio contador):

```
11:38:47 visor: fps render=3.9 · 11 frames · 3840x2160 · recv=11 render=11 drop=0 coste=47.7ms
11:39:22 visor: fps render=0.0 · 15 frames · 3840x2160 · recv=15 render=15 drop=0 coste=42.9ms
11:39:52 visor: fps render=0.0 · 29 frames · 3840x2160 · recv=29 render=29 drop=0 coste=45.1ms
```

**16 frames en 60 s** (0,3 fps) con `drop=0`: el cliente no pierde frames, no
**llegan**. La sesión envió 15,5 MB en ~2 min ≈ **1 Mbps**, mientras el encoder
pedía 14–25 Mbps.

### 2.3 El input sí se inyecta, pero con latencia

La auditoría del anfitrión cuenta **244 `input_injected`** (mouse/teclado) y
`permission_granted{keyboard,mouse,audio}`: el permiso estaba concedido y los
eventos se inyectaron. Lo que falla es la **latencia**: en modo TCP el vídeo y
el control comparten el mismo socket, así que los eventos de entrada quedan
**encolados detrás de los frames de vídeo** de un pipe saturado → «no se
puede seleccionar ni mover» y la escritura no aparece.

## 3. Defectos (3)

| # | Defecto | Dónde | Consecuencia |
|---|---|---|---|
| **P1** | El piso de bpp se aplicó como **exigencia de bitrate** al encoder, ignorando lo que el enlace entrega: a 1386×780@15 pide **13,8 Mbps** (a 4K, 56 Mbps) sobre un enlace de ~1 Mbps | `core/daemon/src/video.rs:342-368` (tasa inicial = piso), `:370-417` (`floor_bps_of`, `size_bps`) | Pipe saturado → control starving → app inutilizable |
| **P2** | La escalera **oscila**: bpp 0,477 → 0,112 → 0,610 → 0,496 → 0,121 → 0,734 en 30 s (subidas y bajadas encadenadas, `NET_RECOVER=3` + zona neutra contando) | `video.rs:493-539`, `:562-580` | Recodificación constante → más tráfico → más saturación |
| **P3** | Un prompt por **intento de conexión** y timeout de handshake de 45 s | `core/daemon/src/lib.rs` (`admit_peer`) + `core/daemon/src/lib.rs` (`handshake_timeout`) | El diálogo reaparece; el usuario decide sobre intentos ya muertos |

**Lo que sí funcionó de C-52**: la escalera **bajó la resolución** (4K →
1386×780, el `MIN_LADDER_SIZE`/50 % acted) y el render del cliente a 1386×780
vuelve a 10 ms. El error no es la idea (bits por píxel), es **cómo se le pide
el bitrate**.

## 4. Corrección propuesta (C-52 Fase B2)

1. **El objetivo nunca supera lo que el enlace entrega**: `objetivo = min(piso_bpp(tamaño),
   tasa_entregada_EWMA)`; y se **baja la resolución** hasta que el piso quepa
   en el presupuesto disponible (no se pide más bitrate del que hay).
2. **Anti-oscilación**: histeresis entre cambios de escala y una sola subida
   cada N muestras buenas.
3. **En TCP, descartar frames de vídeo** en vez de encolarlos (máx. N en vuelo):
   el control no puede esperar detrás de megabytes de vídeo.
4. **Un prompt por par** y decisión reutilizada entre los intentos del mismo
   cliente; timeout de handshake corto (no 45 s).

## 5. Reproducción mínima (para el próximo ciclo)

Sesión por relay con el host en Nativa y el cliente en 1080p; dejar 60 s y leer:
`[video] fps (5 s)` del anfitrión (3 bloques) y `visor: fps render` del cliente
(3 líneas). El guion de 2 min de `evidence/video-quality-regression.md` §7.2
sigue siendo válido; **añadir** el conteo de frames entregados y la latencia
de un input.
