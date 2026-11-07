//! Périphériques GPU partagés par toutes les surfaces, et une surface par écran.

use windows::core::{Interface, Result, PCWSTR};
use windows::Win32::Foundation::{HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct Gpu {
    pub d3d: ID3D11Device,
    pub dxgi_factory: IDXGIFactory2,
    pub dc: ID2D1DeviceContext,
    pub dcomp: IDCompositionDevice,
    pub brush: ID2D1SolidColorBrush,
    pub factory: ID2D1Factory1,
    /// Extrémités et angles arrondis : traits de dessin et traînée propres.
    pub round: ID2D1StrokeStyle,
    pub dwrite: IDWriteFactory,
}

impl Gpu {
    pub fn new() -> Result<Self> {
        unsafe {
            let d3d = create_d3d(D3D_DRIVER_TYPE_HARDWARE).or_else(|_| create_d3d(D3D_DRIVER_TYPE_WARP))?;
            let dxgi_device: IDXGIDevice = d3d.cast()?;
            let dxgi_factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;

            let opts = D2D1_FACTORY_OPTIONS { debugLevel: D2D1_DEBUG_LEVEL_NONE };
            let d2d_factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, Some(&opts))?;
            let d2d_device = d2d_factory.CreateDevice(&dxgi_device)?;
            let dc = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            dc.SetUnitMode(D2D1_UNIT_MODE_PIXELS);
            dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
            let brush = dc.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;
            let props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_SOLID,
                dashOffset: 0.0,
            };
            let base: &ID2D1Factory = &d2d_factory;
            let round: ID2D1StrokeStyle = base.CreateStrokeStyle(&props, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            Ok(Gpu { d3d, dxgi_factory, dc, dcomp, brush, factory: d2d_factory, round, dwrite })
        }
    }
}

unsafe fn create_d3d(driver: D3D_DRIVER_TYPE) -> Result<ID3D11Device> {
    let mut device = None;
    D3D11CreateDevice(
        None,
        driver,
        HMODULE::default(),
        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        Some(&[D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_10_0]),
        D3D11_SDK_VERSION,
        Some(&mut device),
        None,
        None,
    )?;
    Ok(device.unwrap())
}

/// Une fenêtre de surcouche couvrant exactement un écran.
pub struct Surface {
    pub hwnd: HWND,
    /// Rectangle de l'écran, en pixels physiques (coordonnées bureau virtuel).
    pub rect: RECT,
    /// Échelle de l'écran (1.0 = 100 %, 1.5 = 150 %).
    pub scale: f32,
    pub swapchain: IDXGISwapChain1,
    pub target: ID2D1Bitmap1,
    _dcomp_target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    /// Ce qui est actuellement affiché, pour éviter les rendus inutiles.
    pub shown: Shown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    /// Jamais dessinée (ou à redessiner).
    Unknown,
    /// Contenu indépendant du curseur, pour la config identifiée par ce jeton.
    Static(usize),
    /// Contenu dépendant du curseur : à redessiner à chaque image.
    Dynamic,
}

pub const CLASS_NAME: PCWSTR = windows::core::w!("ClicksyOverlay");

impl Surface {
    pub fn new(gpu: &Gpu, rect: RECT, scale: f32) -> Result<Self> {
        unsafe {
            let w = (rect.right - rect.left).max(1);
            let h = (rect.bottom - rect.top).max(1);
            let hinst = GetModuleHandleW(None)?;
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_NOREDIRECTIONBITMAP,
                CLASS_NAME,
                windows::core::w!("Clicksy"),
                WS_POPUP,
                rect.left,
                rect.top,
                w,
                h,
                None,
                None,
                Some(hinst.into()),
                None,
            )?;

            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: w as u32,
                Height: h as u32,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                Stereo: false.into(),
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                Flags: 0,
            };
            let swapchain = gpu.dxgi_factory.CreateSwapChainForComposition(&gpu.d3d, &desc, None)?;
            let surface: IDXGISurface = swapchain.GetBuffer(0)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                colorContext: std::mem::ManuallyDrop::new(None),
            };
            let target = gpu.dc.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;

            let dcomp_target = gpu.dcomp.CreateTargetForHwnd(hwnd, true)?;
            let visual = gpu.dcomp.CreateVisual()?;
            visual.SetContent(&swapchain)?;
            dcomp_target.SetRoot(&visual)?;
            gpu.dcomp.Commit()?;

            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            Ok(Surface {
                hwnd,
                rect,
                scale,
                swapchain,
                target,
                _dcomp_target: dcomp_target,
                _visual: visual,
                shown: Shown::Unknown,
            })
        }
    }

    pub fn width(&self) -> f32 {
        (self.rect.right - self.rect.left) as f32
    }

    pub fn height(&self) -> f32 {
        (self.rect.bottom - self.rect.top) as f32
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.rect.left && x < self.rect.right && y >= self.rect.top && y < self.rect.bottom
    }

    pub fn raise(&self) {
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
