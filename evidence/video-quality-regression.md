# C-52 · Fase A — regresión de calidad de vídeo/texto (campo 2026-11-20)

| Campo | Valor |
|---|---|
| Fecha | 2026-11-21 |
| Rama | `feature/c52-video-regression` (desde `develop` @ `bee2cc3`) |
| Alcance | **Fase A** — causa raíz con evidencia + gates de la Fase B. **Sin** cambios de producción (solo `#[cfg(test)]` en `core/daemon/src/video.rs`) |
| Semilla | `evidence/field-2026-11-20.md` (sesión Julie `ONDH6IU4RU` → Brian `GLV5Q76BAQ`) |
| Logs | Anfitrión `C:\Users\wnuke\AppData\Roaming\anyremote\ui.log` · Cliente `E:\audiany\anyremote\ui.log` |
| Estado | ✅ causa raíz sustentada · ⏳ Fase B pendiente · ⏳ gate de campo pendiente |

---

## 1. Resumen (tl;dr)

La sesión del 20/11 tuvo **dos** partes y el log del anfitrión las deja
separadas. La semilla (`field-2026-11-20.md`) citaba la primera; el log
muestra que **la degradación sin recuperación es la segunda**.

| Sesión | Ventana | Qué pasó |
|---|---|---|
| **S1** (cliente 0.14.1) | 08:59:39 → 09:00:37 | La escalera **subió** a 15 Mbps (3 recuperaciones). El bpp real osciló **0,026 – 0,470** (media 0,266) y el texto quedó ilegible. |
| **S2** (cliente 0.14.3) | 09:52:13 → 09:57:07 | La escalera **cayó** a 200 kbps @ 1001×563 (bpp **0,003 – 0,096**) y **no volvió a subir en 2 min 35 s** (09:54:32 → 09:57:07, sin una sola decisión). |

**Causa raíz (una frase):** el control adaptativo de calidad gobierna dos
magnitudes (**bitrate** y **escala**) que *no* son la magnitud que el usuario
percibe (**bits por píxel del contenido ya codificado**), y ninguna de las dos
acciones **recodifica** el contenido estático: por eso el texto se degrada
hasta ~100× por debajo del piso de legibilidad y no se recupera.

Tres defectos concretos, cada uno con su línea y su evidencia:

| # | Defecto | Dónde | Evidencia de campo |
|---|---|---|---|
| **D1** | La escalera **no tiene suelo de bpp**: sus escalones están por debajo del piso de texto y su suelo (200 kbps) está ~70× por debajo | `core/daemon/src/video.rs:302` (tasa inicial `w*h*3`), `:324-327` (`size_bps` = 1 bit/px), `:311` (`min_bps` = 200 kbps) | S2: 1001×563 @ 200 kbps → 0,003 bpp (`ui.log:5168` + bloque 09:54:54 → 09:57:05) |
| **D2** | La **recuperación es inalcanzable** con el jitter del relay: la zona neutra (100–200 ms) sólo *degrada* `net_low` y la recuperación exige 5 muestras sanas + cooldown de 20 s | `core/daemon/src/video.rs:426`, `:467`, `:268` | S2: 2 min 35 s (155 muestras) sin una sola decisión tras llegar al suelo |
| **D3** | Al **subir** el bitrate **no se pide IDR**: el texto estático conserva la calidad del último IDR; sólo lo que cambia sigue la tasa nueva | `core/daemon/src/lib.rs:65-76` (el trait no tiene método de refresco), `core/codec/src/mf_backend.rs:1090-1127` (sólo escribe Mean/MaxBitRate) | S1: el bpp del bloque cayó a 0,030 y 0,026 bpp con 15 Mbps de objetivo (`ui.log:4713`, `:5052`) |

> El orden importa: **D1 es la causa próxima** (el texto es ilegible porque
> el bpp cae muy por debajo del piso), **D2 explica el «no se recuperó»** y
> **D3 explica por qué «subir el tope» no devuelve la calidad** (y por qué el
> movimiento se veía fluido mientras el texto no).

---

## 2. Reproducción (con los números del log)

### 2.1 Configuración de la sesión (anfitrión Brian)

