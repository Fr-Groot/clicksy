//! Images : rendu d'un curseur Windows en BGRA avec transparence, et écriture PNG (WIC).

use std::path::Path;
use windows::core::HSTRING;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::WindowsAndMessaging::{DrawIconEx, GetIconInfo, DI_NORMAL, HCURSOR, HICON, ICONINFO};

/// Dessine le curseur sur fond noir puis sur fond blanc : la différence donne l'alpha, ce qui
/// marche aussi pour les curseurs monochromes (barre de texte, etc.).
/// Renvoie (largeur, hauteur, point actif, pixels BGRA non prémultipliés).
pub fn cursor_bitmap(cursor: HCURSOR) -> Option<(u32, u32, (i32, i32), Vec<u8>)> {
    unsafe {
        let mut info = ICONINFO::default();
        GetIconInfo(HICON(cursor.0), &mut info).ok()?;
        let mut bm = BITMAP::default();
        let src = if !info.hbmColor.is_invalid() { info.hbmColor } else { info.hbmMask };
        GetObjectW(src.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _));
        let w = bm.bmWidth.max(1);
        // Curseur monochrome : le masque contient AND et XOR empilés (hauteur double).
        let h = if info.hbmColor.is_invalid() { bm.bmHeight / 2 } else { bm.bmHeight }.max(1);
        let hot = (info.xHotspot as i32, info.yHotspot as i32);
        if !info.hbmColor.is_invalid() {
            let _ = DeleteObject(info.hbmColor.into());
        }
        if !info.hbmMask.is_invalid() {
            let _ = DeleteObject(info.hbmMask.into());
        }

        let on_black = render(cursor, w, h, 0x000000)?;
        let on_white = render(cursor, w, h, 0xFFFFFF)?;
        let mut out = vec![0u8; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            let (b, wt) = (&on_black[i * 4..i * 4 + 3], &on_white[i * 4..i * 4 + 3]);
            // alpha = 1 - (blanc - noir) ; couleur = noir / alpha.
            let diff = (wt[0] as i32 - b[0] as i32 + wt[1] as i32 - b[1] as i32 + wt[2] as i32 - b[2] as i32) / 3;
            let a = (255 - diff).clamp(0, 255);
            out[i * 4 + 3] = a as u8;
            if a > 0 {
                for c in 0..3 {
                    out[i * 4 + c] = ((b[c] as i32 * 255) / a).clamp(0, 255) as u8;
                }
            }
        }
        Some((w as u32, h as u32, hot, out))
    }
}

unsafe fn render(cursor: HCURSOR, w: i32, h: i32, bg: u32) -> Option<Vec<u8>> {
    let screen = GetDC(None);
    let mem = CreateCompatibleDC(Some(screen));
    let bmi = BITMAPINFO {
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
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let dib = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
    let old = SelectObject(mem, dib.into());
    let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(bg));
    let rect = windows::Win32::Foundation::RECT { left: 0, top: 0, right: w, bottom: h };
    FillRect(mem, &rect, brush);
    let _ = DeleteObject(brush.into());
    let ok = DrawIconEx(mem, 0, 0, HICON(cursor.0), w, h, 0, None, DI_NORMAL).is_ok();
    let _ = GdiFlush();
    let px = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize).to_vec();
    SelectObject(mem, old);
    let _ = DeleteObject(dib.into());
    let _ = DeleteDC(mem);
    ReleaseDC(None, screen);
    ok.then_some(px)
}

/// Écrit une image BGRA (non prémultipliée) en PNG.
pub fn save(path: &Path, w: u32, h: u32, bgra: &[u8]) -> windows::core::Result<()> {
    unsafe {
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
        encoder.Commit()
    }
}
