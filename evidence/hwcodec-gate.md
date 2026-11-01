# Gate C-14 — aceleración por hardware: H.264 (Media Foundation + OpenH264) · v0.9.0

| Campo | Valor |
|---|---|
| Fecha | 2026-11-01 |
| Rama | `feature/hwcodec-draft` (D-11) · tag `hwcodec-v0.9.0` |
| Origen | **2ª del backlog `specs/02` §7** — elección del usuario: «H.264 por Media Foundation (Recomendado)» |
| Tools externas | **ffmpeg/ffprobe 9.0.2** (gyan.dev, descarga autorizada por el usuario) para validación de stream |
| Terceros | **Cisco OpenH264** (licencia **BSD**) como decodificador — autorizado explícitamente: «si con elementos de terceros se logran mejores resultados podemos usarlos» |
| Gates | Rust **48 suites / 0 fallos** · **clippy 0 avisos / 0 errores** · **fmt 0 diffs** · `flutter analyze` limpio · `flutter test` **9/9** |

## Qué se entregó

1. **Wire y fábrica**: `CodecFormat::H264` (enum3, `H264_CODEC`), anunciado en
   `welcome().supported_codecs`, `--codec h264` en el CLI y **la app lo usa por
   defecto** en `buildListenArgs`.
2. **Encoder H.264 por Media Foundation** (`mf_backend`): selección de MFT
   (`SORTANDFILTER`: hardware → software) con **plan B** (`GetXAvailableType`),
   sonda síncrona (descarta los MFT **V2 asíncronos**: en esta máquina
   NVIDIA/AMD/DX12 lo son — solo sirve el MFT software de Microsoft), dims
   pares con recorte, CBR, perfil **Baseline 66** (sin B-frames), `MF_LOW_LATENCY`,
   **renegociación de tasa** en caliente (fallback a reconstrucción perezosa;
   el tope QW-3 rige) y SPS+PPS (`MF_MT_MPEG_SEQUENCE_HEADER`) ante puestos a
   cada NAL (cualquier frame es decodificable por sí solo).
3. **Fallback visible**: si el MFT no sirve, `video.rs` cae a **VP9/libvpx**
   emitiendo **`video_backend`** con el motivo (sin oscilar: si MF falló, no se
   reintenta en los cambios de escala). La app lo muestra: *«códec de vídeo: …»*.
4. **Decoder: Cisco OpenH264 (BSD)** — nuevo `openh264_backend` con salida
   BGRA8 (vía `write_rgb8` de la propia librería + expansión), `None` →
   `NEED_MORE_INPUT` (semántica de stream idéntica al backend MF).

## Diagnóstico (la evidencia manda)

1. **Encoder**: hubo que descubrir por experimentación: **salida ANTES que
   entrada** (el MFT de MS rechaza la entrada con `MF_E_INVALIDMEDIATYPE` si
   aún no hay salida), `SetSampleTime` **y duración** obligatorios, y —clave—
   **este MFT devuelve `pdwstatus=0` aunque ESCRIBE la muestra**: descartar
   por el bit `SAMPLE_READY` tiraba NALs reales.
2. **Validación externa (punto 1 del usuario)**: volcado a `.h264` y
   **ffprobe** → `h264 · Constrained Baseline · 320×180 · level 13 ·
   44/44 frames`; **decodificación completa con ffmpeg → 0 avisos, exit 0**.
   **Conclusión: el stream del encoder es VÁLIDO → el problema era el uso del
   decoder MFT.**
3. **Decoder MFT de Microsoft: bloqueado documentado** — configura todo
   (tipos, blob SPS/PPS, dims coherentes, notificaciones tras configurar) y
   **acepta todas las entradas pero nunca emite salida**. Matriz agotada:
   formatos de cabecera (AnnexB/AVCC/nativa), órdenes entrada/salida,
   notificaciones con/sin, `COMPRESSED`+perfil, alimentación por lote,
   relleno `FIXED 4096`, salida I420 vs NV12, `QualityVsSpeed` (empeoró),
   SourceReader (rechaza `.264` sin contenedor).
4. **Decisión (punto 2 + autorización de terceros)**: en lugar de un
   decodificador D3D11/DXVA (esfuerzo L) se usa **Cisco OpenH264 (BSD)** —
   decodificador de referencia, ya testeado contra nuestro stream.

## Tests

| Suite | Qué cubre |
|---|---|
| codec · `h264_roundtrip_mf_encoder_openh264_decoder` | **Roundtrip cruzado real**: encoder MF → NALs AnnexB → OpenH264 → BGRA con dims/pixeles correctos |
| codec · `h264_encode_decode_roundtrip_preserves_geometry` | mismo flujo vía `NativeDispatcher` (`NativeDecoder`) con semántica `NEED_MORE_INPUT` |
| codec · `odd_dimensions_are_cropped_to_even` | dims impares → recorte a pares (NV12) |
| codec · `dump_h264_for_external_check` | vuelco reproducible para validación con ffmpeg |
| codec · conv y suites previas | NV12, VP9, deflate, raw intactos |
| daemon e2e · `h264_flows_host_encoder_to_viewer_decoder` | sesión completa **Test → H264 → sesión → OpenH264 → `video_stats` + preview BGRA** (cliente TCP real) + evento `video_backend` con `mf_h264` o fallback `vpx_vp9` |
| Flutter · `buildListenArgs` (C-9 test) | la app pide `--codec h264` |

## Regresión

Rust **48/0** · clippy **0/0** · fmt **0** · Flutter **analyze limpio + 9/9**.
El backend VP9 sigue disponible (`--codec vp9`) y es el fallback automático;
los gates de vídeo M2 (Deflate) y de las rondas anteriores pasan sin cambios.

## Limitaciones

- **GPU real**: en esta máquina todos los MFT de GPU son **asíncronos (V2)**
  → el camino MF usa el **MFT por software** de Microsoft (que ya reduce CPU
  respecto a VP9 y mantiene el pipeline H.264). Aceleración GPU directa =
  trabajo futuro (protocolo async MFT o SDKs de fabricante, §7).
- **Ancho de banda**: H.264/Constrained-Baseline a la misma calidad usa algo
  más de bits que VP9 (perfil pensado para baja latencia); el tope
  `--max-bitrate` y la adaptación por red (C-13) lo gestionan.
- El **decoder** depende de un tercero (BSD, `openh264` 0.9.9, se builda con
  CMake ya exigido por el proyecto); el encoder no depende de terceros.
