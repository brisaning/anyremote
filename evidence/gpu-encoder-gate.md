# Gate C-35 — encoder de GPU (NVENC) y conversión de color rápida (v0.13.8)

**Fecha**: 2026-11-18 · Rama: `feature/c35-gpu-encoder` · Origen: siguiente paso
acordado tras C-33/C-34 («encoder asíncrono para 4K real con control de tasa»).

## Hallazgos

### 1. Hay encoder de GPU, pero el código lo descartaba

`MFTEnumEx` en este equipo devuelve:

| MFT | asíncrono | Notas |
|---|---|---|
| `AMDh264Encoder` | — | no activa |
| `NVIDIA H.264 Encoder MFT` | **sí** | acepta **NV12 en memoria del sistema** (sin D3D) y **provee sus propias muestras** (`MFT_OUTPUT_STREAM_PROVIDES_SAMPLES`, `cbSize=0`) |
| `Microsoft AVC DX12 Encoder` | sí | rechaza el tipo de salida sin gestor D3D12 |
| `H264 Encoder MFT` | no | software (el que se usaba) |

El código anterior descartaba cualquier MFT asíncrono (necesita
`MF_TRANSFORM_ASYNC_UNLOCK=1` **antes** de fijar tipos) y se quedaba con el de
software, que ignora el rate-control (C-33).

### 2. NVENC **sí** respeta el bitrate

Con el ciclo de eventos (`METransformNeedInput` → `ProcessInput`;
`METransformHaveOutput` → `ProcessOutput`), 1080p@15:

| Objetivo | KB/frame | Tiempo | Perfil |
|---|---|---|---|
| 2 Mbps | 10,7 | **11,1 ms** | Main (CABAC) |
| 6 Mbps | 38,6 | **11,8 ms** | Main |
| 15 Mbps | 105,5 | **11,9 ms** | Main |

(30 frames, primeros 6 de calentamiento fuera de la medida; `cargo test
--release`.) Frente al MFT de software, que daba **23,0 KB/frame a cualquier
bitrate** (2/6/15 Mbps idénticos, C-33).

### 3. La conversión de color era más cara que el encoder

`BGRA → I420 → NV12` (tres pasadas y tres `Vec` por frame):

| Resolución | Antes | Una pasada (C-35) | En bandas paralelas (C-35) |
|---|---|---|---|
| 1080p | 16,2 ms | 7,6 ms | **3,4 ms** |
| 4K | 52,3 ms | 29,8 ms | **9,3 ms** |

Ahora `bgra_to_nv12_into` escribe Y y UV entrelazado **directamente** en el
buffer reutilizado del encoder, sin allocs por frame, y reparte por filas de
pares entre hilos (`std::thread::scope`, solo si `h ≥ 720`).

### 4. Límite encontrado: 4K asíncrono se atasca

A 3840×2160 el MFT de NVIDIA acepta la primera entrada (`ProcessInput` Ok en
~10 ms) y **deja de emitir eventos**: ni `METransformHaveOutput` ni
`METransformNeedInput` en los 24 frames siguientes. Se descartaron como causa el
`ICodecAPI`, `MF_MT_VIDEO_NOMINAL_RANGE`, el perfil, el apartamento COM y la
lectura de diagnóstico del tipo. El mismo MFT en un banco aislado **sí** codifica
4K (12/12 salidas, ~30 ms/frame), así que queda como pendiente de investigación
(probablemente requiere entrada por textura D3D11 en vez de memoria del sistema).

**Decisión**: el camino asíncrono se usa **hasta 1080p**; por encima se usa el
MFT síncrono y se registra en el log
(`3840x2160 > 1080p → encoder de GPU desactivado`).

### 5. Efecto colateral bueno en 4K

Con la conversión rápida, el MFT **síncrono** a 4K baja de **70–394 ms** (campo,
C-31) a **28,3 ms/frame** (release, calentado), con 13/24 emisiones por el
lookahead del MFT. 4K «Nativa» deja de ser inviable a 15 fps, aunque sigue en QP
fijo (sin control de tasa).

## Cambios

| Pieza | Detalle |
|---|---|
| `mf_backend.rs` | `MF_TRANSFORM_ASYNC_UNLOCK` + selección GPU primero (hardware ordenado por `MFT_ENUM_FLAG_SORTANDFILTER`); ciclo de eventos con presupuesto de 10 ms (`yield_now`, no `sleep`: en Windows se redondea a 15,6 ms); muestras de entrada nuevas por envío (el MFT las retiene); salida provista por el MFT o por nuestro slot; caída automática al síncrono si algo falla (`async_failed`), sin tumbar la sesión; `ANYREMOTE_HW_ENCODER=0` fuerza el síncrono. |
| `conv.rs` | `bgra_to_nv12_into`: una pasada, sin allocs, en bandas paralelas. Se conserva `bgra_to_i420` para el backend VP9. |
| `mf_backend.rs` | `finish()` común a ambos caminos; la cabecera SPS/PPS se carga **bajo demanda** (`GetOutputCurrentType` en un MFT asíncrono costaba ~40 ms/frame). |

## Gates (2026-11-18)

| Gate | Resultado |
|---|---|
| `cargo test --workspace` | **154 / 0** |
| `cargo clippy --workspace --all-targets` | **0 avisos** |
| `cargo fmt --all -- --check` | **0 diffs** |
| `flutter analyze` / `flutter test` | limpio · **27 / 27** |

## Límites y pendientes

- **4K asíncrono**: atascado (arriba). Siguiente intento: entrada por textura
  D3D11 (`MFT_MESSAGE_SET_D3D_MANAGER`) o el `Microsoft AVC DX12 Encoder` con su
  gestor D3D12.
- En máquinas sin GPU (CI, VMs) todo sigue por el MFT de software: los tests no
  dependen de NVENC.
- **Verificación de campo pendiente**: en el anfitrión real, `curl`/log debe
  mostrar `'NVIDIA H.264 Encoder MFT'` y `kbps` acordes al objetivo cuando hay
  movimiento; `ANYREMOTE_HW_ENCODER=0` sirve de A/B.