```
ui.log:4415  listener de red lanzado (… — vídeo 1080p@15 h264 …)
ui.log:4416  --- listen args: listen --bind 0.0.0.0:21118 --transport udp
                  --rdv 51.222.81.81:21120 --video on --video-size 1920x1080
                  --video-fps 15 --max-bitrate 15000 --codec h264 …
ui.log:4433  [video] h264 cfg: 1920x1080@15 'NVIDIA H.264 Encoder MFT'
                  profile=77 bitrate=6220800 [("rc_cbr",true),("mean",true),
                  ("max",true),("gop",true),("bframes0",false),("cabac",true)]
```

- Tasa inicial **6 220 800 bps** = `1920×1080×3` → `core/daemon/src/video.rs:302`.
  A 1080p@15 son **0,200 bpp** (por debajo del piso de 0,4–0,5 de C-32).
- Tope CLI **15 000 000 bps** (`--max-bitrate 15000`) → 0,482 bpp a 1080p@15.
- Backend **NVENC por MFT** (no el software): C-35 activo.

### 2.2 S1 — la escalera sube y el texto no (08:59:39 → 09:00:37)

| Hora | `ui.log` | captura/entraron/emitidos | kbps | KB/frame | **bpp** | tasa objetivo |
|---|---|---|---|---|---|---|
| 08:59:45 | 4435 | 4 / 6 / 6 | 548 | 55,7 | 0,220 | 6 220 800 (0,200) |
| 08:59:46 | 4436 | *`red_recupera_bitrate → @9331200 · rtt 1ms`* | | | | 9 331 200 (0,300) |
| 08:59:50 | 4437 | 4 / 9 / 8 | 839 | 64,0 | 0,253 | 9 331 200 |
| 08:59:56 | 4438 | 1 / 6 / 6 | 1027 | 104,5 | 0,413 | 9 331 200 |
| 08:59:57 | 4439 | *`red_recupera_bitrate → @13996800 · rtt 77ms`* | | | | 13 996 800 (0,450) |
| 09:00:01 | 4443 | 10 / 12 / 12 | 2341 | 119,0 | 0,470 | 13 996 800 |
| 09:00:04 | 4480 | *`red_recupera_bitrate → @15000000 · rtt 86ms`* | | | | 15 000 000 (0,482) |
| 09:00:06 | 4562 | 14 / 15 / 15 | 2465 | 100,3 | 0,396 | 15 000 000 |
| 09:00:11 | 4628 | 18 / 19 / 19 | 1403 | 45,1 | 0,178 | 15 000 000 |
| 09:00:17 | 4713 | 6 / 9 / 9 | **111** | **7,5** | **0,030** | 15 000 000 |
| 09:00:22 | 4714 | 1 / 6 / 6 | 1141 | 116,1 | 0,459 | 15 000 000 |
| 09:00:27 | 4855 | 9 / 11 / 11 | 1341 | 74,4 | 0,294 | 15 000 000 |
| 09:00:32 | 5052 | 8 / 11 / 11 | **120** | **6,6** | **0,026** | 15 000 000 |

Lecturas:

1. **La red estaba bien**: el `rtt` que imprime el log es el **exceso** sobre
   el mínimo de la sesión (`core/daemon/src/lib.rs:1924` →
   `net_excess`, `core/daemon/src/video.rs:260`), y fue **1 / 77 / 86 ms**.
   El enlace no estaba saturado → **H3 (baseline del relay) queda DESCARTADA**.
2. **El objetivo de bitrate NO se traduce en calidad**: con 15 Mbps de
   objetivo el bloque de 09:00:17 produjo **111 kbps** (0,7 % del presupuesto)
   y el de 09:00:32 **120 kbps** (0,8 %). El control de tasa de NVENC es un
   **techo**, no un suelo: con la pantalla quieta el códec no gasta el
   presupuesto (los P-frames se saltan lo que no cambia).
3. **Media de la sesión = 0,266 bpp** (6 917,7 KB / 103 frames / 2 073 600 px
   × 8) — por debajo del piso de texto de C-32 (`evidence/text-quality-gate.md:21`).
