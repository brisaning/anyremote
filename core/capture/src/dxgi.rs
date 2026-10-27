//! Backend DXGI Desktop Duplication (Windows 8+). Patrón H-RD-05 (`scrap/src/dxgi`).
//! Extrae el escritorio como BGRA8 mediante staging texture con acceso CPU.
//! Clean-room D-01.

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, D3D11_CREATE_DEVICE_FLAG, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_NOT_FOUND, DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter, IDXGIAdapter1,
    IDXGIOutput, IDXGIOutput1, IDXGIResource, IDXGIOutputDuplication, IDXGIFactory1,
};
use windows::core::{HRESULT, Interface};

use crate::{CaptureError, Capturer, Frame, PixelFormat};

/// HRESULT de "timeout al esperar frame" (DXGI_ERROR_TIMEOUT; const ausente en windows 0.61).
const DXGI_ERROR_TIMEOUT: HRESULT = HRESULT(0x887A_0027u32 as _);

/// Duración máxima de espera por un frame nuevo (ms).
const ACQUIRE_TIMEOUT_MS: u32 = 500;

pub struct DxgiCapturer {
    duplication: IDXGIOutputDuplication,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    staging: ID3D11Texture2D,
    width: u32,
    height: u32,
    /// QW-1: se procesa SIEMPRE el primer frame (la vista necesita la imagen
    /// inicial); a partir de ahi se descartan los frames sin cambios.
    first_frame: bool,
}

fn first_active_output() -> Result<IDXGIOutput, CaptureError> {
    unsafe {
        let factory: IDXGIFactory1 =
            CreateDXGIFactory1().map_err(|e| CaptureError::Unavailable(format!("DXGI: {e}")))?;
        for i in 0.. {
            let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(i) {
                Ok(a) => a,
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(e) => return Err(CaptureError::Io(format!("EnumAdapters1: {e}"))),
            };
            for j in 0.. {
                let output: IDXGIOutput = match adapter.EnumOutputs(j) {
                    Ok(o) => o,
                    Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                    Err(e) => return Err(CaptureError::Io(format!("EnumOutputs: {e}"))),
                };
                let desc = output
                    .GetDesc()
                    .map_err(|e| CaptureError::Io(format!("GetDesc: {e}")))?;
                if desc.AttachedToDesktop.as_bool() {
                    return Ok(output);
                }
            }
        }
    }
    Err(CaptureError::Unavailable(
        "sin outputs de escritorio adjuntos".into(),
    ))
}

fn create_device(
    adapter: &IDXGIAdapter1,
) -> Result<(ID3D11Device, ID3D11DeviceContext), CaptureError> {
    let base: IDXGIAdapter = adapter
        .cast()
        .map_err(|e| CaptureError::Unavailable(format!("cast adapter: {e}")))?;
    let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    let mut level = D3D_FEATURE_LEVEL_11_0;
    unsafe {
        D3D11CreateDevice(
            Some(&base),
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            Some(&levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut level),
            Some(&mut context),
        )
        .or_else(|_| {
            // Fallback WARP (software) cuando el adaptador de hardware no coopera.
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                Some(&mut level),
                Some(&mut context),
            )
        })
        .map_err(|e| CaptureError::Unavailable(format!("D3D11CreateDevice: {e}")))?;
    }
    Ok((device.unwrap(), context.unwrap()))
}

impl DxgiCapturer {
    pub fn new() -> Result<Self, CaptureError> {
        let output = first_active_output()?;
        let out1: IDXGIOutput1 = output
            .cast()
            .map_err(|e| CaptureError::Unavailable(format!("IDXGIOutput1: {e}")))?;

        let adapter: IDXGIAdapter1 = unsafe { output.GetParent() }
            .map_err(|e| CaptureError::Io(format!("GetParent adapter: {e}")))?;
        let (device, context) = create_device(&adapter)?;

        let duplication: IDXGIOutputDuplication = unsafe { out1.DuplicateOutput(&device) }
            .map_err(|e| CaptureError::Unavailable(format!("DuplicateOutput: {e}")))?;

        let rect = unsafe { output.GetDesc() }
            .map_err(|e| CaptureError::Io(e.to_string()))?
            .DesktopCoordinates;
        let width = (rect.right - rect.left) as u32;
        let height = (rect.bottom - rect.top) as u32;

        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut tex: Option<ID3D11Texture2D> = None;
        unsafe {
            device
                .CreateTexture2D(&desc, None, Some(&mut tex))
                .map_err(|e| CaptureError::Io(format!("staging texture: {e}")))?;
        }
        let staging = tex.ok_or_else(|| CaptureError::Io("sin staging texture".into()))?;

        Ok(Self {
            duplication,
            device,
            context,
            staging,
            width,
            height,
            first_frame: false,
        })
    }
}

