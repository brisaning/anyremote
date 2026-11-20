# C-52 · Fase B — piso de bpp, refresco (IDR) y recuperación con jitter

| Campo | Valor |
|---|---|
| Fecha | 2026-11-21 |
| Rama | `feature/c52-video-regression` (Fase A ya commiteada: `881552a`) |
| Alcance | **Fase B** de C-52 (calidad de vídeo): D1 (piso de bpp), D3 (refresco), D2 (recuperación con jitter) |
| Semilla / causa raíz | `evidence/video-quality-regression.md` (Fase A) |
| Diseño | `specs/02` §13.1 (Fase B y gates falsables) |
| Decisiones del usuario | suelo **0,45 bpp** · IDR al **recuperar** y al **cambiar de escala** · recuperación **N=3** con la zona neutra contando |
| Estado | ✅ implementado · ✅ gates verdes · ⏳ gate de campo (Fase C) · ⏳ binario (lo cierra el orquestador) |

---

## 1. Qué cambió (producción)

| Punto | Archivo | Qué |
|---|---|---|
| **D1 · piso de bpp** | `core/daemon/src/video.rs:272-291` | `TEXT_BPP_FLOOR = 0.45`, `MIN_LADDER_SIZE = 640×360`, `bpp_floor_bps(w,h,fps)` |
| | `video.rs:342` | `AdaptState::with_fps(size, max_bps, fps)`; `new` delega con fps de referencia 15 |
| | `video.rs:372-387` | `floor_bps_of` / `floor_bps` / `size_bps` expresados en bpp (antes `w*h` y 200 kbps) |
| | `video.rs:393` | `ladder_floor_size`: 50 % del inicial **o** 640×360 (el que sea mayor, sin pasar el inicial) |
| | `video.rs:402` | `scale_down(reason)`: escalón único con el piso del tamaño nuevo (elimina la duplicación de la rama de presión y la de red) |
| | `video.rs:429-441` | presión: el bitrate baja **hasta el piso**; en el piso, escala |
| | `video.rs:509-520` | congestión: ídem (el antiguo suelo de 200 kbps desaparece) |
| **D2 · recuperación** | `video.rs:272` | `NET_RECOVER = 3` (antes 5 sanas) |
| | `video.rs:497-506` | la zona neutra (100–200 ms) **suma** a `net_low` (antes restaba); `net_high` no se toca |
| | `video.rs:530-536` | la recuperación abre ventana nueva (`net_high = 0`) para no re-colapsar con evidencia vieja; cooldown de 20 s intacto |
| **D3 · refresco** | `core/codec/src/lib.rs:76-85` | `VideoEncoder::request_refresh() -> bool` con default `false` |
| | `core/codec/src/mf_backend.rs:1169-1185` | MF: `IsSupported(CODECAPI_AVEncVideoForceKeyFrame)` + `refresh_pending` |
| | `mf_backend.rs:796-812` | `arm_refresh` antes del `ProcessInput` de la entrada que consume el IDR y `clear_refresh` después (evita que el MFT quede en «todo IDR») |
| | `mf_backend.rs:973-981`, `1067-1081` | enganche en el camino asíncrono (NVENC) y en el síncrono |
| | `core/codec/src/vpx_backend.rs:167-175` | VP9: `force_next_key` latched; `vpx_backend.rs:103-107` lo consume en `force_keyframe` de libvpx |
| | `video.rs:562-580` | `decision_needs_refresh` + `apply_target_bps`: pide refresco **solo** en `recupera_bitrate`/`red_recupera_bitrate` y registra `[video] refresco solicitado: motivo=recuperacion (aceptado=…)` |
| | `video.rs:918-927` | cambio de escala: **no** se pide un segundo IDR (la reconstrucción ya lo produce) pero se registra `[video] refresco solicitado: motivo=escala (idr por reconstruccion)` |
| | `video.rs:799` | el estado adaptativo se crea con el fps real de la sesión |

**Verificación del «no dupliques» en escala** (pedida por el diseño): al aplicar
`video_size`, `video.rs:895-905` hace `encoder = encoder_for(codec)` → MFT sin
configurar (el primer frame emite SPS/PPS + IDR) o `Vp9Encoder` nuevo con
`since_key == 0` (`force_keyframe = true` en su primer frame). Un refresco
adicional solo añadiría un IDR redundante en el peor momento del enlace.

---

## 2. Números: antes → después (replay de campo, 1920×1080@15)