4. **El visor no perdió nada**: el anfitrión emitió 103 frames; el cliente
   recibió `recv=107 … drop=0` (`E:\audiany\anyremote\ui.log:2309`) y el
   `coste` de render fue 12–20 ms (`fps render=3.6…14.3`). **Ni el
   transporte ni el render participan del defecto.**
5. El cliente **amplió al 150 %** (`caja=2880x1620`, `ui.log:2306`): amplifica
   un origen ya degradado → el texto se ve peor, no mejor.

### 2.3 S2 — la escalera cae y no vuelve (09:52:13 → 09:57:07)

```
ui.log:5117  recupera_bitrate → 1920x1080 @15000000 bps        (09:52:34)
ui.log:5118  red_bitrate    → 1920x1080 @7500000  bps · rtt 529ms (09:52:38)
ui.log:5126  red_bitrate    → 1920x1080 @3750000  bps · rtt 275ms (09:53:13)
ui.log:5128  red_bitrate    → 1920x1080 @1875000  bps · rtt 234ms (09:53:17)
ui.log:5130  red_bitrate    → 1920x1080 @937500   bps · rtt 230ms (09:53:25)
ui.log:5132  red_bitrate    → 1920x1080 @468750   bps · rtt 794ms (09:53:28)
ui.log:5134  red_bitrate    → 1920x1080 @234375   bps · rtt 227ms (09:53:32)
ui.log:5135  red_bitrate    → 1920x1080 @200000   bps · rtt 296ms (09:53:35)
ui.log:5137  red_escala     → 1632x918                          (09:53:38)
ui.log:5138  [video] h264 cfg: 1632x918@15 … bitrate=1498176   (→ IDR nuevo)
ui.log:5141  red_bitrate    → 1632x918 @749088  bps · rtt 267ms (09:53:44)
ui.log:5143  red_bitrate    → 1632x918 @374544  bps · rtt 265ms (09:53:50)
ui.log:5146  red_bitrate    → 1632x918 @200000  bps · rtt 237ms (09:53:58)
ui.log:5148  red_escala     → 1387x780                          (09:54:03)
ui.log:5149  [video] h264 cfg: 1386x780@15 … bitrate=1081860   (→ IDR nuevo)
ui.log:5151  presion_bitrate→ 1387x780 @540930  bps           (09:54:04)
ui.log:5152  red_bitrate    → 1387x780 @270465  bps · rtt 286ms (09:54:06)
ui.log:5154  red_bitrate    → 1387x780 @200000  bps · rtt 294ms (09:54:10)
ui.log:5156  red_escala     → 1178x663                          (09:54:13)
ui.log:5157  [video] h264 cfg: 1178x662@15 … bitrate=800000    (→ IDR nuevo)
ui.log:5159  red_bitrate    → 1178x663 @400000  bps · rtt 205ms (09:54:18)
ui.log:5161  red_bitrate    → 1178x663 @200000  bps · rtt 232ms (09:54:21)
ui.log:5163  red_escala     → 1001x563                          (09:54:24)
ui.log:5167  red_bitrate    → 1001x563 @400000  bps · rtt 234ms (09:54:28)
ui.log:5168  red_bitrate    → 1001x563 @200000  bps · rtt 329ms (09:54:32)
…           (sin ninguna decisión hasta el cierre de 09:57:07)
```

Bloques de vídeo tras el suelo (bpp real medido, `ui.log:5160…5440`):

```
09:54:54  0.028 bpp (1001x563)      09:55:31  0.022 bpp
09:55:00  0.006 bpp                 09:55:36  0.022 bpp
09:55:05  0.024 bpp                 09:55:41  0.008 bpp
09:55:10  0.076 bpp                 09:56:23  0.096 bpp
09:55:20  0.007 bpp                 09:56:55  0.020 bpp
…                                         (2 min 35 s ≈ 155 muestras)
```

**El texto a 0,003–0,096 bpp es ilegible por definición** (C-32 pidió 0,4–0,5).
Y **no hubo ninguna decisión durante 2 min 35 s** → la calidad quedó
congelada en el suelo hasta el cierre.

