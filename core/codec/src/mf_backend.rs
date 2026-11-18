//! H.264 por **Media Foundation** (C-14, clean-room D-01 desde APIs públicas —
//! sin copia del fork AGPL `hwcodec`).
//!
//! Hallazgos de desarrollo (documentados en `evidence/hwcodec-gate.md`):
//! - Los MFT modernos de GPU (NVIDIA/AMD/DX12) son **V2 asíncronos** y se
//!   descartan en la sonda síncrona; quedan los MFT sync (el de software de
//!   Microsoft y los V1 de fabricante si existen).
//! - El MFT de Microsoft exige **salida antes que entrada** en la
//!   configuración y devuelve `pdwstatus=0` **aunque escribe la muestra**:
//!   hay que leer siempre que `ProcessOutput` dé `Ok` (con el bit
//!   `SAMPLE_READY` se tiraban NALs reales).
//! - El decodificador anuncia **plantillas** de salida (p. ej. 1920×1080) y
//!   rechaza dims hechas a mano: se adopta un tipo **disponible** y las dims
//!   reales llegan con `MF_E_TRANSFORM_STREAM_CHANGE`.
//!
//! Si nada sirve, el llamador (`video.rs`) cae a VP9/libvpx con evento
//! `video_backend`. Las conversiones de color viven en [`crate::conv`].

use std::collections::VecDeque;
use std::sync::OnceLock;

use anyremote_capture::Frame;
use windows::core::{Interface, GUID};
use windows::Win32::Media::MediaFoundation as mf;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};
use windows::Win32::System::Variant::{VariantInit, VARIANT, VARIANT_0_0, VARIANT_0_0_0, VT_UI4};

use crate::conv::{bgra_to_nv12_into, crop_bgra};
use crate::{CodecError, CodecFormat, EncodedFrame, VideoEncoder};