| Estado | Campo 20/11 (S2) | Política C-52 |
|---|---|---|
| Tasa inicial | 6 220 800 (`w*h*3` = 0,200 bpp) | **13 996 800** (0,450 bpp) |
| Escalón 1 (1632×918) | 749 088 (0,033 bpp) | **10 112 688** (0,450 bpp) |
| Escalón 2 (1387×780) | 540 930 (0,019 bpp) | **7 302 555** (0,450 bpp) |
| Escalón 3 (1178×663) | 270 465 (0,009 bpp) | **5 271 845** (0,450 bpp) |
| Suelo (1001×563 = 50 % del inicial) | 200 000 (**0,024 bpp** de objetivo; medido 0,003–0,096) | **3 804 050** (0,450 bpp) |
| Recuperación tras el suelo | ninguna en 2 min 35 s | 1 escalón de escala a los **20 s** (cooldown) y al tope a los **23 s** con jitter neutro |

El suelo de escala sigue siendo el mismo que en campo (1001×563, 50 % del
inicial): lo que cambia es que ese tamaño ya no puede quedar a 200 kbps.

---

## 3. Tests (arnés `mod c52_ladder` y regresión de ciclos anteriores)

| Test | Qué afirma ahora | Antes (Fase A) |
|---|---|---|
| `ladder_preserves_bpp` | **gate D1**: todo escalón (arranque, bitrate, escala, presión de CPU) sostiene ≥0,45 bpp del tamaño activo; con un tope que no sostiene el piso de 1080p (2 Mbps) la política **baja la escala** en vez del bitrate | `c52_no_rung_of_the_ladder_sustains_the_text_bpp_floor` (afirmaba el defecto) |
| `static_text_refreshes_after_recovery` | **gate D3**: el camino real (`apply_target_bps`) pide refresco al recuperar (espía de `VideoEncoder`), no en las bajadas ni en la escala; el modelo recodifica el texto a 0,450 bpp | `c52_static_text_is_not_refreshed_by_the_recovery` |
| `recovery_reachable_under_jitter` | **gate D2**: 155 muestras de jitter neutro recuperan (escala a los 20 s, tope a los 23 s); 3 neutras seguidas dan un escalón | `c52_collapse_does_not_recover_under_neutral_jitter` |
| `c52_field_ladder_starts_at_the_text_floor` | la escalera de S1 arranca en 0,450 bpp y llega al tope; los escalones de campo (6,22 y 9,33 Mbps = 0,200/0,300 bpp) se conservan como evidencia del defecto | `c52_field_ladder_is_reproduced_by_the_policy` |
| `c52_session2_replay_keeps_the_bpp_floor` | con la **misma** serie de RTT de S2 ninguna decisión baja del piso (17 tasas del campo sí lo hacían) | `c52_session2_collapse_is_reproduced_by_the_policy` |

Regresión de ciclos anteriores (tests de `mod tests` adaptados **solo** donde la
aserción codificaba el defecto D1/D2):

| Test | Cambio |
|---|---|
| `net_low_counts_neutral_jitter_toward_recovery` (era `net_low_survives_neutral_jitter`) | la primera decisión en 720p@3 Mbps es `red_escala` (la tasa ya nace en el piso) y la zona neutra suma: `net_low` 5 → 6 |
| `net_adaptation_confirms_then_lowers_and_recovers_after_cooldown` | ídem: `red_escala` + recuperación `red_recupera_escala`; `bps` nunca bajo el piso |
| `net_congestion_at_floor_scales_down_and_blocks_cpu_recovery` | usa 960×540 con tope 6 Mbps (el antiguo 250 kbps está por debajo del piso y no puede recuperar nada); sin forzar `bps = min_bps` |
| `adapt_low_bitrate_first_then_scale_under_pressure` | parte de una tasa por encima del piso (estado real tras recuperar) y verifica que el escalón de bitrate **clava** el piso (3 499 200) antes de escalar |
| `scale_ladder_is_capped_and_does_not_reinflate` | el escalón se contrasta contra el piso de bpp de su tamaño (antes: «4× el suelo de 200 kbps») |
| `max_bitrate_caps_initial_and_ceiling` | la tasa inicial sin tope es 13 996 800 (piso de bpp), no `w*h*3`; el techo `w*h*3*4` no cambia |
| `scale_recovery_respects_the_bitrate_cap` | sin cambios (verde) |

