# Gate C-33 — legibilidad del texto: zoom 1:1, encoder y tasa real (v0.13.6)

**Fecha**: 2026-11-18 · Rama: `feature/c33-viewer-zoom` · Disparador: campo (Brian
anfitrión 3840×2160 al 125 % → Julie cliente 1920×1080): **texto ilegible en las
tres calidades**, también con los presets nuevos de C-32 (1080p @ 15 Mbps).

## Medición de campo (18/11, log de Brian)

Sesión `08:53:55 → 08:55:23` (88 s), 1080p @ 15 Mbps:

| Dato | Valor |
|---|---|
| `bytes_sent` (cierre de sesión) | **2 921 679 B en 88 s ≈ 265 kbps** |
| frames renderizados en Julie | 101 (~1,15 fps; pantalla casi estática) |
| `encode_ms` | 17–23 ms (no es el cuello) |
| `escala_ms` | 12–38 ms |
| calidad adaptada | sube 9,3 → 14 → **15 Mbps** (tope) |

El objetivo de 15 Mbps no se traducía en bits reales. No había forma de verlo en
el log: no existía contador de bytes por ventana.

## Diagnóstico (medido en laboratorio, escritorio real del anfitrión)

Sondas ignoradas (`core/daemon/tests/video_quality_probe.rs`) con la captura real
3840×2160 escalada a 1920×1080 y el pipeline completo captura → `scale_frame` →
H.264/MF → `NativeDecoder`:

| Prueba | Resultado |
|---|---|
| 2 / 6 / 15 / 25 Mbps | **el mismo stream** (I-frame 196–209 KB, P 0,2–4 KB; total idéntico ±1 B: solo cambia el tamaño del SPS) |
| perfil en el SPS | `0x42` = **Baseline** (66, sin CABAC) |
| calidad a 1:1 (recorte ×2) | I-frame **indistinguible** del original |
| PSNR de luma | 27,6 dB, pero **37,1 dB tras quitar una recta**: `dec = 1,108·src − 12,8` (error **sistemático** de rango, no de codificación) |

Y con la lista de MFT de la máquina:

| MFT | asíncrono | ¿se usa? |
|---|---|---|
| `NVIDIA H.264 Encoder MFT` | sí | no (los async se descartan) |
| `Microsoft AVC DX12 Encoder` | sí | no |
| `AMDh264Encoder` | — | no activa |
| `H264 Encoder MFT` (software) | no | **sí** |

**Causa raíz del rate-control**: el MFT acepta `CODECAPI_AVEncCommonRateControlMode`,
`AVEncCommonMeanBitRate` y `AVEncCommonMaxBitRate` (`GetValue` devuelve lo
escrito, `MF_MT_AVG_BITRATE` también queda en el tipo actual) y **los ignora al
codificar**: el stream es idéntico a 2 y a 25 Mbps. Antes, además, se escribían
en `IMFTransform::GetAttributes` — el almacén de atributos del transform, **no**
el de códecs — por lo que ni siquiera llegaban al MFT.

**Causa raíz de la legibilidad** (la importante): no es el códec. El escritorio
del anfitrión es **3840×2160 al 125 %** (texto de ~16 px físicos). Al emitir
1080p el texto llega a **8 px**, y a 720p a **5 px**. En una pantalla de 1080p
eso no se lee en modo «ajustar a la ventana», por muy alto que sea el bitrate.
Los visores de referencia (RustDesk: *Scale original / Scale adaptive* + *scroll
mode*; AnyDesk: *Original size / Fit to window*; Take Control: 10–200 %) tienen
justo ese control: **100 % con barras**.

## Cambios

| Pieza | Detalle |
|---|---|
| **Visor (app)** | Selector **Ajustar / 100 % / 150 %**. A 100 % el frame se dibuja **1:1** (`filterQuality: none`) dentro de un área con **barras** de desplazamiento; al ajustar se usa filtrado de calidad. La rueda sigue siendo del remoto (se reclama el `PointerEvent`), el paneo es por barras. |
| **Diagnóstico de escala** | El log de 5 s del visor añade `frame WxH · caja BxH · escala P% (ajustar/100%)`: en campo se ve si el texto se está reduciendo. |
| **Diagnóstico de tasa (daemon)** | El log de 5 s añade **kbps reales, KB/frame y bpp**: el objetivo del encoder ya no puede mentir. |
| **Encoder** | Perfil **Main (77, CABAC)** primero con caída a Baseline; rate-control por **`ICodecAPI`** (CBR media=pico=tope), GOP 2 s, sin B-frames, `MF_MT_VIDEO_NOMINAL_RANGE` 0–255; **cambio de tasa en vivo** (sin reconstruir el MFT, que en C-24 dejó al visor sin vídeo). Se registra nombre del MFT + parámetros aceptados + tipo actual. |

## Gates (2026-11-18)

| Gate | Resultado |
|---|---|
| `cargo test --workspace` | **154 / 0** |
| `cargo clippy --workspace --all-targets` | **0 avisos** |
| `cargo fmt --all -- --check` | **0 diffs** |
| `flutter analyze` | limpio |
| `flutter test` | **26 / 26** (nuevo test C-33: 100 % dibuja 1:1) |

## Pendiente de validación de campo

- **Julie ← Brian en 1080p con el visor en 100 %**: el texto debe leerse. Si se
  quiere la pantalla completa y el texto a tamaño normal, hay dos vías:
  - **bajar el escritorio del anfitrión a 1920×1080** (captura 1:1, sin escalar;
    además el encode baja a 6–23 ms), o
  - **subir la escala de Windows del anfitrión al 200 %** (mismo framebuffer 4K,
    texto al doble: al emitir 1080p llega a 16 px).
- Con el log nuevo, comprobar que los `kbps` reales se acercan al objetivo
  cuando hay movimiento; el MFT de software **no aplica** el rate-control
  (queda en QP fijo = calidad alta), así que el adaptativo por bitrate no tiene
  efecto en este backend: la palanca real es la **resolución**.
- En WAN, vigilar `kbps` reales: el QP fijo puede gastar más de lo que el enlace
  aguanta; el controlador seguirá bajando la escala.
