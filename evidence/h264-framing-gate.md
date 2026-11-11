# Gate C-26 — framing H.264 robusto + decoder auto-sanado (v0.12.9)

**Fecha**: 2026-11-11 · **Rama**: `feature/h264-framing` · Disparador: reporte de campo — «con equipos con buen internet todo iba muy lento, imposible trabajar» (logs `E:\audiany\juank\anyremote` y `E:\audiany\juank2\anyremote`)

## Diagnóstico (logs de campo)

| Dato | Valor |
|---|---|
| Escenario | LAN directo (`broadcast →192.168.1.63`, **rtt3 ms**), QUIC, ambas máquinas en **v0.12.8**, anfitrión = juank2 (Nativa y720p) |
| Sesión1 | anfitrión **`frames_sent:261`** /954 KB · visora **243 errores de decode** (`native:18`) → solo **18 frames** decodificados (538 KB de los954) |
| Sesión2 | **289 frames** /4,09 MB · visora **269 errores** → ~55 % de bytes decodificados |
| Error | `openh264 native:18` = **`dsParamSetsNotCompatible (0x10) \| dsRefLost (0x02)`** — prácticamente **todos los P-frames fallaban** → el visor se quedaba congelado («imposible trabajar»). En mis pruebas locales: **0 errores**; en ONDH (máquina distinta): errores aislados → **dependía de la máquina** (MFT del anfitrión). |

**Mecanismo**: la cabecera `MF_MT_MPEG_SEQUENCE_HEADER` **no es uniforme
entre MFT** (unos dan Annex-B `00 00 00 01 SPS…`, otros **avcC** con
longitud4B por NAL). Nuestro prepend crudo (`seq_hdr + payload`) producía
bitstreams ilegibles para el decoder en los MFT que no dan Annex-B → el
decoder quedaba **envenenado** y todos los frames posteriores fallaban.

## Qué se implementó (3 capas)

| Capa | Detalle |
|---|---|
| **1. Normalización en el límite** (`mf_backend.rs`) | `normalize_seq_header()`: ya Annex-B → intacto; basura delante → corta desde el primer start code; **avcC → Annex-B** (recorrido `[len4B][NAL]`); si nada → antepone `00 00 00 01`. Se aplica al cargar el blob (propiedad del productor: la cabecera normalizada vive en `seq_hdr`). |
| **2. Prepend inteligente** | Se anteponen los parámetros **solo si el payload no trae ya un SPS in-band** (`contains_sps` — parseo Annex-B de tipos de NAL) → nunca se duplica/enreda la cabecera. **Log de framing una vez por sesión**: `[video] h264 framing: seq_hdr[…]=… payload[…]=… con_sps=…` (diagnóstico de campo para confirmar qué dio el MFT). |
| **3. Decoder auto-sanado** (`openh264_backend.rs`) | Si `native_code &0x30` (bits de **parámetros**), se **recrea** el decoder y se reintenta **una vez** (con borrow separado: primera pasada → owned → reset → reintento). Un P sin referencia vuelve a fallar y el **próximo IDR lo recupera** en vez de dejar el vídeo muerto. Los primeros3 resets se loguean (`[video] decoder H.264 auto-reset …`). |

## Tests (+3 →51 suites)

- `seq_header_normalizes_to_annexb`: Annex-B intacto · basura delante →
  corte · **avcC → Annex-B** · ni uno ni otro → start code antepuesto.
- `contains_sps_detects_in_band_headers`: SPS con start code ✓ · P-slice
  ✗ · PPS ✗ · sin start codes ✗.
- `decoder_recovers_from_param_change` (real MF→OpenH264): warmup a
  320×180 decodifica → cambio a160×90 (SPS distinto) → el decoder se
  **auto-sanifica** y decodifica.

## Gates (2026-11-11)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` | **51 suites / 0 fallos** |
| `cargo clippy --workspace --all-targets` | **0 / 0** |
| `cargo fmt --check` | **0** |
| `flutter analyze` / `flutter test` | limpio · **23/23** |

## Pendiente de validación de campo (obligatorio antes del release)

- **Repetir la prueba en juank/juank2** (mismo escenario LAN): el visor
  debe decodificar **sin** la lluvia de `vídeo: Decode(native:18)`; si
  quedara algo, el `ui.log` traerá `[video] h264 framing: …` (qué dio el
  MFT) y `[video] decoder H.264 auto-reset …` (cuántas veces se sanó).
- Confirmar además el `fps render` pendiente (decisión `Texture`).
