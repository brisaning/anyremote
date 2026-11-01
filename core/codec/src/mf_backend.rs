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

use std::sync::OnceLock;

use anyremote_capture::{Frame, PixelFormat};
use windows::core::GUID;
use windows::Win32::Media::MediaFoundation as mf;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};

use crate::conv::{bgra_to_i420, crop_bgra, i420_to_bgra, i420_to_nv12, nv12_to_i420};
use crate::{CodecError, CodecFormat, EncodedFrame, VideoDecoder, VideoEncoder};

/// `MFStartup` una vez por proceso; `CoInitializeEx` por hilo (idempotente).
fn mf_ready() -> Result<(), CodecError> {
    static START: OnceLock<Result<(), String>> = OnceLock::new();
    unsafe {
        // S_FALSE/RPC_E_CHANGED_MODE son inofensivos (hilo ya inicializado).
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    START
        .get_or_init(|| unsafe {
            mf::MFStartup(mf::MF_VERSION, mf::MFSTARTUP_FULL)
                .map_err(|e| format!("MFStartup: {e}"))
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

/// Renegocia SOLO la tasa del tipo de salida (la entrada NV12 no cambia: es
/// lo que los MFT aceptan en caliente; si lo rechazan, el llamador
/// reconstruye el MFT en el próximo frame).
unsafe fn retype_output(
    t: &mf::IMFTransform,
    w: u32,
    h: u32,
    fps: u32,
    bitrate: u32,
) -> Result<(), CodecError> {
    let outmt = video_type(mf::MFVideoFormat_H264)?;
    frame_size_attr(&outmt, w, h)?;
    common_video_attrs(&outmt, w, h, fps)?;
    outmt
        .SetUINT32(&mf::MF_MT_AVG_BITRATE, bitrate)
        .map_err(|e| CodecError::Encode(format!("MF_MT_AVG_BITRATE: {e}")))?;
    outmt
        .SetUINT32(&mf::MF_MT_MPEG2_PROFILE, mf::eAVEncH264VProfile_Base.0 as u32)
        .map_err(|e| CodecError::Encode(format!("MF_MT_MPEG2_PROFILE: {e}")))?;
    outmt
        .SetUINT32(&mf::MF_LOW_LATENCY, 1)
        .map_err(|e| CodecError::Encode(format!("MF_LOW_LATENCY: {e}")))?;
    t.SetOutputType(0, &outmt, 0)
        .map_err(|e| CodecError::Encode(format!("SetOutputType: {e}")))?;
    Ok(())
}

/// Configura un MFT codificador: **salida H.264 primero** (orden exigido por
/// el MFT de Microsoft — observado en desarrollo), luego entrada NV12.
unsafe fn configure_encoder(
    t: &mf::IMFTransform,
    w: u32,
    h: u32,
    fps: u32,
    bitrate: u32,
) -> Result<(), CodecError> {
    let outmt = video_type(mf::MFVideoFormat_H264)?;
    frame_size_attr(&outmt, w, h)?;
    common_video_attrs(&outmt, w, h, fps)?;
    outmt
        .SetUINT32(&mf::MF_MT_AVG_BITRATE, bitrate)
        .map_err(|e| CodecError::Encode(format!("MF_MT_AVG_BITRATE: {e}")))?;
    // Perfil Baseline (66): sin B-frames → retardo mínimo. Sin nivel fijo
    // (lo negocia el MFT por resolución/bitrate).
    outmt
        .SetUINT32(&mf::MF_MT_MPEG2_PROFILE, mf::eAVEncH264VProfile_Base.0 as u32)
        .map_err(|e| CodecError::Encode(format!("MF_MT_MPEG2_PROFILE: {e}")))?;
    outmt
        .SetUINT32(&mf::MF_LOW_LATENCY, 1)
        .map_err(|e| CodecError::Encode(format!("MF_LOW_LATENCY: {e}")))?;
    if t.SetOutputType(0, &outmt, 0).is_err()
        && !try_available_type(t, false, mf::MFVideoFormat_H264, Some((w, h)))?
    {
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

    // Rate-control CBR (atributos CODECAPI del MFT): el VBR por defecto usa
    // lookahead y retiene frames. NOTA: `AVEncCommonQualityVsSpeed=0` se
    // probó y PEORÓ (dejó de emitir incluso en flush) — no se usa.
    if let Ok(attrs) = t.GetAttributes() {
        let _ = attrs.SetUINT32(&mf::CODECAPI_AVEncCommonRateControlMode, 0);
    }
    Ok(())
}

/// Busca el primer MFT codificador utilizable (hardware → software) y lo
/// pone en modo streaming (la sonda síncrona descarta los MFT asíncronos:
/// devuelven `MF_E_TRANSFORM_ASYNC_LOCKED`).
fn find_encoder_mft(
    w: u32,
    h: u32,
    fps: u32,
    bitrate: u32,
) -> Result<mf::IMFTransform, CodecError> {
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
        if let Err(e) = unsafe { configure_encoder(&t, w, h, fps, bitrate) } {
            last = format!("{e:?}");
            continue;
        }
        if let Err(e) = unsafe { t.ProcessMessage(mf::MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0) } {
            last = format!("BEGIN_STREAMING: {e}");
            continue;
        }
        let _ = unsafe { t.ProcessMessage(mf::MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0) };
        return Ok(t);
    }
    Err(CodecError::Encode(format!("sin MFT H.264 utilizable ({last})")))
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
            self.buffer.Unlock().map_err(|e| CodecError::Encode(format!("Unlock: {e}")))?;
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
            .map_err(|e| CodecError::Decode(format!("{e}")))?
            as usize;
        let mut out = vec![0u8; len];
        let mut ptr: *mut u8 = std::ptr::null_mut();
        buf.Lock(&mut ptr, None, None)
            .map_err(|e| CodecError::Decode(format!("Lock: {e}")))?;
        if !ptr.is_null() && len > 0 {
            std::ptr::copy_nonoverlapping(ptr, out.as_mut_ptr(), len);
        }
        buf.Unlock().map_err(|e| CodecError::Decode(format!("Unlock: {e}")))?;
        Ok(out)
    }
}

/// Codificador H.264 (MF). La creación del MFT es **perezosa** (en el primer
/// frame, con sus dimensiones reales): si MF no sirve, `encode` devuelve
/// error y `video.rs` cae a VP9 con evento `video_backend`.
pub struct MfH264Encoder {
    transform: Option<mf::IMFTransform>,
    fps: u32,
    bitrate: u32,
    width: u32,  // efectivos (siempre pares: NV12)
    height: u32,
    seq: u64,
    nv12: Vec<u8>,
    input: Option<SampleSlot>,
    output: Option<SampleSlot>,
    /// SPS+PPS (`MF_MT_MPEG_SEQUENCE_HEADER`): se anteponen a cada frame
    /// para que cualquier NAL sea decodificable por sí solo (resiliencia).
    seq_hdr: Vec<u8>,
}

// Los MFT son pointers COM sin `Send` (NonNull). Seguridad: ambos wrappers
// se crean y usan SIEMPRE en un único hilo (el hilo de vídeo / el bucle de
// sesión del controlador) y solo se mueven hacia él — patrón igual que el
// resto de backends de este crate.
unsafe impl Send for MfH264Encoder {}
unsafe impl Send for MfH264Decoder {}

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
        let t = find_encoder_mft(ew, eh, self.fps, self.bitrate)?;
        let nv12_len = (ew as usize) * (eh as usize) * 3 / 2;
        let input = make_slot(nv12_len as u32)?;
        let output = make_slot(nv12_len as u32 + 64 * 1024)?; // holgura por ruido
        self.transform = Some(t);
        self.width = ew;
        self.height = eh;
        self.nv12.resize(nv12_len, 0);
        self.input = Some(input);
        self.output = Some(output);
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
                            self.seq_hdr = buf;
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
        let (y, u, v) = bgra_to_i420(ew, eh, bgra);
        self.nv12 = i420_to_nv12(&y, &u, &v);
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
        drop(data);
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
            Err(e) => Err(CodecError::Encode(format!("ProcessOutput: {e}"))),
        }
    }
}

