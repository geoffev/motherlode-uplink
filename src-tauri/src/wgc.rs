// Windows Graphics Capture of one window. The desktop compositor hands over
// the window's pixels; the game process is never opened or touched.
//
// One capture session stays open per window and the latest frame is pulled on
// each grab, rather than a session per frame (which re-flashes the capture
// border and costs more). Based on xcap's wgc.rs (MIT/Apache-2.0), without its
// per-frame process handle (it opened the target process to check DPI).

use std::time::{Duration, Instant};

use image::RgbaImage;
use windows::core::{factory, Interface};
use windows::Graphics::Capture::{Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D, D3D11_BOX,
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

const FORMAT: DirectXPixelFormat = DirectXPixelFormat::B8G8R8A8UIntNormalized;
/// How long the first grab waits for the compositor's first frame.
const FIRST_FRAME_WAIT: Duration = Duration::from_millis(1500);

pub struct WindowCapture {
    hwnd: isize,
    d3d: ID3D11Device,
    ctx: ID3D11DeviceContext,
    device: IDirect3DDevice,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    size: SizeInt32,
    /// The newest frame so far: returned again when the window hasn't redrawn.
    last: Option<RgbaImage>,
}

fn err(e: windows::core::Error) -> String {
    e.message().to_string()
}

impl WindowCapture {
    pub fn new(hwnd: isize) -> Result<WindowCapture, String> {
        // SAFETY: plain COM/D3D calls on this thread; every out-pointer is a
        // local that outlives the call.
        unsafe {
            // Already initialised on this thread is fine.
            let _ = RoInitialize(RO_INIT_MULTITHREADED);

            let mut d3d = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                None,
            )
            .map_err(err)?;
            let d3d: ID3D11Device = d3d.ok_or("No Direct3D device")?;
            let ctx = d3d.GetImmediateContext().map_err(err)?;
            let dxgi: IDXGIDevice = d3d.cast().map_err(err)?;
            let device: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi).map_err(err)?.cast().map_err(err)?;

            let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(err)?;
            let item: GraphicsCaptureItem = interop.CreateForWindow(HWND(hwnd as *mut _)).map_err(err)?;
            let size = item.Size().map_err(err)?;
            let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&device, FORMAT, 2, size).map_err(err)?;
            let session = pool.CreateCaptureSession(&item).map_err(err)?;
            // Best effort: no yellow capture border (Windows 11) and no cursor.
            let _ = session.SetIsBorderRequired(false);
            let _ = session.SetIsCursorCaptureEnabled(false);
            session.StartCapture().map_err(err)?;

            Ok(WindowCapture { hwnd, d3d, ctx, device, pool, session, size, last: None })
        }
    }

    pub fn hwnd(&self) -> isize {
        self.hwnd
    }

    /// The window's latest picture.
    pub fn grab(&mut self) -> Result<RgbaImage, String> {
        let deadline = Instant::now() + if self.last.is_none() { FIRST_FRAME_WAIT } else { Duration::ZERO };
        loop {
            if let Some(frame) = self.newest_frame() {
                let img = self.read(&frame);
                let _ = frame.Close();
                let img = img?;
                self.last = Some(img.clone());
                return Ok(img);
            }
            if let Some(last) = &self.last {
                return Ok(last.clone());
            }
            if Instant::now() >= deadline {
                return Err("The game window isn't sending any frames".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Drains the pool, keeping only the most recent frame.
    fn newest_frame(&mut self) -> Option<Direct3D11CaptureFrame> {
        let mut newest: Option<Direct3D11CaptureFrame> = None;
        while let Ok(f) = self.pool.TryGetNextFrame() {
            if let Some(old) = newest.replace(f) {
                let _ = old.Close();
            }
        }
        newest
    }

    fn read(&mut self, frame: &Direct3D11CaptureFrame) -> Result<RgbaImage, String> {
        let content = frame.ContentSize().map_err(err)?;
        if content.Width != self.size.Width || content.Height != self.size.Height {
            // The window was resized: later frames come at the new size.
            self.pool.Recreate(&self.device, FORMAT, 2, content).map_err(err)?;
            self.size = content;
        }
        // SAFETY: COM/D3D calls on resources owned here; the mapped pointer is
        // only read within the reported row pitch and height, before Unmap.
        unsafe {
            let access: IDirect3DDxgiInterfaceAccess = frame.Surface().map_err(err)?.cast().map_err(err)?;
            let source: ID3D11Texture2D = access.GetInterface().map_err(err)?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            source.GetDesc(&mut desc);
            let w = (content.Width.max(1) as u32).min(desc.Width);
            let h = (content.Height.max(1) as u32).min(desc.Height);

            let mut staging_desc = desc;
            staging_desc.Width = w;
            staging_desc.Height = h;
            staging_desc.BindFlags = 0;
            staging_desc.MiscFlags = 0;
            staging_desc.Usage = D3D11_USAGE_STAGING;
            staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            let mut staging = None;
            self.d3d.CreateTexture2D(&staging_desc, None, Some(&mut staging)).map_err(err)?;
            let staging: ID3D11Texture2D = staging.ok_or("No staging texture")?;

            let region = D3D11_BOX { left: 0, top: 0, right: w, bottom: h, front: 0, back: 1 };
            let dst: ID3D11Resource = staging.cast().map_err(err)?;
            let src: ID3D11Resource = source.cast().map_err(err)?;
            self.ctx.CopySubresourceRegion(Some(&dst), 0, 0, 0, 0, Some(&src), 0, Some(&region));

            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx.Map(Some(&dst), 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(err)?;
            let pitch = mapped.RowPitch as usize;
            let bytes = std::slice::from_raw_parts(mapped.pData as *const u8, pitch * h as usize);
            let mut rgba = Vec::with_capacity((w * h * 4) as usize);
            for row in bytes.chunks(pitch).take(h as usize) {
                for px in row[..(w * 4) as usize].chunks_exact(4) {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
                }
            }
            self.ctx.Unmap(Some(&dst), 0);
            RgbaImage::from_raw(w, h, rgba).ok_or_else(|| "Bad frame size".to_string())
        }
    }
}

impl Drop for WindowCapture {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Grabs a frame of a real window (kept in memory only) and checks it has
    /// pixels. Needs a desktop session. Pick the window by title, since the
    /// largest one can be an invisible overlay:
    ///   WGC_TEST_TITLE="Visual Studio Code" cargo test captures_a_window -- --ignored --nocapture
    #[test]
    #[ignore]
    fn captures_a_window() {
        let w = xcap::Window::all()
            .unwrap()
            .into_iter()
            .filter(|w| !w.is_minimized().unwrap_or(true))
            .filter(|w| std::env::var("WGC_TEST_TITLE").map_or(true, |t| w.title().unwrap_or_default().contains(&t)))
            .max_by_key(|w| w.width().unwrap_or(0) * w.height().unwrap_or(0))
            .expect("a visible window");
        println!("window: {:?} {}x{}", w.title().unwrap_or_default(), w.width().unwrap_or(0), w.height().unwrap_or(0));
        let mut cap = WindowCapture::new(w.id().unwrap() as isize).unwrap();
        let a = cap.grab().unwrap();
        let b = cap.grab().unwrap();
        let lit = a.pixels().filter(|p| p.0[0] as u32 + p.0[1] as u32 + p.0[2] as u32 > 60).count();
        println!("{}x{} frame, {:.0}% lit pixels; second grab {}x{}", a.width(), a.height(), 100.0 * lit as f64 / (a.width() * a.height()) as f64, b.width(), b.height());
        assert!(a.width() > 100 && a.height() > 100);
        assert!(lit > 0, "frame is all black");
    }
}