impl Capturer for DxgiCapturer {
    fn next_frame(&mut self) -> Result<Frame, CaptureError> {
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        unsafe {
            match self
                .duplication
                .AcquireNextFrame(ACQUIRE_TIMEOUT_MS, &mut info, &mut resource)
            {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_TIMEOUT => return Err(CaptureError::Timeout),
                Err(e) => return Err(CaptureError::Io(format!("AcquireNextFrame: {e}"))),
            }

            // QW-1 (perf): si el escritorio no cambio desde el ultimo frame
            // (p. ej. solo se movio el raton: AccumulatedFrames == 0) no hay
            // nada que copiar, escalar ni codificar -> se descarta. Sin esto,
            // cada movimiento de raton sobre una pantalla estatica generaba un
            // frame completo de VP9 y de red (patron RustDesk dxgi/mod.rs).
            if self.first_frame && frame_unchanged(info.AccumulatedFrames, info.LastPresentTime) {
                let _ = self.duplication.ReleaseFrame();
                return Err(CaptureError::Timeout);
            }
            self.first_frame = true;

            let result = (|| {
                let tex: ID3D11Texture2D = resource
                    .as_ref()
                    .ok_or_else(|| CaptureError::Io("sin textura".into()))?
                    .cast()
                    .map_err(|e| CaptureError::Io(e.to_string()))?;
                self.context.CopyResource(&self.staging, &tex);

                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                self.context
                    .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                    .map_err(|e| CaptureError::Io(format!("Map: {e}")))?;
                let src = mapped.pData as *const u8;
                let row = mapped.RowPitch as usize;
                let tight = self.width as usize * 4;
                let mut data = vec![0u8; tight * self.height as usize];
                for y in 0..self.height as usize {
                    std::ptr::copy_nonoverlapping(
                        src.add(y * row),
                        data.as_mut_ptr().add(y * tight),
                        tight,
                    );
                }
                self.context.Unmap(&self.staging, 0);
                Ok(Frame {
                    width: self.width,
                    height: self.height,
                    format: PixelFormat::Bgra8,
                    data,
                })
            })();

            let _ = self.duplication.ReleaseFrame();
            let _: &ID3D11Device = &self.device;
            result
        }
    }

    fn description(&self) -> String {
        format!("dxgi-desktop-duplication {}x{}", self.width, self.height)
    }
}


/// QW-1: el frame adquirido NO aporta cambios de escritorio (p. ej. solo se
/// movio el raton: `AccumulatedFrames == 0`) o no hubo presentacion nueva
/// (`LastPresentTime == 0`). Patron de RustDesk (`scrap/src/dxgi`).
pub fn frame_unchanged(accumulated_frames: u32, last_present_time: i64) -> bool {
    accumulated_frames == 0 || last_present_time == 0
}

#[cfg(test)]
mod qw1_tests {
    use super::*;

    #[test]
    fn frame_unchanged_detects_cursor_only_and_idle_updates() {
        // Solo cursor / sin presentacion nueva -> omitir.
        assert!(frame_unchanged(0, 1_700_000_000_000_000));
        assert!(frame_unchanged(1, 0));
        assert!(frame_unchanged(0, 0));
        // Cambio real de escritorio -> procesar.
        assert!(!frame_unchanged(1, 1_700_000_000_000_000));
        assert!(!frame_unchanged(3, 123));
    }
}