impl VideoEncoder for MfH264Encoder {
    fn encode(&mut self, frame: &Frame) -> Result<EncodedFrame, CodecError> {
        self.ensure(frame.width, frame.height)?;
        let t = self
            .transform
            .clone()
            .ok_or_else(|| CodecError::Encode("sin MFT".into()))?;
        // Si la entrada no está libre, drena antes (evita MF_E_NOTACCEPTING).
        let status = unsafe { t.GetInputStatus(0) }.unwrap_or(1);
        if status & 1 == 0 {
            let _ = self.drain(&t)?;
        }
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
        let mut payload = self.drain(&t)?.unwrap_or_default();
        self.load_seq_header(&t);
        if !payload.is_empty() && !self.seq_hdr.is_empty() && !payload.starts_with(&self.seq_hdr) {
            let mut with_hdr = self.seq_hdr.clone();
            with_hdr.extend_from_slice(&payload);
            payload = with_hdr;
        }
        let seq = self.seq;
        self.seq += 1;
        Ok(EncodedFrame {
            format: CodecFormat::H264,
            seq,
            width: self.width,
            height: self.height,
            data: payload,
        })
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
        if let Some(t) = self.transform.clone() {
            // Renegociación en caliente (solo la salida); si el MFT la
            // rechaza → reconstrucción perezosa en el próximo frame.
            let re = unsafe { retype_output(&t, self.width, self.height, self.fps, bps) };
            if re.is_err() {
                self.transform = None;
                self.seq_hdr.clear();
            }
        }
        true
    }

    fn set_fps(&mut self, fps: u32) {
        self.fps = fps.max(1);
    }
}

/// Decodificador H.264 (MF): NV12 de salida → BGRA (el visor).
pub struct MfH264Decoder {
    transform: Option<mf::IMFTransform>,
    width: u32,
    height: u32,
    input: Option<SampleSlot>,
    output: Option<SampleSlot>,
    /// SPS+PPS vistos en el stream: se inyectan en el TIPO DE ENTRADA
    /// (convención MF: el decoder los recibe por el atributo, no solo
    /// in-band — sin él se queda en NEED_MORE_INPUT para siempre).
    seq_hdr: Vec<u8>,
    /// BEGIN/START enviados DESPUÉS de la configuración final (cambiar
    /// tipos tras el aviso deja al MFT en NEED_MORE_INPUT perpetuo).
    stream_started: bool,
}

impl MfH264Decoder {
    pub fn new() -> Self {
        Self {
            transform: None,
            width: 0,
            height: 0,
            input: None,
            output: None,
            seq_hdr: Vec::new(),
            stream_started: false,
        }
    }

