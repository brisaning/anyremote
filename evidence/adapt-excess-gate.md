# Gate C-27 — usable: adaptación por exceso + decoder salta a IDR + log tasaado (v0.13.0)

**Fecha**: 2026-11-14 · **Rama**: `feature/adapt-excess` · Disparador: reporte de campo — «el aplicativo no es usable: las ventanas se ven por saltos y muy lento» (1 cliente +2 anfitriones: juank 400 Mbps, juank2 100 Mbps, anfitrión2 = esta máquina400 Mbps; logs en `E:\audiany\juank*` y `%APPDATA%`)

## Diagnóstico (logs de campo, v0.12.9,3 sesiones)

| Sesión | Camino | rtt | Producción | Errores decode |
|---|---|---|---|---|
| juank→juank2 (LAN) | QUIC directo | **3 ms** | **157 frames/69 s =2,3 fps** | **146** (`native:18`) |
| juank→juank2 (2ª) | QUIC directo |3 ms | **83/50 s** | **77** |
| juank→anfitrión2 | **relay** (`quic_fallback`→`relay_fallback`, sin reenvío de puertos) | **335 ms** (último507) | **11 frames/78 s =0,14 fps** 😱 |0 |

**Tres causas raíz:**

1. **Adaptación por RTT ABSOLUTO** (`observe_net` con umbrales fijos
   200/100 ms): en el relay la latencia base (335 ms) **supera SIEMPRE el
   umbral de congestión** (y la recuperación exige ≤100 ms, inalcanzable) →
   cascada permanente `red_bitrate` 4M→200 kbps + escalas → el rate-control
   del MFT deja de emitir → **11 frames en78 s**. En perf-audit §1 el
   patrón de RustDesk (`video_qos.rs`) usa **ABR con bitrate-primero y
   umbrales estables** — nosotros teníamos el umbral mal calibrado para
   latencia-base-alta.
2. **Decoder envenenado por cambios de parámetros**: `native:18`
   (paramSets+refLost) **146+77 veces** — el auto-reset de C-26 ayudaba
   pero el reintento de un P sin referencia seguía fallando y el registro
   se inundaba.
3. **Log/UI inundados**: el detalle de C-25 imprimió **735 líneas** de
   error en una sesión (cada `_append` reconstruye la página → carga de UI).

## Qué se implementa

| Pieza | Detalle |
|---|---|
| **1. Congestión RELATIVA** (`NetHint.rtt_min_ms` + `net_excess`) | El controlador manda **RTT + mínimo de sesión**; el host alimenta `observe_net` con el **exceso** (`rtt − min`). Relay estable (335/335) → exceso0 → **sano, sin cascada**; un pico real (507−335=172… >200) sí dispara. `net_rtt_override` (tests) manda baseline0 → **comportamiento absoluto intacto** (e2e C-13 sin cambios). Peer viejo sin el campo →0 → absoluto (compat proto3). |
| **2. Decoder salta hasta IDR** (`CodecError::Skipped` + `has_idr_nal`) | Tras auto-reset, los frames sin referencia se **SALTAN en silencio** (hasta el próximo IDR NAL5) en vez de devolver `native:18`; el IDR reconstruye el estado. **Ningún error de decode debe escapar tras el reset** (asertado en test). `handle_video_wire` ignora `Skipped` (sin audit ni evento). |
| **3. Log tasaado** | `video_error`: primeros**3** completos + resumen **cada50**; contador por sesión (se limpia al cerrar visor). La **auditoría sigue grabando todos**. |

## Tests (+3 →24 Flutter /51 suites Rust)

- `net_excess_is_relative_to_session_min`:335/335→0 (relay sano) ·
  507/335→172 · nunca negativo · sin baseline→absoluto.
- `has_idr_nal_detects_idr_slices` (IDR/P/SPS).
- `decoder_recovers_from_param_change` **endurecido**: tras el cambio de
  SPS solo pueden escapar `Ok` o `Skipped` — **cualquier otro error
  (native:18) hace panic**.
- Widget:4 eventos `video_error` → **solo3** líneas en el registro.

## Gates (2026-11-14)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` | **51 suites / 0 fallos** |
| `cargo clippy --workspace --all-targets` | **0 / 0** |
| `cargo fmt --check` | **0** |
| `flutter analyze` / `flutter test` | limpio · **24/24** |

## Pendiente de validación de campo

- Repetir el escenario: **relay** (anfitrión2) debe mantener calidad
  (sin cascada a200 kbps) y **LAN** debe ir limpio de `native:18`.
- **Para WAN directo** (latencia real): reenviar el puerto **21118/TCP y
  UDP** en el router del anfitrión2 — hoy cae al relay (rtt335 ms) y eso
  también limita la interacción.
- `fps render` por modo (decisión `Texture`).