### 2.4 El cliente también lo vio (E:\audiany\anyremote\ui.log)

```
:2331 09:52:33  visor: fps render=16.3 · 41 frames · 1920x1080 · caja=800x450
:2332 09:52:38  visor: fps render=8.8  · 70 frames · 1920x1080 · caja=1383x778 (escala 72 %)
:2343 09:53:38  visor: fps render=15.5 · 324 frames · 1178x662 · caja=1767x993 (150 %)
:2344 09:53:43  visor: fps render=19.5 · 350 frames · 1178x662
:2345 09:53:48  visor: fps render=13.1 · 383 frames · 1632x918
:2346 09:53:53  visor: fps render=22.8 · 435 frames · 1632x918
…
:2378 09:56:38  visor: fps render=13.7 · 885 frames · 1000x562 · caja=801x450
:2385 09:57:03  visor: fps render=10.9 · 1002 frames · 1000x562
```

El tamaño del frame que recibe el cliente **baja con la escalera** y el
render sigue fluido (`drop=0`, 7–25 ms) → el defecto es de **calidad del
contenido**, no de entrega ni de pintado.

---

## 3. Causa raíz

### 3.1 D1 — la escalera no tiene suelo de bpp (causa próxima)

`AdaptState` razona en **bitrate** y en **escala**, nunca en bits por píxel:

| Punto | Fórmula | bpp a 1080p@15 | Nota |
|---|---|---|---|
| Tasa inicial | `w*h*3` (`video.rs:302`) | **0,200** | por debajo del piso 0,45 |
| Tope CLI (`--max-bitrate 15000`) | 15 000 000 | **0,482** | justo en el piso |
| Tras un paso de escala | `size_bps` = `máx(w*h, 4×suelo)` (`video.rs:324-327`) | 0,095 @1001×563 | 5× por debajo |
| Suelo de la escalera | `min_bps` = 200 000 (`video.rs:311`) | **0,006** @1080p · **0,003** @1001×563 | ~70× por debajo |

El suelo de la escalera está definido en **bitrate** (200 kbps) y en
**escala** (50 % del tamaño inicial, `video.rs:357-358`), no en la unidad
que decide si el texto se lee. Por eso la escalera puede —y en S2 lo hizo—
llegar a un estado **70–150× por debajo** del piso de legibilidad sin que
ninguna regla lo prohíba.

> Consecuencia adicional: el techo de 15 Mbps **no** alcanza para 4K
> (0,121 bpp a 3840×2160@15). Es el mismo problema que C-32 documentó
> («el cauce real es la resolución»), y por eso la Fase B debe **acoplar
> resolución ↔ presupuesto** y no sólo subir el bitrate.

### 3.2 D2 — la recuperación es inalcanzable con el jitter del relay

`observe_net` (`video.rs:414-489`):

- congestión: `rtt_exceso ≥ 200 ms` **3 veces** → `net_high = 3` → baja el
  bitrate 50 % (o la escala si ya está en el suelo) y abre un **cooldown de
  20 s** (`video.rs:266-268`);
- sanitizer: `rtt_exceso ≤ 100 ms` **5 veces** (`NET_RECOVER`) **y** cooldown
  agotado → recupera escala y luego bitrate (`video.rs:467-487`);
- **zona neutra (100–200 ms)**: sólo *degrada* `net_low` en 1 (`video.rs:426`),
  no lo borra (fix de C-A1) pero tampoco lo deja crecer.

Con el relay, el `rtt_min` de la sesión fue **283 ms** (cliente,
`E:\audiany\anyremote\ui.log:2312`/`:2386`) y el exceso real se mantuvo entre
**205 y 794 ms** durante todo el colapso. Una serie que oscila entre la zona
neutra y la de congestión **nunca** acumula 5 muestras sanas seguidas →
**la recuperación no se alcanza** y, en el suelo, no hay decisión posible
(`video.rs:449-451`: el suelo de escala bloquea y el de bitrate ya está
tocado).

**Consecuencia:** el usuario vio «se degradó y no se recuperó» porque la
escaleraDown quedó **congelada 2 min 35 s** en el peor estado.