Nuevos tests en el crate de códec: `request_refresh_without_mft_is_rejected`
(MF sin transform → `false`) y `request_refresh_latches_the_next_keyframe` (VP9).

---

## 4. Gates ejecutados (resultado real)

```
cd core
cargo fmt --all                                  → sin diferencias
cargo fmt --all -- --check                       → sin diferencias
cargo clippy --workspace --all-targets -- -D warnings → 0 avisos / 0 errores
cargo test --workspace                           → 179 passed / 0 failed
```

| Gate de `specs/02` §13.1 | Comando | Resultado |
|---|---|---|
| `ladder_preserves_bpp` | `cargo test -p anyremote-daemon --lib ladder_preserves_bpp` | ✅ 1 passed / 0 failed |
| `static_text_refreshes_after_recovery` | `cargo test -p anyremote-daemon --lib static_text_refreshes_after_recovery` | ✅ 1 passed / 0 failed |
| `recovery_reachable_under_jitter` | `cargo test -p anyremote-daemon --lib recovery_reachable_under_jitter` | ✅ 1 passed / 0 failed |
| Regresión C-27/C-29/C-32/C-35/C-36 | `cargo test --workspace` | ✅ 179 passed / 0 failed |
| Clippy + fmt | arriba | ✅ |

Toolchain: `stable-x86_64-pc-windows-msvc` (el feature `vpx` del crate `codec`
se compila en el workspace porque `daemon` lo activa: `daemon/Cargo.toml:22`).

**No se reconstruye el binario** en esta fase (lo hace el orquestador al cerrar
el ciclo, D-13c) y **no hay commit**: el trabajo queda en el árbol de
`feature/c52-video-regression`.

---

## 5. Riesgos y pendientes (honesto)

1. **Arranque más caro**: a 1080p la tasa inicial pasa de 6,2 a 14,0 Mbps
   (0,200 → 0,450 bpp). En un enlace que no los sostenga, la escalera lo baja en
   ~3 s (3 muestras de congestión) pero esos primeros segundos pueden
   congestionar. Es el precio de que el texto sea legible desde el primer IDR.
2. **Recuperación «de golpe»**: al restaurar la escala se restaura el piso de
   bpp del tamaño inicial (≈14 Mbps a 1080p). Si el enlace solo aguanta ~8 Mbps
   se verá un diente de sierra con periodo ≈ cooldown (20 s). El guard es el
   cooldown; si el gate de campo lo muestra, el ajuste correcto es una
   recuperación más lenta (más muestras/`stable window`), **no** bajar el piso.
3. **MF**: si el MFT no soporta `CODECAPI_AVEncVideoForceKeyFrame` (p. ej. el
   codificador software de Microsoft), `request_refresh` devuelve `false` y el
   texto solo se refresca en el GOP (30 entradas ≈ 30 s en estático). El log
   dirá `aceptado=false`; el campo debe confirmarlo con la medición **M2**
   (posición de los IDR), que la Fase A dejó pendiente.
4. **Limpiar el comando**: se pone a `1` justo antes del `ProcessInput` que lo
   consume y a `0` después. Si un MFT asíncrono muestreara la propiedad más
   tarde, el IDR se perdería (degradación al comportamiento anterior, nunca
   «todo IDR»). Verificar con M2.
5. **Tope del usuario < piso de texto**: si `--max-bitrate` no sostiene 0,45 bpp
   ni en el tamaño mínimo de la escalera, el piso queda acotado por el tope y el
   texto no llegará a 0,45. Es deliberado (el enlace manda), pero el visor
   debería avisar; no se toca la UI en este ciclo.
6. **Pendiente de la Fase B del diseño**: la instrumentación M1–M4 (RTT por
   muestra, IDR por frame, bpp por región, resolución nativa) sigue sin
   implementarse; este ciclo solo añade la línea de refresco por decisión.

---

## 6. Trazabilidad

- Causa raíz: `evidence/video-quality-regression.md` §3 (D1/D2/D3).
- Diseño y gates: `specs/02` §13.1 (Fase B).
- Decisiones del usuario aplicadas: suelo 0,45 bpp · IDR al recuperar y al
  cambiar de escala · N=3 con la zona neutra contando · cooldown de 20 s
  mantenido.
- Ciclos tocados sin reabrir: C-27 (`net_excess` intacto), C-29/C-32 (escalera y
  presets, tests adaptados en las aserciones que afirmaban el defecto), C-35/C-36
  (ruta del encoder y escalado, sin cambios de comportamiento fuera del refresco).