    /// Crea el decoder UNA vez. Salida: primero nuestro tipo (dims del wire);
    /// si lo rechaza, el primero **disponible sin filtro de dims** — los
    /// decoders anuncian plantillas (p. ej. 1920×1080) que rechazan dims
    /// ajenas hechas a mano (observado en desarrollo C-14). Las dims reales
    /// las fija el stream vía `MF_E_TRANSFORM_STREAM_CHANGE`.
    fn ensure(&mut self, w: u32, h: u32) -> Result<(), CodecError> {
        if self.transform.is_some() {
            return Ok(());
        }
        mf_ready()?;
        let in_t = type_info(mf::MFVideoFormat_H264);
        let out_t = type_info(mf::MFVideoFormat_NV12);
        let mut last = String::from("sin MFT enumerados");
        // Diag: decoders con y sin filtro de salida
        let all = enum_mfts(mf::MFT_CATEGORY_VIDEO_DECODER, Some(&in_t), None)?;
        eprintln!("decoders(sin filtro salida) = {}", all.len());
        for a in &all {
            let name = unsafe {
                let mut pw = windows::core::PWSTR(std::ptr::null_mut());
                let mut len = 0u32;
                if a.GetAllocatedString(&mf::MFT_FRIENDLY_NAME_Attribute, &mut pw, &mut len).is_ok()
                    && !pw.0.is_null()
                {
                    let s = String::from_utf16_lossy(std::slice::from_raw_parts(pw.0, len as usize));
                    windows::Win32::System::Com::CoTaskMemFree(Some(pw.0 as _));
                    s
                } else {
                    "<sin nombre>".into()
                }
            };
            eprintln!("   · {name}");
        }
        for act in enum_mfts(mf::MFT_CATEGORY_VIDEO_DECODER, Some(&in_t), Some(&out_t))? {
            let t = match unsafe { act.ActivateObject::<mf::IMFTransform>() } {
                Ok(t) => t,
                Err(e) => {
                    last = format!("ActivateObject: {e}");
                    continue;
                }
            };
            unsafe {
                let inmt = match video_type(mf::MFVideoFormat_H264) {
                    Ok(m) => m,
                    Err(e) => {
                        last = format!("{e:?}");
                        continue;
                    }
                };
                if t.SetInputType(0, &inmt, 0).is_err()
                    && !try_available_type(&t, true, mf::MFVideoFormat_H264, None)?
                {
                    last = "SetInputType de decoder rechazado".into();
                    continue;
                }
                let mut out_ok = false;
                if let Ok(outmt) = video_type(mf::MFVideoFormat_NV12) {
                    let _ = frame_size_attr(&outmt, w, h);
                    let _ = common_video_attrs(&outmt, w, h, 30);
                    let _ = nv12_raw_attrs(&outmt, w, h);
                    out_ok = t.SetOutputType(0, &outmt, 0).is_ok();
                }
                if !out_ok {
                    out_ok = try_available_type(&t, false, mf::MFVideoFormat_NV12, None)?;
                }
                if !out_ok {
                    last = "SetOutputType de decoder rechazado".into();
                    continue;
                }
                // BEGIN/START se envían en `decode` tras la configuración
                // final (cabecera inyectada) — ver `stream_started`.
            }
            let cb = unsafe { t.GetOutputStreamInfo(0) }
                .map(|i| i.cbSize)
                .unwrap_or(0);
            let raw = (w as usize) * (h as usize) * 3 / 2;
            let out_max = (cb as usize).max(raw) + 64 * 1024;
            let input = make_slot(raw as u32 + 64 * 1024)?;
            let output = make_slot(out_max as u32)?;
            self.transform = Some(t);
            self.width = w;
            self.height = h;
            self.input = Some(input);
            self.output = Some(output);
            return Ok(());
        }
        Err(CodecError::Decode(format!("sin MFT H.264 decoder ({last})")))
    }

    /// Reajusta el slot de entrada si el NAL supera la capacidad actual.
    fn ensure_input_capacity(&mut self, need: usize) -> Result<(), CodecError> {
        let max = self
            .input
            .as_ref()
            .map(|s| unsafe { s.buffer.GetMaxLength() }.unwrap_or(0) as usize)
            .unwrap_or(0);
        if need + 1024 <= max {
            return Ok(());
        }
        self.input = Some(make_slot((need + 65_536) as u32)?);
        Ok(())
    }
}

impl Default for MfH264Decoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Busca el próximo start code Annex B (`00 00 01` / `00 00 00 01`).
fn find_start_code(d: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while i + 3 < d.len() {
        if d[i] == 0 && d[i + 1] == 0 {
            if d[i + 2] == 1 {
                return Some((i, 3));
            }
            if d[i + 2] == 0 && i + 4 <= d.len() && d[i + 3] == 1 {
                return Some((i, 4));
            }
        }
        i += 1;
    }
    None
}

/// Extrae SPS+PPS (NAL 7 y 8) de un stream Annex B — desde el primer start
/// code hasta el primer NAL que no sea de parámetros.
fn extract_seq_header(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some((sc, len)) = find_start_code(data, pos) {
        let nal0 = sc + len;
        if nal0 >= data.len() {
            break;
        }
        let t = data[nal0] & 0x1F;
        let end = find_start_code(data, nal0)
            .map(|(p, _)| p)
            .unwrap_or(data.len());
        if t == 7 || t == 8 {
            out.extend_from_slice(&data[sc..end]);
            pos = end;
        } else {
            break;
        }
    }
    out
}

/// Dims reales del tipo de salida actual (el MFT las actualiza con
/// `MF_E_TRANSFORM_STREAM_CHANGE` cuando el stream difiere de la plantilla).
fn current_out_dims(t: &mf::IMFTransform) -> Option<(u32, u32)> {
    unsafe {
        let mt = t.GetOutputCurrentType(0).ok()?;
        let v = mt.GetUINT64(&mf::MF_MT_FRAME_SIZE).ok()?;
        Some(((v >> 32) as u32, (v & 0xFFFF_FFFF) as u32))
    }
}

