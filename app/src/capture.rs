//! Capture d'écran de l'écran où se trouve la souris : PNG dans Images\Clicksy
//! (ou le dossier choisi) et copie dans le presse-papiers.
//! Les effets Clicksy visibles sont inclus ; la barre d'outils ne l'est pas si elle est
//! exclue des captures.

use fx_core::Shared;
use std::path::{Path, PathBuf};
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{HANDLE, HGLOBAL, POINT};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_DIB;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Shell::{FOLDERID_Pictures, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

pub fn take(shared: &Shared) {
    // On retire la bulle éventuelle pour qu'elle n'apparaisse pas sur l'image.
    shared.fx.lock().unwrap().banner = None;
    shared.wake();
    std::thread::sleep(std::time::Duration::from_millis(150));

    let cfg = shared.config.load_full();
    match unsafe { capture(&cfg.capture.folder, cfg.capture.copy_to_clipboard) } {
        Ok(path) => {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let extra = if cfg.capture.copy_to_clipboard { " et copiée" } else { "" };
            shared.banner(format!("Capture enregistrée{extra} : {name}"));
        }
        Err(e) => shared.banner(format!("Capture impossible : {e}")),
    }
}

fn pictures_dir() -> PathBuf {
    unsafe {
        if let Ok(p) = SHGetKnownFolderPath(&FOLDERID_Pictures, KF_FLAG_DEFAULT, None) {
            let s = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const _));
            if !s.is_empty() {
                return PathBuf::from(s);
            }
        }
    }
    std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Pictures")).unwrap_or_else(std::env::temp_dir)
}

unsafe fn capture(folder: &str, clipboard: bool) -> Result<PathBuf, String> {
    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
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

    let dir = if folder.trim().is_empty() { pictures_dir().join("Clicksy") } else { PathBuf::from(folder.trim()) };
    std::fs::create_dir_all(&dir).map_err(|e| format!("dossier {} : {e}", dir.display()))?;
    let t = GetLocalTime();
    let path = dir.join(format!(
        "Capture {:04}-{:02}-{:02} {:02}-{:02}-{:02}.png",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    ));
    save_png(&path, w as u32, h as u32, &pixels).map_err(|e| format!("PNG : {e}"))?;
    if clipboard {
        let _ = copy_to_clipboard(w, h, &pixels);
    }
    Ok(path)
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