### 3.3 D3 — subir el bitrate no recodifica el contenido estático

Hay **asimetría** entre las dos acciones de la escalera:

| Acción | Qué hace el código | ¿Recodifica lo estático? |
|---|---|---|
| **Cambiar la tasa** | `encoder.set_target_bitrate(bps)` (`video.rs:787-797`) → `mf_backend.rs:1090-1127` escribe `CODECAPI_AVEncCommonMeanBitRate` y `…MaxBitRate` | **NO**: el trait `VideoEncoder` (`core/codec/src/lib.rs:65-76`) **no tiene ningún método de refresco**; sólo cambia el *techo* del control de tasa |
| **Cambiar la escala** | `video.rs:800-828` **reconstruye el encoder** (`encoder_for(codec)`) → nuevo `h264 cfg` + nuevo `h264 framing` (IDR) | **SÍ**, pero **a la calidad nueva (baja)** → el texto se destruye de golpe (`ui.log:5138`, `:5149`, `:5157`) |

Y el único refresco «programado» es el **GOP de 2 s**
(`CODECAPI_AVEncMPVGOPSize = fps*2 = 30` entradas, `mf_backend.rs:285-287`):
con pantalla estática el bucle sólo entrega ~1 entrada/s (reenvío a 1 Hz,
`reemit_gap`, `video.rs:508-514`) → **el IDR llega cada ~30 s**, no cuando la
tasa sube.

Efecto observado, coherente con el reporte («mover una ventana se hacía
fluido, pero con la imagen degradada»):

- lo que **cambia** se recodifica a la tasa nueva → movimiento fluido;
- lo que **no cambia** conserva la calidad del último IDR → texto borroso.

El arnés lo cuantifica: con el tope a 15 Mbps, la región estática se queda en
**0,200 bpp** (la del primer IDR) mientras la región en movimiento sube a
**0,482 bpp**.

---

## 4. Hipótesis de la semilla: qué se confirma y qué se descarta

| Hipótesis | Veredicto | Dato |
|---|---|---|
| **H1** bpp insuficiente | ✅ **CONFIRMADA (causa próxima)** | 0,266 bpp de media en S1; 0,003–0,096 bpp en S2; el piso de C-32 es 0,4–0,5 (`evidence/text-quality-gate.md:21`) |
| **H2** no se refresca el frame estático al subir el bitrate | ✅ **CONFIRMADA (mecanismo y magnitud)** | Sin IDR en el camino de tasa (`core/codec/src/lib.rs:65-76`, `mf_backend.rs:1090-1127`); el GOP de 30 entradas llega cada ~30 s en estático (`mf_backend.rs:285-287`, `video.rs:508-514`) |
| **H3** baseline del relay | ❌ **DESCARTADA como causa** | El log imprime el **exceso**, no el absoluto: 1/77/86 ms en S1 (`lib.rs:1924`, `video.rs:260`). El relay no fuerce la cascada en S1. En S2 el exceso **sí** fue alto (205–794 ms) → es un episodio real de congestión, no un problema de baseline (el baseline se aplicó bien: `rtt_min=283/351 ms`) |
| **H4** calentamiento inicial del encoder | ❌ **DESCARTADA como causa** | `encode_ms=265,8 ms` sólo en el primer bloque de S1 (`ui.log:4435`); desde el segundo bloque es 7,7–16,2 ms. En S2 el primer bloque fue 228,1 ms (`ui.log:5111`) y el resto 7,3–13,8 ms. El calentamiento dura un bloque; la degradación dura toda la sesión |
| **Extra** el transporte o el render | ❌ **DESCARTADOS** | 103 emitidos = 107 recibidos, `drop=0`, coste 7–25 ms (`E:\audiany\anyremote\ui.log:2303-2385`) |

---

## 5. Arnés de simulación determinista (solo tests)

Añadido en `core/daemon/src/video.rs` (módulo `#[cfg(test)] mod c52_ladder`,
líneas 1172+). **No toca producción**: usa la política real (`AdaptState`) y
un modelo del códec entre frames.