impl VideoDecoder for MfH264Decoder {
    fn decode(&mut self, frame: &EncodedFrame) -> Result<Frame, CodecError> {
        if frame.data.is_empty() {
            return Err(CodecError::Decode("frame H.264 vacío".into()));
        }
        self.ensure(frame.width, frame.height)?;
        self.ensure_input_capacity(frame.data.len())?;
        let t = self
            .transform
            .clone()
            .ok_or_else(|| CodecError::Decode("sin MFT decoder".into()))?;

        // Convención MF: el SPS/PPS del stream viaja en el TIPO DE ENTRADA.
        let hdr = extract_seq_header(&frame.data);
        eprintln!("DEC hdr={}B (frame {}B) seq_actual={}B", hdr.len(), frame.data.len(), self.seq_hdr.len());
        if !hdr.is_empty() && self.seq_hdr != hdr {
            self.seq_hdr = hdr;
            let t2 = t.clone();
            unsafe {
                let inmt = video_type(mf::MFVideoFormat_H264)?;
                inmt.SetBlob(&mf::MF_MT_MPEG_SEQUENCE_HEADER, &self.seq_hdr)
                    .map_err(|e| CodecError::Decode(format!("SetBlob(seq): {e}")))?;
                // dims EN LA ENTRADA: sin ellas el MFT toma su plantilla
                // (1920×1080) y la salida con las dims del stream se rechaza
                // por inconsistencia (raíz observada en C-14).
                let _ = frame_size_attr(&inmt, frame.width, frame.height);
                let r_in = t2.SetInputType(0, &inmt, 0);
                eprintln!("DEC SetInput(blob) → {r_in:?}");
                if r_in.is_ok() {
                    // Regla MF: cambiar la entrada INVALIDA la salida →
                    // reasentar (sino, ProcessOutput devuelve INVALIDMEDIATYPE).
                    if let Ok(outmt) = video_type(mf::MFVideoFormat_NV12) {
                        let _ = frame_size_attr(&outmt, frame.width, frame.height);
                        let _ = common_video_attrs(&outmt, frame.width, frame.height, 30);
                        let _ = nv12_raw_attrs(&outmt, frame.width, frame.height);
                        let _ = t2.SetOutputType(0, &outmt, 0);
                    }
                    let r_out = try_available_type(&t2, false, mf::MFVideoFormat_NV12, None);
                    eprintln!("DEC re-set salida → {r_out:?}");
                } else {
                    let r_alt = try_available_type(&t2, true, mf::MFVideoFormat_H264, None);
                    eprintln!("DEC entrada rechazada, plan B → {r_alt:?}");
                }
            }
        }

        if !self.stream_started {
            // Avisos de streaming DESPUÉS de la configuración final (con la
            // cabecera ya en el tipo de entrada): si los tipos cambian después
            // del aviso, el MFT se queda en NEED_INPUT para siempre.
            unsafe {
                let _ = t.ProcessMessage(mf::MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
                let _ = t.ProcessMessage(mf::MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
                if let Ok(mt) = t.GetInputCurrentType(0) {
                    let sz = mt.GetBlobSize(&mf::MF_MT_MPEG_SEQUENCE_HEADER).unwrap_or(0);
                    let fs = mt.GetUINT64(&mf::MF_MT_FRAME_SIZE).unwrap_or(0);
                    eprintln!("DEC in-current: seqblob={sz}B size={fs:#x}");
                } else {
                    eprintln!("DEC in-current: ERR");
                }
                if let Ok(mt) = t.GetOutputCurrentType(0) {
                    let fs = mt.GetUINT64(&mf::MF_MT_FRAME_SIZE).unwrap_or(0);
                    let sub = mt.GetGUID(&mf::MF_MT_SUBTYPE).unwrap_or_default();
                    eprintln!("DEC out-current: size={fs:#x} sub={sub:?}");
                }
                let mut isi: mf::MFT_INPUT_STREAM_INFO = std::mem::zeroed();
                if t.GetInputStreamInfo(0, &mut isi).is_ok() {
                    eprintln!("DEC inInfo: flags={:#x} cbAlignment={} cbSize={}", isi.dwFlags, isi.cbAlignment, isi.cbSize);
                }
            }
            self.stream_started = true;
        }

        let time = (frame.seq as i64) * 10_000_000 / 15;
        let input = self
            .input
            .as_ref()
            .ok_or_else(|| CodecError::Decode("sin muestra de entrada".into()))?;
        input.fill(&frame.data, time, 10_000_000 / 15)?;

        // Si quedó salida sin drenar, drenar antes de alimentar (evita
        // MF_E_NOTACCEPTING por estados previos).
        let status = unsafe { t.GetInputStatus(0) }.unwrap_or(1);
        if status & 1 == 0 {
            for _ in 0..4 {
                let out_sample = self
                    .output
                    .as_ref()
                    .map(|s| s.sample.clone())
                    .ok_or_else(|| CodecError::Decode("sin muestra de salida".into()))?;
                let mut data = mf::MFT_OUTPUT_DATA_BUFFER {
                    dwStreamID: 0,
                    pSample: core::mem::ManuallyDrop::new(Some(out_sample)),
                    dwStatus: 0,
                    pEvents: core::mem::ManuallyDrop::new(None),
                };
                let mut st = 0u32;
                let res = unsafe { t.ProcessOutput(0, std::slice::from_mut(&mut data), &mut st) };
                let _p: Option<mf::IMFSample> =
                    unsafe { core::mem::ManuallyDrop::take(&mut data.pSample) };
                let _e = unsafe { core::mem::ManuallyDrop::take(&mut data.pEvents) };
                drop(data);
                if res.is_err() {
                    break;
                }
            }
        }
        unsafe {
            t.ProcessInput(0, &input.sample, 0)
                .map_err(|e| CodecError::Decode(format!("ProcessOutput/ProcessInput: {e}")))?;
        }

        // Hasta 8 salidas: STREAM_CHANGE reasienta el tipo con dims reales.
        for _ in 0..8 {
            let out_sample = self
                .output
                .as_ref()
                .map(|s| s.sample.clone())
                .ok_or_else(|| CodecError::Decode("sin muestra de salida".into()))?;
            let mut data = mf::MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: core::mem::ManuallyDrop::new(Some(out_sample)),
                dwStatus: 0,
                pEvents: core::mem::ManuallyDrop::new(None),
            };
            let mut st = 0u32;
            let res = unsafe { t.ProcessOutput(0, std::slice::from_mut(&mut data), &mut st) };
            let produced: Option<mf::IMFSample> =
                unsafe { core::mem::ManuallyDrop::take(&mut data.pSample) };
            let _ev = unsafe { core::mem::ManuallyDrop::take(&mut data.pEvents) };
            drop(data);
            match res {
                Ok(()) => {
                    // C-14: leer SIEMPRE que Ok (este MFT no marca el bit de
                    // SAMPLE_READY — igual que el encoder).
                    let s = produced
                        .ok_or_else(|| CodecError::Decode("MFT sin muestra".into()))?;
                    let nv12 = read_sample(&s)?;
                    if nv12.is_empty() {
                        return Err(CodecError::Decode("muestra de salida vacía".into()));
                    }
                    // Dims: manda el stream (lo del wire); si difiere, las del
                    // tipo actual; si no, incoherente.
                    let wire = (frame.width, frame.height);
                    let cur = current_out_dims(&t).unwrap_or(wire);
                    let (w, h) = if nv12.len() == (wire.0 as usize * wire.1 as usize * 3 / 2) {
                        wire
                    } else if nv12.len() == (cur.0 as usize * cur.1 as usize * 3 / 2) {
                        cur
                    } else {
                        return Err(CodecError::Decode(format!(
                            "NV12 de {} B no cuadra con {}x{} ni {}x{}",
                            nv12.len(),
                            wire.0,
                            wire.1,
                            cur.0,
                            cur.1
                        )));
                    };
                    let (y, u, v) = nv12_to_i420(w, h, &nv12);
                    let data = i420_to_bgra(
                        w as usize,
                        h as usize,
                        &y,
                        &u,
                        &v,
                        w as usize,
                        (w / 2) as usize,
                    );
                    self.width = w;
                    self.height = h;
                    return Ok(Frame {
                        width: w,
                        height: h,
                        format: PixelFormat::Bgra8,
                        data,
                    });
                }
                Err(e) if e.code() == mf::MF_E_TRANSFORM_STREAM_CHANGE => {
                    // El MFT pide reasentar la salida con las dims reales.
                    if let Ok(mt) = unsafe { t.GetOutputCurrentType(0) } {
                        let _ = unsafe { t.SetOutputType(0, &mt, 0) };
                    }
                    let _ = unsafe {
                        try_available_type(&t, false, mf::MFVideoFormat_NV12, None)?
                    };
                }
                Err(e) if e.code() == mf::MF_E_TRANSFORM_NEED_MORE_INPUT => {
                    return Err(CodecError::Decode(
                        "bitstream incompleto (NEED_MORE_INPUT)".into(),
                    ));
                }
                Err(e) => return Err(CodecError::Decode(format!("ProcessOutput: {e}"))),
            }
        }
        Err(CodecError::Decode("sin frame tras reintentos".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            if nals <= 3 {
                let hb: String = enc.seq_hdr.iter().take(8).map(|b| format!("{b:02x}")).collect();
                let fb: String = f.data.iter().take(8).map(|b| format!("{b:02x}")).collect();
                eprintln!("BLOB_hdr[0..8]={hb} frame[0..8]={fb} igual_prefijo={}",
                    f.data.starts_with(&enc.seq_hdr));
                let head: String = f.data.iter().take(24).map(|b| format!("{b:02x}")).collect();
                let sc = f.data.windows(4).position(|w| w == [0, 0, 0, 1]);
                let nal_t = sc.map(|i| f.data.get(i + 4).map(|b| b & 0x1F));
                eprintln!("NAL#{nals} len={} start={:?} tipo={:?} head={head}",
                    f.data.len(), sc, nal_t);
            }
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

    /// Extracción de SPS+PPS de un stream Annex B sintético.
    #[test]
    fn extract_seq_header_takes_sps_and_pps_only() {
        let annexb = [
            0, 0, 0, 1, 0x67, 0x42, 0x00, 0x1e, // SPS
            0, 0, 0, 1, 0x68, 0xce, 0x06, // PPS
            0, 0, 0, 1, 0x65, 0x88, 0x84, 0x00, 0x33, // slice IDR
        ];
        let hdr = extract_seq_header(&annexb);
        assert_eq!(hdr.len(), 15, "SPS+PPS = hasta el slice: {hdr:?}");
        assert_eq!(&hdr[4..5], &[0x67]);
        assert!(extract_seq_header(&annexb[12..]).is_empty());
        // sin cabecera → vacío
        assert!(extract_seq_header(&[0, 0, 0, 1, 0x65, 1, 2]).is_empty());
    }

    /// Variantes de alimentación del decoder: ¿qué patrón produce salida?
    #[test]
    fn debug_feed_variants() {
        if mf_ready().is_err() {
            return;
        }
        let frame = TestPatternCapturer::render(320, 180, 7);
        // recopilar NALs del encoder
        let mut enc = MfH264Encoder::new(15, 600_000);
        let mut nals: Vec<EncodedFrame> = Vec::new();
        for _ in 0..30 {
            let f = enc.encode(&frame).unwrap();
            if !f.data.is_empty() {
                nals.push(f);
            }
            if nals.len() >= 8 {
                break;
            }
        }
        assert!(!nals.is_empty(), "encoder produce NALs");

        for variant in ["A actual", "B hdr-solo-primero", "C sin-blob"] {
            let mut dec = MfH264Decoder::new();
            let mut got = 0;
            let mut err1 = String::new();
            for (i, f) in nals.iter().enumerate() {
                let mut f2 = f.clone();
                if variant == "B hdr-solo-primero" && i == 0 {
                    // primer input = SOLO la cabecera
                    let hdr = extract_seq_header(&f2.data);
                    let mut only_hdr = f2.clone();
                    only_hdr.data = hdr;
                    let r = dec.decode(&only_hdr);
                    if r.is_err() && err1.is_empty() {
                        err1 = format!("{:?}", r.unwrap_err());
                    }
                    // y luego el resto (sin cabecera)
                    let rest = f2.data.clone();
                    let hdr2 = extract_seq_header(&rest);
                    if rest.starts_with(&hdr2) && hdr2.len() <= rest.len() {
                        f2.data = rest[hdr2.len()..].to_vec();
                    }
                }
                if variant == "C sin-blob" {
                    // quitar la cabecera inyectada del tipo (decoder fresco no
                    // la tiene: solo quitar el prefijo del sample)
                    let hdr = extract_seq_header(&f2.data);
                    if !hdr.is_empty() && f2.data.starts_with(&hdr) {
                        f2.data = f2.data[hdr.len()..].to_vec();
                    }
                }
                match dec.decode(&f2) {
                    Ok(fr) => {
                        got += 1;
                        eprintln!("[{variant}] frame {i} → DECODIFICADO {}x{}", fr.width, fr.height);
                    }
                    Err(e) => {
                        if err1.is_empty() {
                            err1 = format!("{e:?}");
                        }
                    }
                }
                if got > 0 {
                    break;
                }
            }
            eprintln!("[{variant}] got={got} err={err1}");
        }
    }

    /// Lote: ¿qué FORMATO de cabecera acepta el decoder?
    #[test]
    fn debug_decoder_batch() {
        if mf_ready().is_err() {
            return;
        }
        let frame = TestPatternCapturer::render(320, 180, 7);
        let mut enc = MfH264Encoder::new(15, 600_000);
        let mut nals: Vec<EncodedFrame> = Vec::new();
        for _ in 0..30 {
            let f = enc.encode(&frame).unwrap();
            if !f.data.is_empty() {
                nals.push(f);
            }
            if nals.len() >= 6 {
                break;
            }
        }
        let annexb = extract_seq_header(&nals[0].data);
        let enc_hdr = enc.seq_hdr.clone();
        // AVCC: unidades NAL con prefijo de longitud de4 bytes (sin start codes)
        let mut avcc = Vec::new();
        let mut pos = 0;
        while let Some((sc, len)) = find_start_code(&annexb, pos) {
            let nal0 = sc + len;
            let end = find_start_code(&annexb, nal0)
                .map(|(p, _)| p)
                .unwrap_or(annexb.len());
            let nal = &annexb[nal0..end];
            avcc.extend_from_slice(&(nal.len() as u32).to_be_bytes());
            avcc.extend_from_slice(nal);
            pos = end;
        }
        eprintln!("hdrs: annexb={}B enc={}B avcc={}B", annexb.len(), enc_hdr.len(), avcc.len());

        for (name, blob) in [("annexb", annexb.clone()), ("enc", enc_hdr.clone()), ("avcc", avcc.clone())] {
            let mut dec = MfH264Decoder::new();
            if let Err(e) = dec.ensure(320, 180) {
                eprintln!("[{name}] ensure err {e:?}");
                continue;
            }
            let t = dec.transform.clone().unwrap();
            unsafe {
                // ENTRADA primero (el decoder la exige) + atributos nuevos:
                // COMPRESSED + perfil (probados en esta variante).
                let inmt = video_type(mf::MFVideoFormat_H264).unwrap();
                if !blob.is_empty() {
                    let _ = inmt.SetBlob(&mf::MF_MT_MPEG_SEQUENCE_HEADER, &blob);
                }
                let _ = frame_size_attr(&inmt, 320, 180);
                let _ = inmt.SetUINT32(&mf::MF_MT_COMPRESSED, 1);
                let _ = inmt.SetUINT32(&mf::MF_MT_MPEG2_PROFILE, mf::eAVEncH264VProfile_Base.0 as u32);
                let ri = t.SetInputType(0, &inmt, 0);
                if ri.is_err() {
                    eprintln!("[{name}] SetInput err {ri:?}");
                    continue;
                }
                let outmt = video_type(mf::MFVideoFormat_NV12).unwrap();
                let _ = frame_size_attr(&outmt, 320, 180);
                let _ = common_video_attrs(&outmt, 320, 180, 30);
                let _ = nv12_raw_attrs(&outmt, 320, 180);
                let ro = t.SetOutputType(0, &outmt, 0);
                if ro.is_err() {
                    eprintln!("[{name}] SetOutput err {ro:?}");
                    continue;
                }
                // Variante: SIN notificaciones de streaming.
                if name == "enc" {
                    let _ = t.ProcessMessage(mf::MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
                    let _ = t.ProcessMessage(mf::MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
                }
            }
            dec.ensure_input_capacity(1 << 20).unwrap();
            let mut got = 0;
            for (i, f) in nals.iter().enumerate() {
                let input = dec.input.as_ref().unwrap();
                if input.fill(&f.data, (i as i64) * 666_666, 666_666).is_err() {
                    continue;
                }
                let pi = unsafe { t.ProcessInput(0, &input.sample, 0) };
                if pi.is_err() {
                    eprintln!("[{name}] PI[{i}] err {pi:?}");
                    continue;
                }
                let out_sample = dec.output.as_ref().unwrap().sample.clone();
                let mut data = mf::MFT_OUTPUT_DATA_BUFFER {
                    dwStreamID: 0,
                    pSample: core::mem::ManuallyDrop::new(Some(out_sample)),
                    dwStatus: 0,
                    pEvents: core::mem::ManuallyDrop::new(None),
                };
                let mut st = 0u32;
                let res = unsafe { t.ProcessOutput(0, std::slice::from_mut(&mut data), &mut st) };
                let produced: Option<mf::IMFSample> =
                    unsafe { core::mem::ManuallyDrop::take(&mut data.pSample) };
                let _e = unsafe { core::mem::ManuallyDrop::take(&mut data.pEvents) };
                drop(data);
                match res {
                    Ok(()) => {
                        let n = produced
                            .map(|s| read_sample(&s).map(|v| v.len()))
                            .unwrap_or(Ok(0));
                        if matches!(n, Ok(l) if l > 0) {
                            got += 1;
                            eprintln!("[{name}] PO[{i}] → {n:?} ¡DECODE!");
                        }
                    }
                    Err(e) if e.code() == mf::MF_E_TRANSFORM_NEED_MORE_INPUT => {
                        // QUIRK C-14: ¿escribe la muestra A PESAR del error?
                        let n = produced
                            .map(|s| read_sample(&s).map(|v| v.len()))
                            .unwrap_or(Ok(0));
                        if matches!(n, Ok(l) if l > 0) {
                            got += 1;
                            eprintln!("[{name}] PO[{i}] NEED_MORE pero muestra → {n:?} ¡DECODE!");
                        }
                    }
                    Err(e) => {
                        eprintln!("[{name}] PO[{i}] err {e:?}");
                        break;
                    }
                }
            }
            eprintln!("[{name}] got={got}");
        }
    }

    /// Validación externa: ¿el SourceReader de MS (pipeline oficial)
    /// decodifica nuestro stream? Si falla, el problema es del ENCODER.
    #[test]
    fn debug_sourcereader_validates_stream() {
        if mf_ready().is_err() {
            return;
        }
        let frame = TestPatternCapturer::render(320, 180, 7);
        let mut enc = MfH264Encoder::new(15, 600_000);
        let mut nals: Vec<EncodedFrame> = Vec::new();
        for _ in 0..30 {
            let f = enc.encode(&frame).unwrap();
            if !f.data.is_empty() {
                nals.push(f);
            }
            if nals.len() >= 8 {
                break;
            }
        }
        let mut all = Vec::new();
        for f in &nals {
            all.extend_from_slice(&f.data);
        }
        let path = std::env::temp_dir().join(format!("anyremote-sr-{}.264", std::process::id()));
        std::fs::write(&path, &all).expect("escribe .h264");

        unsafe {
            let mut attrs_opt: Option<mf::IMFAttributes> = None;
            mf::MFCreateAttributes(&mut attrs_opt, 1).expect("attrs");
            let attrs = attrs_opt.expect("attrs");
            let pth = windows::core::HSTRING::from(path.to_string_lossy().to_string());
            let stream = match mf::MFCreateFile(
                mf::MF_ACCESSMODE_READ,
                mf::MF_OPENMODE_FAIL_IF_NOT_EXIST,
                mf::MF_FILEFLAGS_NONE,
                &pth,
            ) {
                Ok(bs) => bs,
                Err(e) => {
                    let _ = std::fs::remove_file(&path);
                    panic!("MFCreateFile: {e}");
                }
            };
            let reader = match mf::MFCreateSourceReaderFromByteStream(&stream, &attrs) {
                Ok(r) => r,
                Err(e) => {
                    let _ = std::fs::remove_file(&path);
                    panic!("SourceReader: {e}");
                }
            };
            let _ = reader.SetStreamSelection(0, true);
            // forzar salida NV12 → fuerza el decoder
            let outmt = video_type(mf::MFVideoFormat_NV12).unwrap();
            let _ = frame_size_attr(&outmt, 320, 180);
            let r_set = reader.SetCurrentMediaType(0, None, &outmt);
            eprintln!("SR SetCurrentMediaType → {r_set:?}");
            let mut decoded = 0usize;
            let mut compressed = 0usize;
            for _ in 0..60 {
                let mut actual = 0u32;
                let mut flags = 0u32;
                let mut ts = 0i64;
                let mut sample: Option<mf::IMFSample> = None;
                let r = reader.ReadSample(
                    0,
                    0,
                    Some(&mut actual),
                    Some(&mut flags),
                    Some(&mut ts),
                    Some(&mut sample),
                );
                match r {
                    Ok(()) => {
                        if flags & mf::MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                            break;
                        }
                        if let Some(sp) = sample {
                            let data = read_sample(&sp).unwrap_or_default();
                            if data.len() == 320 * 180 * 3 / 2 {
                                decoded += 1;
                            } else if !data.is_empty() {
                                compressed += 1;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("SR ReadSample err {e:?}");
                        break;
                    }
                }
            }
            eprintln!("SR decoded_nv12={decoded} otros={compressed}");
            let _ = std::fs::remove_file(&path);
            assert!(
                decoded > 0,
                "el SourceReader de MS decodifica nuestro stream (si falla, el encoder produce basura)"
            );
        }
    }

    /// Teoría FIXED_SAMPLE_SIZE: rellenar cada sample a cbSize exacto (4096).
    #[test]
    fn debug_fixed_size_feed() {
        if mf_ready().is_err() {
            return;
        }
        // bitrate bajo para que el key quepa en4096
        let frame = TestPatternCapturer::render(160, 90, 7);
        let mut enc = MfH264Encoder::new(15, 150_000);
        let mut nals: Vec<EncodedFrame> = Vec::new();
        for _ in 0..30 {
            let f = enc.encode(&frame).unwrap();
            if !f.data.is_empty() {
                nals.push(f);
            }
            if nals.len() >= 6 {
                break;
            }
        }
        let max_len = nals.iter().map(|f| f.data.len()).max().unwrap_or(0);
        eprintln!("tamaños: {:?} (max {max_len})", nals.iter().map(|f| f.data.len()).collect::<Vec<_>>());
        if max_len > 4096 {
            eprintln!("keyframe > 4096: la teoría FIXED es inviable para producción");
            return;
        }
        let mut dec = MfH264Decoder::new();
        dec.ensure(160, 90).unwrap();
        let t = dec.transform.clone().unwrap();
        let hdr = extract_seq_header(&nals[0].data);
        unsafe {
            let inmt = video_type(mf::MFVideoFormat_H264).unwrap();
            let _ = inmt.SetBlob(&mf::MF_MT_MPEG_SEQUENCE_HEADER, &hdr);
            let _ = frame_size_attr(&inmt, 160, 90);
            t.SetInputType(0, &inmt, 0).unwrap();
            let outmt = video_type(mf::MFVideoFormat_NV12).unwrap();
            let _ = frame_size_attr(&outmt, 160, 90);
            let _ = common_video_attrs(&outmt, 320, 180, 30);
            let _ = nv12_raw_attrs(&outmt, 320, 180);
            t.SetOutputType(0, &outmt, 0).unwrap();
            let _ = t.ProcessMessage(mf::MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
            let _ = t.ProcessMessage(mf::MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
        }
        // slot exacto de4096
        dec.input = Some(make_slot(4096).unwrap());
        let mut got = 0;
        for (i, f) in nals.iter().enumerate() {
            let mut padded = f.data.clone();
            padded.resize(4096, 0); // relleno a tamaño fijo
            let input = dec.input.as_ref().unwrap();
            input.fill(&padded, (i as i64) * 666_666, 666_666).unwrap();
            let pi = unsafe { t.ProcessInput(0, &input.sample, 0) };
            if pi.is_err() {
                eprintln!("PI[{i}] err {pi:?}");
                continue;
            }
            let out_sample = dec.output.as_ref().unwrap().sample.clone();
            let mut data = mf::MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: core::mem::ManuallyDrop::new(Some(out_sample)),
                dwStatus: 0,
                pEvents: core::mem::ManuallyDrop::new(None),
            };
            let mut st = 0u32;
            let res = unsafe { t.ProcessOutput(0, std::slice::from_mut(&mut data), &mut st) };
            let produced: Option<mf::IMFSample> =
                unsafe { core::mem::ManuallyDrop::take(&mut data.pSample) };
            let _e = unsafe { core::mem::ManuallyDrop::take(&mut data.pEvents) };
            drop(data);
            match res {
                Ok(()) => {
                    let n = produced.map(|s| read_sample(&s).map(|v| v.len())).unwrap_or(Ok(0));
                    eprintln!("PO[{i}] → {n:?}");
                    if matches!(n, Ok(l) if l > 0) {
                        got += 1;
                    }
                }
                Err(e) => eprintln!("PO[{i}] err {e:?}"),
            }
        }
        eprintln!("FIXED got={got}");
    }

    /// Última variante: salida I420 (también la anuncia el decoder).
    #[test]
    fn debug_i420_output() {
        if mf_ready().is_err() {
            return;
        }
        let frame = TestPatternCapturer::render(320, 180, 7);
        let mut enc = MfH264Encoder::new(15, 600_000);
        let mut nals: Vec<EncodedFrame> = Vec::new();
        for _ in 0..30 {
            let f = enc.encode(&frame).unwrap();
            if !f.data.is_empty() {
                nals.push(f);
            }
            if nals.len() >= 6 {
                break;
            }
        }
        let mut dec = MfH264Decoder::new();
        dec.ensure(320, 180).unwrap();
        let t = dec.transform.clone().unwrap();
        let hdr = extract_seq_header(&nals[0].data);
        unsafe {
            let inmt = video_type(mf::MFVideoFormat_H264).unwrap();
            let _ = inmt.SetBlob(&mf::MF_MT_MPEG_SEQUENCE_HEADER, &hdr);
            let _ = frame_size_attr(&inmt, 320, 180);
            t.SetInputType(0, &inmt, 0).unwrap();
            let outmt = video_type(mf::MFVideoFormat_I420).unwrap();
            let _ = frame_size_attr(&outmt, 320, 180);
            let _ = common_video_attrs(&outmt, 320, 180, 30);
            let ro = t.SetOutputType(0, &outmt, 0);
            eprintln!("I420 SetOutput → {ro:?}");
            let _ = t.ProcessMessage(mf::MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
            let _ = t.ProcessMessage(mf::MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
        }
        dec.input = Some(make_slot(1 << 20).unwrap());
        let mut got = 0;
        for (i, f) in nals.iter().enumerate() {
            let input = dec.input.as_ref().unwrap();
            input.fill(&f.data, (i as i64) * 666_666, 666_666).unwrap();
            let pi = unsafe { t.ProcessInput(0, &input.sample, 0) };
            if pi.is_err() {
                eprintln!("PI[{i}] err {pi:?}");
                continue;
            }
            let out_sample = dec.output.as_ref().unwrap().sample.clone();
            let mut data = mf::MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: core::mem::ManuallyDrop::new(Some(out_sample)),
                dwStatus: 0,
                pEvents: core::mem::ManuallyDrop::new(None),
            };
            let mut st = 0u32;
            let res = unsafe { t.ProcessOutput(0, std::slice::from_mut(&mut data), &mut st) };
            let produced: Option<mf::IMFSample> =
                unsafe { core::mem::ManuallyDrop::take(&mut data.pSample) };
            let _e = unsafe { core::mem::ManuallyDrop::take(&mut data.pEvents) };
            drop(data);
            match res {
                Ok(()) => {
                    let n = produced.map(|s| read_sample(&s).map(|v| v.len())).unwrap_or(Ok(0));
                    eprintln!("PO[{i}] → {n:?}");
                    if matches!(n, Ok(l) if l > 0) {
                        got += 1;
                    }
                }
                Err(e) => eprintln!("PO[{i}] err {e:?}"),
            }
        }
        eprintln!("I420 got={got}");
        assert!(got > 0, "I420 produce salida");
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
