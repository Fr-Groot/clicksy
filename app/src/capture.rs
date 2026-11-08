//! Capture de l'écran où se trouve la souris : PNG dans le dossier de la session
//! (ou Images\Clicksy si les sessions sont désactivées) et copie dans le presse-papiers.
//! Les effets Clicksy visibles sont inclus ; la barre d'outils ne l'est pas si elle est
//! exclue des captures.

use fx_core::Shared;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{HANDLE, HGLOBAL, POINT};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_DIB;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

pub fn take(shared: &Shared) {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    let cfg = shared.config.load_full();
    match save(shared, pt, "Capture", cfg.capture.copy_to_clipboard) {
        Ok(path) => {
            let extra = if cfg.capture.copy_to_clipboard { " et copiée" } else { "" };
            shared.banner(format!("Capture enregistrée{extra} : {}", file_name(&path)));
        }
        Err(e) => shared.banner(format!("Capture impossible : {e}")),
    }
}

pub fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

/// Enregistre l'écran qui contient `pt` (« Capture » ou « Tableau »).
/// Bulle, touches affichées et pointeur de dessin sont masqués le temps de la capture.
pub fn save(shared: &Shared, pt: POINT, kind: &str, clipboard: bool) -> Result<PathBuf, String> {
    shared.fx.lock().unwrap().banner = None;
    shared.modes.snapshot.store(true, Ordering::Release);
    shared.wake();
    std::thread::sleep(std::time::Duration::from_millis(150));
    let grabbed = unsafe { grab(pt) };
    shared.modes.snapshot.store(false, Ordering::Release);
    shared.wake();
    let (w, h, pixels) = grabbed?;
    let path = crate::session::image_path(shared, kind)?;
    unsafe {
        save_png(&path, w as u32, h as u32, &pixels).map_err(|e| format!("PNG : {e}"))?;
        if clipboard {
            let _ = copy_to_clipboard(w, h, &pixels);
        }
    }
    Ok(path)
}

/// Copie de l'écran composé (fenêtres + surcouche Clicksy), en BGRA opaque.
unsafe fn grab(pt: POINT) -> Result<(i32, i32, Vec<u8>), String> {
    let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if !GetMonitorInfoW(mon, &mut info).as_bool() {
        return Err("écran introuvable".into());
    }
    let r = info.rcMonitor;
    let (w, h) = (r.right - r.left, r.bottom - r.top);

    // Copie de l'écran composé (fenêtres + surcouche Clicksy).
    let screen = GetDC(None);
    let mem = CreateCompatibleDC(Some(screen));
    let bmp = CreateCompatibleBitmap(screen, w, h);
    let old = SelectObject(mem, bmp.into());
    let ok = BitBlt(mem, 0, 0, w, h, Some(screen), r.left, r.top, SRCCOPY | CAPTUREBLT);
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    let lines = GetDIBits(mem, bmp, 0, h as u32, Some(pixels.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
    SelectObject(mem, old);
    let _ = DeleteObject(bmp.into());
    let _ = DeleteDC(mem);
    ReleaseDC(None, screen);
    if ok.is_err() || lines == 0 {
        return Err("lecture de l'écran refusée".into());
    }
    for px in pixels.chunks_exact_mut(4) {
        px[3] = 255;
    }
    Ok((w, h, pixels))
}

unsafe fn save_png(path: &Path, w: u32, h: u32, bgra: &[u8]) -> windows::core::Result<()> {
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
    let stream = factory.CreateStream()?;
    stream.InitializeFromFilename(&HSTRING::from(path.as_os_str()), windows::Win32::Foundation::GENERIC_WRITE.0)?;
    let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
    encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
    let mut frame = None;
    let mut props = None;
    encoder.CreateNewFrame(&mut frame, &mut props)?;
    let frame = frame.unwrap();
    frame.Initialize(props.as_ref())?;
    frame.SetSize(w, h)?;
    let mut fmt = GUID_WICPixelFormat32bppBGRA;
    frame.SetPixelFormat(&mut fmt)?;
    frame.WritePixels(h, w * 4, bgra)?;
    frame.Commit()?;
    encoder.Commit()?;
    let _ = PWSTR::null();
    Ok(())
}

/// Presse-papiers au format DIB (lignes de bas en haut, 32 bits).
unsafe fn copy_to_clipboard(w: i32, h: i32, bgra: &[u8]) -> windows::core::Result<()> {
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: h,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        biSizeImage: (w * h * 4) as u32,
        ..Default::default()
    };
    let hsize = std::mem::size_of::<BITMAPINFOHEADER>();
    let total = hsize + bgra.len();
    let hglobal: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, total)?;
    let dst = GlobalLock(hglobal) as *mut u8;
    std::ptr::copy_nonoverlapping(&header as *const _ as *const u8, dst, hsize);
    let stride = (w * 4) as usize;
    for row in 0..h as usize {
        let src = &bgra[(h as usize - 1 - row) * stride..][..stride];
        std::ptr::copy_nonoverlapping(src.as_ptr(), dst.add(hsize + row * stride), stride);
    }
    let _ = GlobalUnlock(hglobal);
    OpenClipboard(None)?;
    let _ = EmptyClipboard();
    let res = SetClipboardData(CF_DIB.0 as u32, Some(HANDLE(hglobal.0)));
    let _ = CloseClipboard();
    res.map(|_| ())
}