/// `MFStartup` una vez por proceso; `CoInitializeEx` por hilo (idempotente).
pub fn mf_ready() -> Result<(), CodecError> {
    static START: OnceLock<Result<(), String>> = OnceLock::new();
    unsafe {
        // S_FALSE/RPC_E_CHANGED_MODE son inofensivos (hilo ya inicializado).
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    START
        .get_or_init(|| unsafe {
            mf::MFStartup(mf::MF_VERSION, mf::MFSTARTUP_FULL).map_err(|e| format!("MFStartup: {e}"))
        })
        .clone()
        .map_err(CodecError::Encode)
}

fn type_info(subtype: GUID) -> mf::MFT_REGISTER_TYPE_INFO {
    mf::MFT_REGISTER_TYPE_INFO {
        guidMajorType: mf::MFMediaType_Video,
        guidSubtype: subtype,
    }
}

/// Enumera MFT de una categoría (hardware primero por `SORTANDFILTER`).
fn enum_mfts(
    category: GUID,
    input: Option<&mf::MFT_REGISTER_TYPE_INFO>,
    output: Option<&mf::MFT_REGISTER_TYPE_INFO>,
) -> Result<Vec<mf::IMFActivate>, CodecError> {
    unsafe {
        let mut activates: *mut Option<mf::IMFActivate> = std::ptr::null_mut();
        let mut count: u32 = 0;
        mf::MFTEnumEx(
            category,
            mf::MFT_ENUM_FLAG_ALL | mf::MFT_ENUM_FLAG_SORTANDFILTER,
            input.map(|t| t as *const _),
            output.map(|t| t as *const _),
            &mut activates,
            &mut count,
        )
        .map_err(|e| CodecError::Encode(format!("MFTEnumEx: {e}")))?;
        let mut out = Vec::new();
        if !activates.is_null() {
            for i in 0..count as usize {
                if let Some(a) = (*activates.add(i)).clone() {
                    out.push(a);
                }
            }
            CoTaskMemFree(Some(activates as *const _));
        }
        Ok(out)
    }
}

/// Tipo de vídeo con dos atributos base (mayor + subtipo).
unsafe fn video_type(subtype: GUID) -> Result<mf::IMFMediaType, CodecError> {
    let mt = mf::MFCreateMediaType()
        .map_err(|e| CodecError::Encode(format!("MFCreateMediaType: {e}")))?;
    mt.SetGUID(&mf::MF_MT_MAJOR_TYPE, &mf::MFMediaType_Video)
        .map_err(|e| CodecError::Encode(format!("MF_MT_MAJOR_TYPE: {e}")))?;
    mt.SetGUID(&mf::MF_MT_SUBTYPE, &subtype)
        .map_err(|e| CodecError::Encode(format!("MF_MT_SUBTYPE: {e}")))?;
    Ok(mt)
}

fn frame_size_attr(mt: &mf::IMFMediaType, w: u32, h: u32) -> Result<(), CodecError> {
    unsafe {
        mt.SetUINT64(&mf::MF_MT_FRAME_SIZE, ((w as u64) << 32) | h as u64)
            .map_err(|e| CodecError::Encode(format!("MF_MT_FRAME_SIZE: {e}")))
    }
}

/// Atributos comunes que los MFT de H.264 exigen en ambos tipos.
fn common_video_attrs(mt: &mf::IMFMediaType, w: u32, h: u32, fps: u32) -> Result<(), CodecError> {
    unsafe {
        mt.SetUINT64(&mf::MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1)
            .map_err(|e| CodecError::Encode(format!("MF_MT_PIXEL_ASPECT_RATIO: {e}")))?;
        mt.SetUINT64(&mf::MF_MT_FRAME_RATE, ((fps as u64) << 32) | 1)
            .map_err(|e| CodecError::Encode(format!("MF_MT_FRAME_RATE: {e}")))?;
        // Progresivo (MFVideoInterlace_Progressive = 2).
        mt.SetUINT32(&mf::MF_MT_INTERLACE_MODE, 2)
            .map_err(|e| CodecError::Encode(format!("MF_MT_INTERLACE_MODE: {e}")))?;
        let _ = (w, h);
        Ok(())
    }
}

/// Atributos de un tipo **sin comprimir** NV12 (entrada del codificador y
/// salida del decodificador: stride, tamaño fijo…).
fn nv12_raw_attrs(mt: &mf::IMFMediaType, w: u32, h: u32) -> Result<(), CodecError> {
    unsafe {
        mt.SetUINT32(&mf::MF_MT_DEFAULT_STRIDE, w)
            .map_err(|e| CodecError::Encode(format!("MF_MT_DEFAULT_STRIDE: {e}")))?;
        mt.SetUINT32(&mf::MF_MT_SAMPLE_SIZE, w * h * 3 / 2)
            .map_err(|e| CodecError::Encode(format!("MF_MT_SAMPLE_SIZE: {e}")))?;
        mt.SetUINT32(&mf::MF_MT_FIXED_SIZE_SAMPLES, 1)
            .map_err(|e| CodecError::Encode(format!("MF_MT_FIXED_SIZE_SAMPLES: {e}")))?;
        mt.SetUINT32(&mf::MF_MT_ALL_SAMPLES_INDEPENDENT, 0)
            .map_err(|e| CodecError::Encode(format!("MF_MT_ALL_SAMPLES_INDEPENDENT: {e}")))?;
    }
    Ok(())
}

/// Plan B: si el MFT rechaza NUESTRO tipo, adopta uno de los suyos
/// (`GetInputAvailableType`/`GetOutputAvailableType`) con el subtipo y — si
/// se pasa `size` — las mismas dimensiones. `None` = sin filtro de dims (los
/// decoders anuncian plantillas como 1920×1080).
unsafe fn try_available_type(
    t: &mf::IMFTransform,
    input: bool,
    subtype: GUID,
    size: Option<(u32, u32)>,
) -> Result<bool, CodecError> {
    for i in 0..64u32 {
        let mt = if input {
            t.GetInputAvailableType(0, i)
        } else {
            t.GetOutputAvailableType(0, i)
        };
        let Ok(mt) = mt else { break }; // fin de la lista
        let Ok(sub) = mt.GetGUID(&mf::MF_MT_SUBTYPE) else {
            continue;
        };
        if sub != subtype {
            continue;
        }
        if let (Some((w, h)), Ok(fs)) = (size, mt.GetUINT64(&mf::MF_MT_FRAME_SIZE)) {
            if fs != (((w as u64) << 32) | h as u64) {
                continue;
            }
        }
        let r = if input {
            t.SetInputType(0, &mt, 0)
        } else {
            t.SetOutputType(0, &mt, 0)
        };
        if r.is_ok() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// C-33: fija un parámetro `CODECAPI_*` del MFT.
///
/// **Este es el camino correcto**: `IMFTransform::GetAttributes` expone los
/// atributos del transform (p. ej. `MF_TRANSFORM_ASYNC`), NO el almacén de
/// códecs. Los `CODECAPI_*` escritos ahí se ignoraban (campo 18/11: objetivo
/// de 15 Mbps y ~265 kbps reales, texto borroso en las tres calidades).
fn codec_api_u32(t: &mf::IMFTransform, api: &GUID, value: u32) -> bool {
    let Ok(capi) = t.cast::<mf::ICodecAPI>() else {
        return false;
    };
    let mut v: VARIANT = unsafe { VariantInit() };
    v.Anonymous.Anonymous = core::mem::ManuallyDrop::new(VARIANT_0_0 {
        vt: VT_UI4,
        wReserved1: 0,
        wReserved2: 0,
        wReserved3: 0,
        Anonymous: VARIANT_0_0_0 { ulVal: value },
    });
    unsafe { capi.IsSupported(api).is_ok() && capi.SetValue(api, &v).is_ok() }
}

/// Lectura de un `CODECAPI_*` (diagnóstico: confirma que el MFT lo aplicó).
fn codec_api_get_u32(t: &mf::IMFTransform, api: &GUID) -> Option<u32> {
    let capi = t.cast::<mf::ICodecAPI>().ok()?;
    let v = unsafe { capi.GetValue(api) }.ok()?;
    unsafe {
        if v.Anonymous.Anonymous.vt == VT_UI4 {
            Some(v.Anonymous.Anonymous.Anonymous.ulVal)
        } else {
            None
        }
    }
}

/// Configura tipos del MFT codificador: **salida H.264 primero** (orden
/// exigido por el MFT de Microsoft — observado en desarrollo), luego entrada
/// NV12. Devuelve el `profile_idc` aceptado (0 si hubo que adoptar un tipo
/// disponible del MFT).
unsafe fn configure_types(
    t: &mf::IMFTransform,
    w: u32,
    h: u32,
    fps: u32,
    bitrate: u32,
) -> Result<u32, CodecError> {
    // C-33: **Main (77, CABAC)** primero: ~10–15 % más eficiencia que
    // Baseline al mismo bitrate (CABAC + 8×8). Si el MFT no lo acepta, se
    // conserva el perfil Base (66) de antes (sin B-frames).
    let mut profile = 0u32;
    let mut accepted = false;
    for candidate in [mf::eAVEncH264VProfile_Main, mf::eAVEncH264VProfile_Base] {
        let outmt = video_type(mf::MFVideoFormat_H264)?;
        frame_size_attr(&outmt, w, h)?;
        common_video_attrs(&outmt, w, h, fps)?;
        outmt
            .SetUINT32(&mf::MF_MT_AVG_BITRATE, bitrate)
            .map_err(|e| CodecError::Encode(format!("MF_MT_AVG_BITRATE: {e}")))?;
        outmt
            .SetUINT32(&mf::MF_MT_MPEG2_PROFILE, candidate.0 as u32)
            .map_err(|e| CodecError::Encode(format!("MF_MT_MPEG2_PROFILE: {e}")))?;
        outmt
            .SetUINT32(&mf::MF_LOW_LATENCY, 1)
            .map_err(|e| CodecError::Encode(format!("MF_LOW_LATENCY: {e}")))?;
        // C-33: `conv.rs` entrega YUV de **rango completo** (0–255, sin
        // offset). Sin declararlo, el MFT lo trata como 16–235 y el
        // decodificador expande otra vez: medido en campo, el frame vuelve
        // como `1,108·fuente − 12,8` (contraste lavado, negros recortados).
        outmt
            .SetUINT32(
                &mf::MF_MT_VIDEO_NOMINAL_RANGE,
                mf::MFNominalRange_0_255.0 as u32,
            )
            .map_err(|e| CodecError::Encode(format!("MF_MT_VIDEO_NOMINAL_RANGE: {e}")))?;
        if t.SetOutputType(0, &outmt, 0).is_ok() {
            profile = candidate.0 as u32;
            accepted = true;
            break;
        }
    }
    if !accepted && !try_available_type(t, false, mf::MFVideoFormat_H264, Some((w, h)))? {
        return Err(CodecError::Encode(
            "SetOutputType rechazado (ni el tipo propio ni uno disponible)".into(),
        ));
    }

    let inmt = video_type(mf::MFVideoFormat_NV12)?;
    frame_size_attr(&inmt, w, h)?;
    common_video_attrs(&inmt, w, h, fps)?;
    nv12_raw_attrs(&inmt, w, h)?;
    if t.SetInputType(0, &inmt, 0).is_err()
        && !try_available_type(t, true, mf::MFVideoFormat_NV12, Some((w, h)))?
    {
        return Err(CodecError::Encode(
            "SetInputType rechazado (ni el tipo propio ni uno disponible)".into(),
        ));
    }
    Ok(profile)
}

/// C-33: política de rate-control por `ICodecAPI` (CBR con media = pico =
/// tope). `AVEncCommonQualityVsSpeed=0` se probó y PEORÓ — no se usa.
///
/// Devuelve qué parámetros aceptó el MFT (diagnóstico de campo).
fn apply_rate_control(
    t: &mf::IMFTransform,
    fps: u32,
    profile: u32,
    bitrate: u32,
) -> [(&'static str, bool); 6] {
    let rc = codec_api_u32(t, &mf::CODECAPI_AVEncCommonRateControlMode, 0); // CBR
    let mean = codec_api_u32(t, &mf::CODECAPI_AVEncCommonMeanBitRate, bitrate);
    let max = codec_api_u32(t, &mf::CODECAPI_AVEncCommonMaxBitRate, bitrate);
    // GOP de 2 s: refresco de imagen periódico sin disparar el bitrate.
    let gop_n = fps.saturating_mul(2).max(8);
    let gop = codec_api_u32(t, &mf::CODECAPI_AVEncMPVGOPSize, gop_n);
    // Sin B-frames (retardo mínimo; Main los permitiría).
    let bframes = codec_api_u32(t, &mf::CODECAPI_AVEncMPVDefaultBPictureCount, 0);
    let cabac = profile == mf::eAVEncH264VProfile_Main.0 as u32
        && codec_api_u32(t, &mf::CODECAPI_AVEncH264CABACEnable, 1);
    [
        ("rc_cbr", rc),
        ("mean", mean),
        ("max", max),
        ("gop", gop),
        ("bframes0", bframes),
        ("cabac", cabac),
    ]
}

/// Configura un MFT codificador completo (tipos + rate-control).
unsafe fn configure_encoder(
    t: &mf::IMFTransform,
    w: u32,
    h: u32,
    fps: u32,
    bitrate: u32,
) -> Result<(), CodecError> {
    let profile = configure_types(t, w, h, fps, bitrate)?;
    let applied = apply_rate_control(t, fps, profile, bitrate);
    let effective = codec_api_get_u32(t, &mf::CODECAPI_AVEncCommonMeanBitRate);
    let cur = unsafe { t.GetOutputCurrentType(0) }.ok();
    let cur_avg = cur
        .as_ref()
        .and_then(|m| unsafe { m.GetUINT32(&mf::MF_MT_AVG_BITRATE) }.ok());
    let cur_prof = cur
        .as_ref()
        .and_then(|m| unsafe { m.GetUINT32(&mf::MF_MT_MPEG2_PROFILE) }.ok());
    let name = unsafe { t.GetAttributes() }
        .ok()
        .and_then(|a| {
            let mut p = windows::core::PWSTR::null();
            let mut len = 0u32;
            a.GetAllocatedString(&mf::MFT_FRIENDLY_NAME_Attribute, &mut p, &mut len)
                .ok()
                .map(|_| p.to_string().unwrap_or_default())
        })
        .unwrap_or_default();
    eprintln!(
        "[video] h264 cfg: {w}x{h}@{fps} '{name}' profile={profile} bitrate={bitrate} \
         {applied:?} mean_efectivo={effective:?} tipo_actual=avg:{cur_avg:?}/prof:{cur_prof:?}"
    );
    Ok(())
}

/// Tamaño mínimo del buffer de salida: el máximo entre el NV12 crudo y el
/// `cbSize` que declara el MFT (`GetOutputStreamInfo`) + holgura.
fn output_slot_size(t: &mf::IMFTransform, nv12_len: usize) -> u32 {
    let cb = unsafe { t.GetOutputStreamInfo(0) }
        .map(|i| i.cbSize as usize)
        .unwrap_or(0);
    (cb.max(nv12_len) + 64 * 1024) as u32
}

/// Busca el primer MFT codificador utilizable (hardware → software) y lo
/// pone en modo streaming (la sonda síncrona descarta los MFT asíncronos:
/// devuelven `MF_E_TRANSFORM_ASYNC_LOCKED`).
/// C-35: ¿el MFT es asíncrono? (los de GPU — NVENC, DX12 — lo son).
fn mft_is_async(t: &mf::IMFTransform) -> bool {
    unsafe { t.GetAttributes() }
        .ok()
        .and_then(|a| unsafe { a.GetUINT32(&mf::MF_TRANSFORM_ASYNC) }.ok())
        .unwrap_or(0)
        != 0
}

/// Busca el primer MFT codificador utilizable: **GPU asíncrona primero**
/// (NVENC/DX12, con `MF_TRANSFORM_ASYNC_UNLOCK`) y, si nada sirve, el de
/// software síncrono de siempre. Devuelve `(MFT, es_asíncrono)`.
fn find_encoder_mft(
    w: u32,
    h: u32,
    fps: u32,
    bitrate: u32,
    allow_async: bool,
) -> Result<(mf::IMFTransform, bool), CodecError> {
    mf_ready()?;
    let in_t = type_info(mf::MFVideoFormat_NV12);
    let out_t = type_info(mf::MFVideoFormat_H264);
    let mut last = String::from("sin MFT enumerados");
    for act in enum_mfts(mf::MFT_CATEGORY_VIDEO_ENCODER, Some(&in_t), Some(&out_t))? {
        let t = match unsafe { act.ActivateObject::<mf::IMFTransform>() } {
            Ok(t) => t,
            Err(e) => {
                last = format!("ActivateObject: {e}");
                continue;
            }
        };
        let is_async = mft_is_async(&t);
        if is_async {
            if !allow_async {
                last = "asíncrono descartado (unlock desactivado)".into();
                continue;
            }
            // Desbloqueo OBLIGATORIO antes de fijar tipos (si no:
            // MF_E_TRANSFORM_ASYNC_LOCKED).
            if let Ok(a) = unsafe { t.GetAttributes() } {
                if unsafe { a.SetUINT32(&mf::MF_TRANSFORM_ASYNC_UNLOCK, 1) }.is_err() {
                    last = "ASYNC_UNLOCK rechazado".into();
                    continue;
                }
            }
        }
        if let Err(e) = unsafe { configure_encoder(&t, w, h, fps, bitrate) } {
            last = format!("{e:?}");
            continue;
        }
        if let Err(e) = unsafe { t.ProcessMessage(mf::MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0) } {
            last = format!("BEGIN_STREAMING: {e}");
            continue;
        }
        let _ = unsafe { t.ProcessMessage(mf::MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0) };
        return Ok((t, is_async));
    }
    Err(CodecError::Encode(format!(
        "sin MFT H.264 utilizable ({last})"
    )))
}

/// Muestra con su memoria reutilizable (sin allocs por frame).
struct SampleSlot {
    sample: mf::IMFSample,
    buffer: mf::IMFMediaBuffer,
}

fn make_slot(max: u32) -> Result<SampleSlot, CodecError> {
    unsafe {
        let sample =
            mf::MFCreateSample().map_err(|e| CodecError::Encode(format!("MFCreateSample: {e}")))?;
        let buffer = mf::MFCreateMemoryBuffer(max)
            .map_err(|e| CodecError::Encode(format!("MFCreateMemoryBuffer: {e}")))?;
        sample
            .AddBuffer(&buffer)
            .map_err(|e| CodecError::Encode(format!("AddBuffer: {e}")))?;
        Ok(SampleSlot { sample, buffer })
    }
}

impl SampleSlot {
    fn fill(&self, bytes: &[u8], time_100ns: i64, duration_100ns: i64) -> Result<(), CodecError> {
        unsafe {
            let mut ptr: *mut u8 = std::ptr::null_mut();
            self.buffer
                .Lock(&mut ptr, None, None)
                .map_err(|e| CodecError::Encode(format!("Lock: {e}")))?;
            if ptr.is_null() {
                return Err(CodecError::Encode("Lock devolvió puntero nulo".into()));
            }
            let max = self.buffer.GetMaxLength().unwrap_or(0);
            if bytes.len() as u32 > max {
                let _ = self.buffer.Unlock();
                return Err(CodecError::Encode(format!(
                    "frame de {} B excede el buffer de {max} B",
                    bytes.len()
                )));
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            self.buffer
                .Unlock()
                .map_err(|e| CodecError::Encode(format!("Unlock: {e}")))?;
            self.buffer
                .SetCurrentLength(bytes.len() as u32)
                .map_err(|e| CodecError::Encode(format!("SetCurrentLength: {e}")))?;
            self.sample
                .SetSampleTime(time_100ns)
                .map_err(|e| CodecError::Encode(format!("SetSampleTime: {e}")))?;
            // MF_E_NO_DURATION: el MFT de H.264 exige duración en la muestra.
            self.sample
                .SetSampleDuration(duration_100ns)
                .map_err(|e| CodecError::Encode(format!("SetSampleDuration: {e}")))?;
            Ok(())
        }
    }
}

/// Lee el contenido de cualquier `IMFSample` (contiguo).
fn read_sample(s: &mf::IMFSample) -> Result<Vec<u8>, CodecError> {
    unsafe {
        let buf = s
            .ConvertToContiguousBuffer()
            .map_err(|e| CodecError::Decode(format!("ConvertToContiguousBuffer: {e}")))?;
        let len = buf
            .GetCurrentLength()
            .map_err(|e| CodecError::Decode(format!("{e}")))? as usize;
        let mut out = vec![0u8; len];
        let mut ptr: *mut u8 = std::ptr::null_mut();
        buf.Lock(&mut ptr, None, None)
            .map_err(|e| CodecError::Decode(format!("Lock: {e}")))?;
        if !ptr.is_null() && len > 0 {
            std::ptr::copy_nonoverlapping(ptr, out.as_mut_ptr(), len);
        }
        buf.Unlock()
            .map_err(|e| CodecError::Decode(format!("Unlock: {e}")))?;
        Ok(out)
    }
}

/// Codificador H.264 (MF). La creación del MFT es **perezosa** (en el primer
/// frame, con sus dimensiones reales): si MF no sirve, `encode` devuelve
/// error y `video.rs` cae a VP9 con evento `video_backend`.
/// C-24: intervalo mínimo entre reconstrucciones del MFT marcadas por
/// `set_target_bitrate` — cada reconstrucción reinicia el calentamiento
/// (n inputs antes del primer slice emitido), así que con la cascada de
/// adaptación (cada ~3 s en WAN) y pantalla estática (~1 Hz) el encoder
/// nunca llegaba a emitir → sesión SIN vídeo (campo 2026-11-05).
const REBUILD_COALESCE: std::time::Duration = std::time::Duration::from_secs(6);

/// Posición del primer start code Annex-B (`00 00 01`; cubre también
/// `00 00 00 01` porque contiene `00 00 01` en la posición+1).
pub(crate) fn find_start_code(b: &[u8], from: usize) -> Option<usize> {
    if from >= b.len() {
        return None;
    }
    b[from..]
        .windows(3)
        .position(|w| w == [0, 0, 1])
        .map(|p| p + from)
}

/// ¿El buffer contiene un NAL **SPS** (tipo 7) delimitado por start codes?
/// Si el MFT ya lleva los parámetros in-band, NO se debe anteponer nada.
fn contains_sps(b: &[u8]) -> bool {
    let mut i = 0;
    while let Some(p) = find_start_code(b, i) {
        let long = b[p..].starts_with(&[0, 0, 0, 1]);
        let nal = p + if long { 4 } else { 3 };
        if let Some(first) = b.get(nal) {
            if first & 0x1F == 7 {
                return true;
            }
        }
        i = nal;
    }
    false
}

/// Convierte un blob estilo avcC (`[len_be32][NAL]…`) a Annex-B; `None` si
/// el recorrido no consume exactamente el buffer.
fn avc_c_to_annexb(b: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(b.len() + 8);
    let mut pos = 0;
    let mut nals = 0;
    while pos + 4 <= b.len() {
        let n = u32::from_be_bytes([b[pos], b[pos + 1], b[pos + 2], b[pos + 3]]) as usize;
        if n == 0 || n > 4096 || pos + 4 + n > b.len() {
            return None;
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&b[pos + 4..pos + 4 + n]);
        pos += 4 + n;
        nals += 1;
    }
    (pos == b.len() && nals >= 1).then_some(out)
}

/// Normaliza el blob `MF_MT_MPEG_SEQUENCE_HEADER` a Annex-B. Los MFT no
/// uniforman el formato: unos dan `00 00 00 01 SPS … PPS` y otros la
/// cabecera en formato **avcC** (longitud 4B por NAL). En juank2
/// (2026-11-11) el prepend crudo resultaba ilegible para el decoder y
/// **todos** los P-frames fallaban con `native:18` (243/261 frames).
fn normalize_seq_header(blob: Vec<u8>) -> Vec<u8> {
    if blob.is_empty() {
        return blob;
    }
    if blob.starts_with(&[0, 0, 0, 1]) || blob.starts_with(&[0, 0, 1]) {
        return blob; // ya Annex-B
    }
    match find_start_code(&blob, 0) {
        Some(p) => blob[p..].to_vec(), // basura delante → cortar
        None => avc_c_to_annexb(&blob) // ¿avcC?
            .unwrap_or_else(|| {
                let mut out = vec![0u8, 0, 0, 1];
                out.extend_from_slice(&blob);
                out
            }),
    }
}

/// C-28: tope de la cola de salida pendiente (≈0,5 s a15 fps).
const PENDING_CAP: usize = 8;

/// C-28: elige la pieza a emitir — lo MÁS VIEJO primero (el drenaje previo
/// corresponde al frame ANTERIOR; antes se tiraba con `let _ =` y se
/// perdían frames en interacción intensa: campo14/11 ≈1,5 fps emitidos).
/// La cola queda acotada a `cap` (con el MFT rezagado se descarta lo nuevo
/// para no acumular latencia sin límite).
fn pick_output(pending: &mut VecDeque<Vec<u8>>, current: Vec<u8>, cap: usize) -> Vec<u8> {
    match pending.pop_front() {
        Some(old) => {
            if !current.is_empty() && pending.len() < cap {
                pending.push_back(current);
            }
            old
        }
        None => current,
    }
}

pub struct MfH264Encoder {
    transform: Option<mf::IMFTransform>,
    fps: u32,
    bitrate: u32,
    width: u32, // efectivos (siempre pares: NV12)
    height: u32,
    seq: u64,
    nv12: Vec<u8>,
    input: Option<SampleSlot>,
    output: Option<SampleSlot>,
    /// SPS+PPS (`MF_MT_MPEG_SEQUENCE_HEADER`): se anteponen a cada frame
    /// para que cualquier NAL sea decodificable por sí solo (resiliencia).
    seq_hdr: Vec<u8>,
    /// C-24: última reconstrucción del MFT (coalescado de `set_target_bitrate`).
    last_rebuild: Option<std::time::Instant>,
    /// C-26: ¿ya se logueó el diagnóstico de framing de esta sesión?
    framing_logged: bool,
    /// C-28: salidas drenadas esperando turno (FIFO, tope [PENDING_CAP]).
    pending: VecDeque<Vec<u8>>,
    /// C-35: MFT asíncrono (GPU) → hay que bombear eventos.
    is_async: bool,
    /// C-35: si el camino asíncrono falla, no reintentarlo en esta sesión.
    allow_async: bool,
    /// C-35: el MFT avisó que quiere otra entrada (`METransformNeedInput`).
    need_input: bool,
    /// C-35: el MFT provee sus propias muestras de salida
    /// (`MFT_OUTPUT_STREAM_PROVIDES_SAMPLES`: NVENC/DX12).
    out_provides: bool,
}

// Los MFT son pointers COM sin `Send` (NonNull). Seguridad: ambos wrappers
// se crean y usan SIEMPRE en un único hilo (el hilo de vídeo / el bucle de
// sesión del controlador) y solo se mueven hacia él — patrón igual que el
// resto de backends de este crate.
unsafe impl Send for MfH264Encoder {}

impl MfH264Encoder {
    pub fn new(fps: u32, bitrate: u32) -> Self {
        Self {
            transform: None,
            fps: fps.max(1),
            bitrate: bitrate.max(100_000),
            width: 0,
            height: 0,
            seq: 0,
            nv12: Vec::new(),
            input: None,
            output: None,
            seq_hdr: Vec::new(),
            last_rebuild: None,
            framing_logged: false,
            pending: VecDeque::new(),
            is_async: false,
            // C-35: `ANYREMOTE_HW_ENCODER=0` fuerza el MFT síncrono (útil en
            // equipos con GPU problemática y para comparar en campo).
            allow_async: std::env::var("ANYREMOTE_HW_ENCODER").ok().as_deref() != Some("0"),
            need_input: true,
            out_provides: false,
        }
    }

    fn ensure(&mut self, w: u32, h: u32) -> Result<(), CodecError> {
        let ew = w & !1;
        let eh = h & !1;
        if ew < 16 || eh < 16 {
            return Err(CodecError::Encode(format!(
                "dims {w}x{h} no aptos para H.264"
            )));
        }
        if self.transform.is_some() && (ew, eh) == (self.width, self.height) {
            return Ok(());
        }
        // C-35: el camino asíncrono está validado hasta 1080p. A 4K NVENC deja
        // de emitir eventos tras el primer frame (medido en este equipo), así
        // que por encima de 1080p se usa el MFT síncrono de siempre.
        let pixeles = (ew as u64) * (eh as u64);
        let permitir_async = self.allow_async && pixeles <= 1920 * 1080;
        if self.allow_async && !permitir_async {
            eprintln!(
                "[video] h264: {ew}x{eh} > 1080p → encoder de GPU desactivado (MFT síncrono)"
            );
        }
        let (t, is_async) = find_encoder_mft(ew, eh, self.fps, self.bitrate, permitir_async)?;
        let nv12_len = (ew as usize) * (eh as usize) * 3 / 2;
        let input = make_slot(nv12_len as u32)?;
        // Tamaño de salida = cbSize DECLARADO por el MFT (no el nuestro):
        // a 720p el de Microsoft exige más que `raw+64k` y devuelve
        // MF_E_BUFFERTOOSMALL (reporte de campo en v0.9.0). Los MFT de GPU
        // proveen la muestra (cbSize=0): no se reserva slot.
        let out_provides = unsafe { t.GetOutputStreamInfo(0) }
            .map(|i| i.dwFlags & mf::MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0)
            .unwrap_or(false);
        let output = if out_provides {
            None
        } else {
            Some(make_slot(output_slot_size(&t, nv12_len))?)
        };
        self.is_async = is_async;
        self.out_provides = out_provides;
        self.need_input = true;
        self.transform = Some(t);
        self.last_rebuild = Some(std::time::Instant::now());
        self.width = ew;
        self.height = eh;
        self.nv12.resize(nv12_len, 0);
        self.input = Some(input);
        self.output = output;
        self.seq_hdr.clear();
        Ok(())
    }

    /// Carga SPS+PPS desde el tipo de salida actual (cuando exista).
    fn load_seq_header(&mut self, t: &mf::IMFTransform) {
        if !self.seq_hdr.is_empty() {
            return;
        }
        unsafe {
            if let Ok(mt) = t.GetOutputCurrentType(0) {
                if let Ok(size) = mt.GetBlobSize(&mf::MF_MT_MPEG_SEQUENCE_HEADER) {
                    if size > 0 && size < 4096 {
                        let mut buf = vec![0u8; size as usize];
                        if mt
                            .GetBlob(&mf::MF_MT_MPEG_SEQUENCE_HEADER, &mut buf, None)
                            .is_ok()
                        {
                            // C-26: normalizar a Annex-B en el límite (avcC →
                            // start codes) — el prepend de `encode` solo
                            // entiende Annex-B.
                            self.seq_hdr = normalize_seq_header(buf);
                        }
                    }
                }
            }
        }
    }

    /// Empaqueta BGRA → NV12 en `self.nv12` (recorta a dims pares).
    fn pack_nv12(&mut self, frame: &Frame) -> Result<(), CodecError> {
        let (ew, eh) = (self.width, self.height);
        let cropped;
        let bgra: &[u8] = if frame.width == ew && frame.height == eh {
            &frame.data
        } else {
            cropped = crop_bgra(&frame.data, frame.width, ew, eh);
            &cropped
        };
        let len = (ew as usize) * (eh as usize) * 3 / 2;
        if self.nv12.len() != len {
            self.nv12.resize(len, 0);
        }
        bgra_to_nv12_into(ew, eh, bgra, &mut self.nv12);
        Ok(())
    }

    /// Saca una muestra codificada (o `None` si el MFT aún no tiene salida).
    ///
    /// NOTA C-14: este MFT devuelve `pdwstatus=0` **aunque ESCRIBE la
    /// muestra** — descartar por el bit `SAMPLE_READY` tiraba NALs reales.
    fn drain(&self, t: &mf::IMFTransform) -> Result<Option<Vec<u8>>, CodecError> {
        let out_sample = self
            .output
            .as_ref()
            .map(|s| s.sample.clone())
            .ok_or_else(|| CodecError::Encode("sin muestra de salida".into()))?;
        let mut data = mf::MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: core::mem::ManuallyDrop::new(Some(out_sample)),
            dwStatus: 0,
            pEvents: core::mem::ManuallyDrop::new(None),
        };
        let mut status: u32 = 0;
        let res = unsafe { t.ProcessOutput(0, std::slice::from_mut(&mut data), &mut status) };
        let produced: Option<mf::IMFSample> =
            unsafe { core::mem::ManuallyDrop::take(&mut data.pSample) };
        let _events = unsafe { core::mem::ManuallyDrop::take(&mut data.pEvents) };
        let _ = status;
        match res {
            Ok(()) => {
                let s = produced.ok_or_else(|| CodecError::Encode("MFT sin muestra".into()))?;
                let data = read_sample(&s)?;
                if data.is_empty() {
                    return Ok(None); // Ok sin payload (retardo real)
                }
                Ok(Some(data))
            }
            Err(e) if e.code() == mf::MF_E_TRANSFORM_NEED_MORE_INPUT => Ok(None),
            // Prefijo estable para que `encode` reasiente el buffer y reintente.
            Err(e) if e.code() == mf::MF_E_BUFFERTOOSMALL => {
                Err(CodecError::Encode(format!("MF_E_BUFFERTOOSMALL: {e}")))
            }
            Err(e) => Err(CodecError::Encode(format!("ProcessOutput: {e}"))),
        }
    }
}

impl MfH264Encoder {
    /// C-35: cierra el frame (diagnóstico de framing, prepend de SPS/PPS y
    /// secuencia) — común a los caminos síncrono y asíncrono.
    fn finish(&mut self, t: &mf::IMFTransform, payload: Vec<u8>) -> EncodedFrame {
        let mut payload = payload;
        // C-35: `GetOutputCurrentType`/`GetBlob` en un MFT asíncrono cuesta
        // ~40 ms por frame. La cabecera solo se necesita si el payload NO
        // trae SPS in-band: se carga bajo demanda (y se cachea).
        if !payload.is_empty() && !contains_sps(&payload) && self.seq_hdr.is_empty() {
            self.load_seq_header(t);
        }
        // C-26: diagnóstico de framing (una vez por sesión).
        if !self.framing_logged && !payload.is_empty() {
            self.framing_logged = true;
            eprintln!(
                "[video] h264 framing: seq_hdr[{}]={:02x?} payload[0..{}]={:02x?} con_sps={}",
                self.seq_hdr.len(),
                &self.seq_hdr[..self.seq_hdr.len().min(16)],
                payload.len().min(16),
                &payload[..payload.len().min(16)],
                contains_sps(&payload),
            );
        }
        // C-26: prepend SOLO si el payload no trae ya los parámetros
        // in-band (si los trae, duplicarlos/enredarlos envenena el stream).
        if !payload.is_empty()
            && !self.seq_hdr.is_empty()
            && !payload.starts_with(&self.seq_hdr)
            && !contains_sps(&payload)
        {
            let mut with_hdr = self.seq_hdr.clone();
            with_hdr.extend_from_slice(&payload);
            payload = with_hdr;
        }
        let seq = self.seq;
        self.seq += 1;
        EncodedFrame {
            format: CodecFormat::H264,
            seq,
            width: self.width,
            height: self.height,
            data: payload,
        }
    }

    /// C-35: saca la muestra que el MFT provee (NVENC/DX12) o reutiliza
    /// nuestro slot (MFT que no provee muestras).
    fn process_output_async(
        &mut self,
        t: &mf::IMFTransform,
    ) -> Result<Option<Vec<u8>>, CodecError> {
        let supplied = if self.out_provides {
            None
        } else {
            self.output.as_ref().map(|s| s.sample.clone())
        };
        let mut data = mf::MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: core::mem::ManuallyDrop::new(supplied),
            dwStatus: 0,
            pEvents: core::mem::ManuallyDrop::new(None),
        };
        let mut status = 0u32;
        let res = unsafe { t.ProcessOutput(0, std::slice::from_mut(&mut data), &mut status) };
        let produced: Option<mf::IMFSample> =
            unsafe { core::mem::ManuallyDrop::take(&mut data.pSample) };
        let _events = unsafe { core::mem::ManuallyDrop::take(&mut data.pEvents) };
        let _ = status;
        match res {
            Ok(()) => {
                let Some(s) = produced else {
                    return Ok(None);
                };
                let bytes = read_sample(&s)?;
                Ok(if bytes.is_empty() { None } else { Some(bytes) })
            }
            Err(e) if e.code() == mf::MF_E_TRANSFORM_NEED_MORE_INPUT => Ok(None),
            Err(e) => Err(CodecError::Encode(format!("ProcessOutput async: {e}"))),
        }
    }

    /// C-35: consume los eventos del MFT asíncrono y recoge las salidas.
    fn pump_async(
        &mut self,
        t: &mf::IMFTransform,
        outs: &mut Vec<Vec<u8>>,
    ) -> Result<(), CodecError> {
        let gen: mf::IMFMediaEventGenerator = t
            .cast()
            .map_err(|e| CodecError::Encode(format!("IMFMediaEventGenerator: {e}")))?;
        loop {
            let ev = match unsafe { gen.GetEvent(mf::MF_EVENT_FLAG_NO_WAIT) } {
                Ok(e) => e,
                Err(e) if e.code() == mf::MF_E_NO_EVENTS_AVAILABLE => break,
                Err(e) => return Err(CodecError::Encode(format!("GetEvent: {e}"))),
            };
            let ty = unsafe { ev.GetType() }.unwrap_or(0);
            match ty {
                ty if ty == mf::METransformNeedInput.0 as u32 => self.need_input = true,
                ty if ty == mf::METransformHaveOutput.0 as u32 => {
                    if let Some(bytes) = self.process_output_async(t)? {
                        outs.push(bytes);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// C-35: fallo del camino asíncrono → se descarta el MFT de GPU y el
    /// siguiente frame reconstruye con el síncrono. Nunca tumba la sesión.
    fn async_failed(&mut self, err: CodecError) -> Result<EncodedFrame, CodecError> {
        if !self.allow_async {
            return Err(err);
        }
        self.allow_async = false;
        self.transform = None;
        self.pending.clear();
        self.seq_hdr.clear();
        self.framing_logged = false;
        eprintln!("[video] h264: MFT de GPU falló ({err:?}) → se usa el síncrono");
        let seq = self.seq;
        self.seq += 1;
        Ok(EncodedFrame {
            format: CodecFormat::H264,
            seq,
            width: self.width,
            height: self.height,
            data: Vec::new(),
        })
    }

    /// C-35: un frame por el camino asíncrono (NVENC/DX12): bombea eventos,
    /// envía la entrada cuando el MFT la pide y recoge lo que haya.
    fn encode_async(
        &mut self,
        frame: &Frame,
        t: &mf::IMFTransform,
    ) -> Result<EncodedFrame, CodecError> {
        let mut outs = Vec::new();
        if let Err(e) = self.pump_async(t, &mut outs) {
            return self.async_failed(e);
        }
        let mut submitted = false;
        if self.need_input {
            if let Err(e) = self.pack_nv12(frame) {
                return self.async_failed(e);
            }
            let fps = self.fps.max(1) as i64;
            let time = (self.seq as i64) * 10_000_000 / fps;
            let dur = 10_000_000 / fps;
            // C-35: un MFT asíncrono RETIENE la muestra hasta que la codifica
            // (a 4K son ~25 ms). Reutilizar el mismo `IMFSample` mientras está
            // en vuelo dejaba al encoder sin eventos tras el primer frame: se
            // crea una muestra por envío.
            let input = make_slot(self.nv12.len() as u32)?;
            input.fill(&self.nv12, time, dur)?;
            match unsafe { t.ProcessInput(0, &input.sample, 0) } {
                Ok(()) => {
                    submitted = true;
                    self.need_input = false;
                }
                // El MFT aún no pidió entrada: se descarta este frame (el
                // bucle de reenvío del daemon lo reintenta).
                Err(e) if e.code() == mf::MF_E_NOTACCEPTING => {}
                Err(e) => {
                    return self
                        .async_failed(CodecError::Encode(format!("ProcessInput async: {e}")))
                }
            }
        }
        if submitted {
            // C-35: algunos MFT (NVENC a 4K) dejan la salida lista SIN emitir
            // `METransformHaveOutput`: se prueba un `ProcessOutput` directo
            // antes de esperar eventos.
            if let Ok(Some(bytes)) = self.process_output_async(t) {
                outs.push(bytes);
            }
            // C-35: el MFT de GPU es asíncrono — la salida llega en milisegundos.
            // Se espera un presupuesto corto (el mismo hilo de vídeo duerme
            // después) en vez de devolver vacío y perder el frame: con la
            // pantalla estática el reenvío es a 1 Hz y el frame llegaría 1 s
            // tarde.
            // OJO: `sleep(1ms)` en Windows se redondea a ~15,6 ms (resolución
            // del temporizador) y multiplicaba el coste por frame. Se sondea
            // con `yield_now` y un presupuesto corto: NVENC responde en ~4 ms.
            const ASYNC_WAIT_MS: u64 = 10;
            let deadline =
                std::time::Instant::now() + std::time::Duration::from_millis(ASYNC_WAIT_MS);
            loop {
                if let Err(e) = self.pump_async(t, &mut outs) {
                    return self.async_failed(e);
                }
                if !outs.is_empty() || std::time::Instant::now() >= deadline {
                    break;
                }
                std::thread::yield_now();
            }
        }
        let mut payload = Vec::new();
        for out in outs {
            if payload.is_empty() {
                payload = out;
            } else if self.pending.len() < PENDING_CAP {
                self.pending.push_back(out);
            }
        }
        let payload = pick_output(&mut self.pending, payload, PENDING_CAP);
        Ok(self.finish(t, payload))
    }
}

impl VideoEncoder for MfH264Encoder {
    fn encode(&mut self, frame: &Frame) -> Result<EncodedFrame, CodecError> {
        self.ensure(frame.width, frame.height)?;
        let t = self
            .transform
            .clone()
            .ok_or_else(|| CodecError::Encode("sin MFT".into()))?;
        if self.is_async {
            return self.encode_async(frame, &t);
        }
        // Si la entrada no está libre, drena antes (evita MF_E_NOTACCEPTING).
        // C-28: ese drenaje traía la salida PENDIENTE del frame ANTERIOR —
        // antes se TIRABA (`let _ =`) y se perdían frames sistemáticamente
        // en interacción intensa (campo14/11: ~1,5 fps emitidos con la
        // captura a15). Ahora se conserva para emitirla en orden (FIFO).
        let status = unsafe { t.GetInputStatus(0) }.unwrap_or(1);
        let pre_raw = if status & 1 == 0 {
            self.drain(&t)?
        } else {
            None
        };
        self.pack_nv12(frame)?;
        let fps = self.fps.max(1) as i64;
        let time = (self.seq as i64) * 10_000_000 / fps;
        let dur = 10_000_000 / fps;
        let input = self
            .input
            .as_ref()
            .ok_or_else(|| CodecError::Encode("sin muestra de entrada".into()))?;
        input.fill(&self.nv12, time, dur)?;
        unsafe {
            t.ProcessInput(0, &input.sample, 0)
                .map_err(|e| CodecError::Encode(format!("ProcessInput: {e}")))?;
        }
        let payload = match self.drain(&t) {
            Ok(v) => v.unwrap_or_default(),
            Err(err) if matches!(&err, CodecError::Encode(m) if m.starts_with("MF_E_BUFFERTOOSMALL")) =>
            {
                // El MFT pide más buffer del preparado (p. ej. primer GOP
                // grande a 720p): se agranda y se reintenta UNA vez; si
                // vuelve a fallar → error real (el llamador hace fallback).
                let grown = self
                    .output
                    .as_ref()
                    .and_then(|s| unsafe { s.buffer.GetMaxLength() }.ok())
                    .map(|cur| (cur as usize) * 2)
                    .unwrap_or(0);
                let nv = (self.width as usize) * (self.height as usize) * 3 / 2;
                let want = grown.max(nv + 65_536) as u32;
                let mut retried = false;
                if let Ok(slot) = make_slot(want) {
                    self.output = Some(slot);
                    retried = true;
                }
                if retried {
                    self.drain(&t)?.unwrap_or_default()
                } else {
                    return Err(err);
                }
            }
            Err(e) => return Err(e),
        };
        // C-28: emite lo MÁS VIEJO primero (el pre-drenaje corresponde al
        // frame anterior); la cola queda acotada a [PENDING_CAP].
        if let Some(raw) = pre_raw {
            if self.pending.len() < PENDING_CAP {
                self.pending.push_back(raw);
            }
        }
        let payload = pick_output(&mut self.pending, payload, PENDING_CAP);
        Ok(self.finish(&t, payload))
    }

    fn format(&self) -> CodecFormat {
        CodecFormat::H264
    }

    fn set_target_bitrate(&mut self, bps: usize) -> bool {
        let bps = (bps as u32).clamp(100_000, 40_000_000);
        if bps == self.bitrate {
            return true;
        }
        self.bitrate = bps;
        // C-33: si el MFT está vivo, aplica la tasa EN CALIENTE por
        // `ICodecAPI` — sin reconstruir. La reconstrucción reinicia el
        // calentamiento del MFT y en campo (C-24) dejó al visor sin vídeo.
        if let Some(t) = self.transform.as_ref() {
            let ok = codec_api_u32(t, &mf::CODECAPI_AVEncCommonMeanBitRate, bps)
                && codec_api_u32(t, &mf::CODECAPI_AVEncCommonMaxBitRate, bps);
            if ok {
                return true;
            }
        }
        // Reasignación DIFERIDA (v0.9.1): un `SetOutputType` en caliente
        // (con entradas ya en cola) dejó al MFT sin emitir — observado a
        // 720p en campo. Se marca reconstrucción y el PRÓXIMO frame rehace
        // la configuración completa (camino que sí funciona), con el cbSize
        // nuevo de `GetOutputStreamInfo`.
        // C-24 (coalescado): cada reconstrucción reinicia el calentamiento
        // del MFT; con la cascada de adaptación (~3 s en WAN) y pantalla
        // estática (~1 Hz de reenvío) el encoder no emitía NADA en toda la
        // sesión → visor en negro. Máx. una cada [REBUILD_COALESCE]; el
        // bitrate pendiente se aplica en la próxima reconstrucción (usa
        // `self.bitrate`, que ya está actualizado).
        let fresh = self
            .last_rebuild
            .map(|t| t.elapsed() >= REBUILD_COALESCE)
            .unwrap_or(true);
        if fresh {
            self.transform = None;
            self.seq_hdr.clear();
            self.pending.clear(); // salidas del MFT anterior (seq_hdr nuevo)
        }
        true
    }

    fn set_fps(&mut self, fps: u32) {
        self.fps = fps.max(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PixelFormat, VideoDecoder}; // el roundtrip decodifica con NativeDecoder
    use anyremote_capture::TestPatternCapturer;

    /// Sonda + roundtrip completo con el MFT que haya en la máquina
    /// (software de Windows en CI; hardware si hay un MFT sync de GPU).
    #[test]
    fn h264_encode_decode_roundtrip_preserves_geometry() {
        if mf_ready().is_err() {
            eprintln!("MF no disponible en esta máquina: se omite");
            return;
        }
        let frame = TestPatternCapturer::render(320, 180, 7);
        let mut enc = MfH264Encoder::new(15, 600_000);
        let mut dec = crate::NativeDecoder::new();
        // Semántica de stream: los primeros NALs pueden ser solo SPS/PPS
        // (NEED_MORE_INPUT) hasta que llegue un slice — como en la sesión
        // real, se itera hasta que el stream decodifica.
        let mut out: Option<Frame> = None;
        let mut last: Option<EncodedFrame> = None;
        let mut nals = 0usize;
        let mut first_err = String::new();
        for _ in 0..40 {
            let f = enc.encode(&frame).expect("encode H.264");
            if f.data.is_empty() {
                continue;
            }
            assert_eq!(f.format, CodecFormat::H264);
            assert_eq!((f.width, f.height), (320, 180), "dims pares preservados");
            assert!(
                f.data.windows(4).any(|w| w == [0, 0, 0, 1]),
                "Annex B con start code"
            );
            nals += 1;
            last = Some(f);
            match dec.decode(last.as_ref().unwrap()) {
                Ok(fr) => {
                    out = Some(fr);
                    break;
                }
                Err(CodecError::Decode(e)) => {
                    if first_err.is_empty() {
                        first_err = e.clone();
                    }
                    if e.contains("NEED_MORE_INPUT") {
                        continue;
                    }
                    panic!("decode H.264: {e:?}");
                }
                Err(e) => panic!("decode H.264: {e:?}"),
            }
        }
        let out = out.unwrap_or_else(|| {
            panic!(
                "stream no decodificó: nals={nals} last_len={:?} primera_err={first_err}",
                last.as_ref().map(|f| f.data.len())
            )
        });
        let _ = last;
        assert_eq!((out.width, out.height), (320, 180));
        assert_eq!(out.format, PixelFormat::Bgra8);
        assert_eq!(out.data.len(), 320 * 180 * 4);
        assert!(out.data.iter().any(|&b| b > 0), "no todo negro");
    }

    /// Vuelca el stream a un archivo para validación externa (ffmpeg).
    #[test]
    fn dump_h264_for_external_check() {
        if mf_ready().is_err() {
            return;
        }
        let frame = TestPatternCapturer::render(320, 180, 7);
        let mut enc = MfH264Encoder::new(15, 600_000);
        let mut all = Vec::new();
        let mut frames = 0usize;
        for _ in 0..60 {
            let f = enc.encode(&frame).expect("encode");
            if !f.data.is_empty() {
                all.extend_from_slice(&f.data);
                frames += 1;
            }
        }
        let path = std::env::temp_dir().join("anyremote_c14.h264");
        std::fs::write(&path, &all).expect("write");
        eprintln!(
            "DUMP {} frames, {} bytes -> {}",
            frames,
            all.len(),
            path.display()
        );
    }

    /// Regresión de campo (v0.9.0 → v0.9.1): a **1280×720** con cambios de
    /// tasa (como los de `AdaptState`), el MFT exigía más buffer que
    /// `raw+64k` y devolvía `MF_E_BUFFERTOOSMALL` (0xC00D36B1) → fallback a
    /// VP9 en el anfitrión. Ahora el slot sale de `GetOutputStreamInfo` y
    /// hay reintento al agrandar.
    #[test]
    fn h264_720p_with_bitrate_changes_does_not_overflow() {
        if mf_ready().is_err() {
            eprintln!("MF no disponible: se omite");
            return;
        }
        let frame = TestPatternCapturer::render(1280, 720, 11);
        let mut enc = MfH264Encoder::new(15, 2_000_000);
        let mut dec = crate::NativeDecoder::new();
        let mut produced = 0usize;
        let mut decoded: Option<Frame> = None;
        // Calentamiento del MFT a720p ≈16 frames + 2 reconstrucciones
        // (i=5 y i=15) que lo reinician: 50 deja holgura (el primero sale
        // en i≈31).
        for i in 0..50 {
            if i == 5 {
                // igual que AdaptState al crearse: tasa ≠ inicial
                assert!(enc.set_target_bitrate(3_000_000));
            }
            if i == 15 {
                // adaptación en caliente (C-13)
                assert!(enc.set_target_bitrate(1_500_000));
            }
            let f = enc
                .encode(&frame)
                .expect("encode 720p sin MF_E_BUFFERTOOSMALL");
            if !f.data.is_empty() {
                produced += 1;
                assert_eq!((f.width, f.height), (1280, 720));
                if decoded.is_none() {
                    match dec.decode(&f) {
                        Ok(fr) => decoded = Some(fr),
                        Err(CodecError::Decode(e)) if e.contains("NEED_MORE_INPUT") => {}
                        Err(e) => panic!("decode 720p: {e:?}"),
                    }
                }
            }
        }
        assert!(produced > 0, "frames a 720p");
        let out = decoded.expect("algún frame 720p decodifica con OpenH264");
        assert_eq!((out.width, out.height), (1280, 720));
    }

    /// C-28: el pre-drenaje (frame anterior) se emite ANTES que el actual
    /// y la cola queda acotada (antes ese drenaje se tiraba → ~1,5 fps).
    #[test]
    fn pick_output_keeps_order_and_caps_pending() {
        // Sin cola → sale el actual.
        let mut pending = VecDeque::new();
        assert_eq!(pick_output(&mut pending, vec![1], 8), vec![1]);
        // Pre-drenaje (más viejo) primero; el actual queda encolado (FIFO).
        pending.push_back(vec![9]);
        assert_eq!(pick_output(&mut pending, vec![2], 8), vec![9]);
        assert_eq!(pick_output(&mut pending, vec![3], 8), vec![2]);
        assert_eq!(pick_output(&mut pending, Vec::new(), 8), vec![3]);
        // Acotado: pre-drenaje + actual en cada vuelta no desborda.
        let mut pending = VecDeque::new();
        for i in 0..30u8 {
            pending.push_back(vec![0xF0]);
            let _ = pick_output(&mut pending, vec![i], 8);
        }
        assert!(pending.len() <= 8, "cola acotada: {}", pending.len());
        // Actual vacío → no encola basura.
        let mut pending = VecDeque::new();
        assert!(pick_output(&mut pending, Vec::new(), 8).is_empty());
    }

    /// C-26: la cabecera del MFT se normaliza a Annex-B en el límite
    /// (avcC → start codes) — sin esto el prepend producía P-frames
    /// ilegibles en juank2 (`native:18`,243/261 frames perdidos).
    #[test]
    fn seq_header_normalizes_to_annexb() {
        // Annex-B puro → intacto.
        let ab = vec![0, 0, 0, 1, 0x67, 0x42, 0, 0, 1, 0x68, 0xEE];
        assert_eq!(normalize_seq_header(ab.clone()), ab);
        // Con basura delante → se corta desde el start code.
        // Basura delante: se corta desde el primer start code (aquí de3
        // bytes: `00 00 01`, también Annex-B válido).
        let dirty = vec![0xFF, 0x00, 0x00, 0x01, 0x67, 0x42];
        assert_eq!(normalize_seq_header(dirty), vec![0, 0, 1, 0x67, 0x42]);
        // avcC ([len4B][NAL]…) → Annex-B.
        let mut acc = Vec::new();
        let sps = [0x67u8, 0x42, 0x00, 0x1E];
        let pps = [0x68u8, 0xCE, 0x06];
        acc.extend_from_slice(&(sps.len() as u32).to_be_bytes());
        acc.extend_from_slice(&sps);
        acc.extend_from_slice(&(pps.len() as u32).to_be_bytes());
        acc.extend_from_slice(&pps);
        assert_eq!(
            normalize_seq_header(acc),
            vec![0, 0, 0, 1, 0x67, 0x42, 0x00, 0x1E, 0, 0, 0, 1, 0x68, 0xCE, 0x06]
        );
        // Ni Annex-B ni avcC → se antepone un start code (último recurso).
        let out = normalize_seq_header(vec![0x67, 0x42, 0x00]);
        assert!(out.starts_with(&[0, 0, 0, 1]));
    }

    /// C-26: detección de SPS in-band para NO duplicar la cabecera.
    #[test]
    fn contains_sps_detects_in_band_headers() {
        assert!(contains_sps(&[0, 0, 0, 1, 0x67, 0x42, 0, 0, 1, 0x41]));
        assert!(!contains_sps(&[0, 0, 0, 1, 0x41, 0x9A]), "P-slice solo");
        assert!(!contains_sps(&[0, 0, 0, 1, 0x68, 0xCE]), "PPS no es SPS");
        assert!(!contains_sps(&[0x67, 0x42]), "sin start codes no cuenta");
    }

    /// C-33: un cambio de tasa sobre un MFT vivo se aplica **en vivo** por
    /// `ICodecAPI` (sin reconstruir: cada reconstrucción reinicia el
    /// calentamiento y en campo —C-24— dejó al visor sin vídeo). Si el MFT no
    /// admite el cambio en vivo, cae a la reconstrucción diferida de siempre.
    #[test]
    fn bitrate_change_is_applied_live_or_rebuilt() {
        if mf_ready().is_err() {
            eprintln!("MF no disponible en esta máquina: se omite");
            return;
        }
        let mut enc = MfH264Encoder::new(15, 1_000_000);
        enc.ensure(320, 180).expect("ensure inicial");
        let t = enc.transform.clone().expect("MFT vivo");
        assert!(enc.set_target_bitrate(700_000));
        assert_eq!(enc.bitrate, 700_000);
        let live = codec_api_get_u32(&t, &mf::CODECAPI_AVEncCommonMeanBitRate);
        let rebuilt = enc.transform.is_none();
        assert!(
            live == Some(700_000) || rebuilt,
            "la tasa debe quedar aplicada (viva o reconstruyendo): {live:?}"
        );
        // Un segundo cambio tampoco puede perder la tasa.
        assert!(enc.set_target_bitrate(400_000));
        assert_eq!(enc.bitrate, 400_000);
        if let Some(t) = enc.transform.as_ref() {
            assert_eq!(
                codec_api_get_u32(t, &mf::CODECAPI_AVEncCommonMeanBitRate),
                Some(400_000)
            );
        }
    }

    /// Recorte a dims pares: un frame impar se recorta (1 px) sin error.
    #[test]
    fn odd_dimensions_are_cropped_to_even() {
        if mf_ready().is_err() {
            return;
        }
        let frame = TestPatternCapturer::render(321, 181, 3);
        let mut enc = MfH264Encoder::new(15, 500_000);
        let f = enc.encode(&frame).expect("encode con dims impares");
        assert_eq!((f.width, f.height), (320, 180));
    }
}
