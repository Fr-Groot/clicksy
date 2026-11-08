//! Métadonnées du projet (`project.json`) et description de la source.

use crate::Target;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITORINFO};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;

/// Description figée de la source au début de l'enregistrement (pixels physiques, bureau).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInfo {
    /// "ecran", "fenetre" ou "zone".
    pub kind: String,
    /// Rectangle capturé au départ [gauche, haut, droite, bas] : l'écran, ou la fenêtre.
    pub capture_rect: [i32; 4],
    /// Zone à garder à l'export (zone uniquement), en coordonnées bureau.
    pub region: Option<[i32; 4]>,
    pub window_title: Option<String>,
    /// Échelle de l'écran (1.0 = 100 %).
    pub scale: f32,
    #[serde(skip)]
    pub hwnd: isize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub format: u32,
    pub app: String,
    pub created: String,
    pub source: SourceInfo,
    /// Début de l'enregistrement (horloge 100 ns) ; les `t` de events.jsonl sont relatifs à lui.
    pub t0_100ns: i64,
    pub video: VideoInfo,
    pub audio: AudioInfo,
    pub events: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoInfo {
    pub file: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Horodatage de la 1ʳᵉ image (horloge 100 ns) : la vidéo commence à `first_frame - t0`.
    pub first_frame_100ns: i64,
    pub frames: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioInfo {
    pub system: Option<String>,
    pub mic: Option<String>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Project {
    pub fn new(t0: i64, source: SourceInfo, fps: u32, system: bool, mic: bool) -> Self {
        Project {
            format: 1,
            app: format!("Clicksy {}", env!("CARGO_PKG_VERSION")),
            created: timestamp_label(),
            source,
            t0_100ns: t0,
            video: VideoInfo { file: "screen.mp4".into(), width: 0, height: 0, fps, first_frame_100ns: 0, frames: 0 },
            audio: AudioInfo {
                system: system.then(|| "system.wav".into()),
                mic: mic.then(|| "mic.wav".into()),
                sample_rate: crate::audio::RATE,
                channels: 2,
            },
            events: "events.jsonl".into(),
            duration_ms: 0,
        }
    }

    pub fn finish(&mut self, first_frame: i64, frames: u64, size: (u32, u32), elapsed: std::time::Duration, dir: &Path) {
        self.video.first_frame_100ns = first_frame;
        self.video.frames = frames;
        self.video.width = size.0;
        self.video.height = size.1;
        self.duration_ms = elapsed.as_millis() as u64;
        // Une piste son qui n'a rien produit n'est pas référencée.
        for slot in [&mut self.audio.system, &mut self.audio.mic] {
            if let Some(f) = slot {
                if !dir.join(&*f).exists() {
                    *slot = None;
                }
            }
        }
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("project.json"), text).map_err(|e| format!("project.json : {e}"))
    }
}

fn timestamp_label() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02} {:02}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// Crée `<dossier>\<nom>`, ou `<dossier>\Enregistrement AAAA-MM-JJ HH-MM-SS` sans nom.
pub fn new_dir(parent: &Path, name: Option<&str>) -> Result<PathBuf, String> {
    let dir = match name {
        Some(n) => parent.join(n),
        None => parent.join(format!("Enregistrement {}", timestamp_label())),
    };
    std::fs::create_dir_all(dir.join("cursors")).map_err(|e| format!("dossier {} : {e}", dir.display()))?;
    Ok(dir)
}

fn monitor_rect(hmonitor: isize) -> Result<(RECT, f32), String> {
    let hmon = HMONITOR(hmonitor as *mut _);
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        if !GetMonitorInfoW(hmon, &mut info).as_bool() {
            return Err("écran introuvable".into());
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        Ok((info.rcMonitor, dx as f32 / 96.0))
    }
}

/// Rectangle visible d'une fenêtre (sans l'ombre invisible de Windows 10/11).
pub fn window_rect(hwnd: isize) -> Option<RECT> {
    let mut r = RECT::default();
    unsafe {
        DwmGetWindowAttribute(
            HWND(hwnd as *mut _),
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut _ as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
        .ok()?;
    }
    Some(r)
}

fn arr(r: RECT) -> [i32; 4] {
    [r.left, r.top, r.right, r.bottom]
}

pub fn describe(target: &Target) -> Result<SourceInfo, String> {
    Ok(match *target {
        Target::Monitor { hmonitor } => {
            let (r, scale) = monitor_rect(hmonitor)?;
            SourceInfo { kind: "ecran".into(), capture_rect: arr(r), region: None, window_title: None, scale, hwnd: 0 }
        }
        Target::Region { hmonitor, left, top, right, bottom } => {
            let (r, scale) = monitor_rect(hmonitor)?;
            SourceInfo {
                kind: "zone".into(),
                capture_rect: arr(r),
                region: Some([left, top, right, bottom]),
                window_title: None,
                scale,
                hwnd: 0,
            }
        }
        Target::Window { hwnd } => {
            let r = window_rect(hwnd).ok_or("fenêtre introuvable")?;
            let mut buf = [0u16; 256];
            let n = unsafe { GetWindowTextW(HWND(hwnd as *mut _), &mut buf) };
            let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
            let hmon = unsafe {
                windows::Win32::Graphics::Gdi::MonitorFromWindow(HWND(hwnd as *mut _), windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST)
            };
            let scale = monitor_rect(hmon.0 as isize).map(|m| m.1).unwrap_or(1.0);
            SourceInfo { kind: "fenetre".into(), capture_rect: arr(r), region: None, window_title: Some(title), scale, hwnd }
        }
    })
}