| Test | Qué prueba | Resultado |
|---|---|---|
| `c52_field_ladder_is_reproduced_by_the_policy` | La escalera de S1 (9 331 200 → 13 996 800 → 15 000 000) la produce la política real con RTT sano | ✅ |
| `c52_session2_collapse_is_reproduced_by_the_policy` | Las **21 decisiones** de S2 (razón, tasa y tamaño exactos) las reproduce la política real con la serie de RTT observada | ✅ |
| `c52_collapse_does_not_recover_under_neutral_jitter` | 155 muestras en zona neutra tras el suelo → **cero decisiones** (el «no se recuperó» de 2 min 35 s); con 5 muestras sanas sí recupera | ✅ |
| `c52_no_rung_of_the_ladder_sustains_the_text_bpp_floor` | **D1**: ningún escalón sostiene 0,45 bpp; el suelo queda 70× por debajo | ✅ (caracteriza el defecto) |
| `c52_static_text_is_not_refreshed_by_the_recovery` | **D3**: al subir a 15 Mbps el texto estático sigue en 0,200 bpp; la zona en movimiento sube a 0,482; el texto sólo se refresca en el GOP (30 entradas) o con un IDR forzado | ✅ (caracteriza el defecto) |

**Qué NO prueba el arnés** (limite honesto): no modela el H.264 bit a bit;
no reproduce el GOP real de NVENC (el log **no** dice qué frames fueron IDR);
no reproduce el escalado 4K→1080p ni el consumo del visor. Para esas dos
zonas hacen falta las mediciones M2/M4 del §7.

**Gates de la Fase B que este arnés deja definidos y hoy fallan** (los tests
se invierten):

- `ladder_preserves_bpp` → «todo escalón de la escalera sostiene ≥ 0,45 bpp
  para el tamaño activo» (hoy: escalón inicial 0,200).
- `static_text_refreshes_after_recovery` → «tras una recuperación, el
  contenido estático se recodifica a la tasa nueva» (hoy: no hay ni el
  mecanismo en el contrato del códec).

---

## 6. Gates ejecutados en esta Fase A (resultado real)

| Gate | Comando | Resultado |
|---|---|---|
| Tests del workspace | `cd core && cargo test --workspace --no-fail-fast` | **177 passed / 0 failed** (172 previos + 5 nuevos) |
| Clippy | `cd core && cargo clippy --workspace --all-targets -- -D warnings` | **0 avisos / 0 errores** |
| Formato | `cd core && cargo fmt --all -- --check` | **0 diferencias** |
| Flutter | — | **no aplica**: esta fase no toca `app/` |

No se reconstruye el binario: la Fase A **no cambia comportamiento de
producción** (D-13c aplica a los ciclos cerrados; aquí sólo hay tests).

---

## 7. Qué falta medir (gate de campo) y qué necesita el usuario

### 7.1 Mediciones que el log actual **no** tiene

| # | Falta | Por qué importa | Cómo se mide |
|---|---|---|---|
| **M1** | La **serie de RTT por muestra** (1/s), no sólo el valor en cada decisión | Distingue «jitter que bloquea la recuperación» (D2) de «las muestras dejaron de llegar» | Registrar el exceso en cada `observe_net` (tasaado) |
| **M2** | La **posición de los IDR** (qué frames son clave) | Cuantificar D3: cuándo se recodificó el texto por última vez | Un contador `idr`/`keyframe` por frame en el log de 5 s |
| **M3** | El **bpp por región** (estática vs en movimiento) | El bpp global mezcla frames IDR completos con P-frames de salto | Separar `emitidos` por tipo (IDR / P) |
| **M4** | La **resolución nativa de la captura** | C-31/C-32 dicen que el escritorio de Brian es 4K: a 1080p el texto pasa de 16 px a 8 px — un factor independiente del bitrate | Log de la resolución capturada junto a `--video-size` |

### 7.2 Guion de campo para el usuario (≈2 min, §13.1 Fase C)

1. Amy/Julie abre sesión **por relay** (mismo VPS `51.222.81.81:21120`) y
   Brian comparte su escritorio **4K**.
2. Con la pantalla **quieta**, anotar 3 líneas de `[video] fps (5 s)` del
   anfitrión y 3 de `visor: fps render` del cliente.
3. **Mover una ventana** 10 s y volver a la quietud; anotar otras 3+3.
4. **Subir el tope** con `--max-bitrate` (o dejar que la escalera suba) y
   anotar si el texto mejora (no debería, hasta la Fase B).
5. Pegar el guion y las líneas en `evidence/c52-field.md`.

Con M1–M4 en el log, la Fase B (y el gate de campo) tienen datos para decidir
el umbral de bpp por tamaño y la estrategia de refresco.

---

## 8. Plan de la Fase B (propuesta, para el usuario)

Objetivo: que la calidad **percibida** (bpp del contenido estático) sea la
magnitud gobernada, con refresco al recuperar.

| Pieza | Cambios previstos | Riesgo |
|---|---|---|
| **B1 · Suelo de bpp por tamaño** | La escalera elige la **escala** que sostiene un bpp mínimo (≥0,45) con la tasa disponible; deja de poder bajar a 200 kbps @ 1080p | Toca C-27/C-29 (misma `AdaptState`) |
| **B2 · Tasa inicial con presupuesto de texto** | La tasa inicial deja de ser `w*h*3` (0,200 bpp) y pasa a derivarse del bpp mínimo | Regresión en C-32/C-33 |
| **B3 · Refresco (IDR) al recuperar** | Nuevo método en `VideoEncoder` (`force_keyframe`) + `CODECAPI_AVEncVideoForceKeyFrame` (disponible en `windows` 0.61) + llamada en la rama de tasa | Toca C-35/C-36 (misma ruta de encoder); riesgo de "calentamiento" del MFT |
| **B4 · Recuperación alcanzable con jitter** | La zona neutra debe permitir recuperar (hoy sólo degrada) sin oscilación | Toca C-27/C-29 |
| **B5 · Instrumentación** | IDR por bloque, RTT por muestra, resolución nativa | Sin riesgo |

**Gates de la Fase B** (falsables, con comando):

| Gate | Qué exige | Comando |
|---|---|---|
| `ladder_preserves_bpp` | Todo escalón de la escalera sostiene ≥0,45 bpp para el tamaño activo; el suelo no baja de ese bpp | `cd core && cargo test -p anyremote-daemon --lib ladder_preserves_bpp` |
| `static_text_refreshes_after_recovery` | Tras una recuperación, el encoder recibe una petición de refresco y el contenido estático se recodifica a la tasa nueva | `cd core && cargo test -p anyremote-daemon --lib static_text_refreshes_after_recovery` |
| `recovery_reachable_under_jitter` | Una serie con jitter en la zona neutra recupera en ≤ N muestras | `cd core && cargo test -p anyremote-daemon --lib recovery_reachable_under_jitter` |
| Regresión C-27/C-29/C-32/C-35/C-36 | Los tests de esos ciclos siguen verdes | `cd core && cargo test --workspace --no-fail-fast` |
| Clippy + fmt | 0 avisos | `cd core && cargo clippy --workspace --all-targets -- -D warnings` · `cargo fmt --all -- --check` |
| Binario verificado | `VERSION` = `app/pubspec.yaml` = `core/daemon/Cargo.toml` y build | `powershell -ExecutionPolicy Bypass -File scripts\build-release.ps1` |

> **Nota de alcance (D-12):** B1–B5 son **un solo frente** (calidad de
> vídeo). No se mezclan con seguridad (C-46…C-51) ni con teclado (C-54/C-55).

---

## 9. Trazabilidad

- Semilla: `evidence/field-2026-11-20.md` · Contexto de RE: ninguno nuevo
  (no hay hallazgos H-AD/H-RD en esta fase).
- Ciclos previos que **no** se reabren a ciegas pero sí se tocan:
  C-27 (`net_excess` y la política de RTT), C-29 (escalera y recuperación),
  C-32 (presets de bitrate y piso de bpp del texto), C-35 (encoder NVENC y
  rate-control), C-36 (escalado paralelo). Sus tests deben seguir verdes.
- `specs/02` §13.1 (fases/gates) y `specs/04` (fila C-52) actualizados en
  este mismo commit.
