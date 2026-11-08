//! Vidéo : Windows.Graphics.Capture (sans curseur, sans cadre jaune) encodée en H.264 par
//! l'encodeur matériel de Windows, via la crate MIT `windows-capture`.

use crate::project::SourceInfo;
use crate::Target;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::{AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, MinimumUpdateIntervalSettings,
    SecondaryWindowSettings, Settings,
};
use windows_capture::window::Window;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub struct Flags {
    path: PathBuf,
    width: u32,
    height: u32,
    fps: u32,
    first_ts: Arc<AtomicI64>,
    frames: Arc<AtomicU64>,
}

pub struct Handler {
    encoder: Option<VideoEncoder>,
    first_ts: Arc<AtomicI64>,
    frames: Arc<AtomicU64>,
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = Flags;
    type Error = BoxError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let f = ctx.flags;
        // Débit adapté à la taille : ~0,1 bit par pixel et par image, plafonné à 40 Mbit/s.
        let bitrate = ((f.width as u64 * f.height as u64 * f.fps as u64) / 10).clamp(4_000_000, 40_000_000) as u32;
        let encoder = VideoEncoder::new(
            VideoSettingsBuilder::new(f.width, f.height).frame_rate(f.fps).bitrate(bitrate),
            AudioSettingsBuilder::default().disabled(true),
            ContainerSettingsBuilder::default(),
            &f.path,
        )?;
        Ok(Handler { encoder: Some(encoder), first_ts: f.first_ts, frames: f.frames })
    }

    fn on_frame_arrived(&mut self, frame: &mut Frame, _ctrl: InternalCaptureControl) -> Result<(), Self::Error> {
        if let Some(enc) = self.encoder.as_mut() {
            if self.first_ts.load(Ordering::Acquire) == 0 {
                self.first_ts.store(frame.timestamp()?.Duration, Ordering::Release);
            }
            enc.send_frame(frame)?;
            self.frames.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }
}

pub struct VideoCapture {
    control: Option<CaptureControl<Handler, BoxError>>,
    first_ts: Arc<AtomicI64>,
    frames: Arc<AtomicU64>,
    size: (u32, u32),
}

/// H.264 exige des dimensions paires.
fn even(v: i32) -> u32 {
    (v.max(2) as u32) & !1
}

impl VideoCapture {
    pub fn start(target: &Target, source: &SourceInfo, fps: u32, path: &Path) -> Result<Self, String> {
        let r = source.capture_rect;
        let size = (even(r[2] - r[0]), even(r[3] - r[1]));
        let first_ts = Arc::new(AtomicI64::new(0));
        let frames = Arc::new(AtomicU64::new(0));
        let flags = Flags {
            path: path.to_path_buf(),
            width: size.0,
            height: size.1,
            fps,
            first_ts: first_ts.clone(),
            frames: frames.clone(),
        };
        let interval = MinimumUpdateIntervalSettings::Custom(Duration::from_micros(1_000_000 / fps.max(1) as u64));
        let control = match *target {
            Target::Window { hwnd } => {
                let item = Window::from_raw_hwnd(hwnd as *mut _);
                Handler::start_free_threaded(settings(item, interval, flags))
            }
            Target::Monitor { hmonitor } | Target::Region { hmonitor, .. } => {
                let item = Monitor::from_raw_hmonitor(hmonitor as *mut _);
                Handler::start_free_threaded(settings(item, interval, flags))
            }
        }
        .map_err(|e| format!("capture vidéo impossible : {e}"))?;
        Ok(VideoCapture { control: Some(control), first_ts, frames, size })
    }

    /// Finalise le MP4. Renvoie (horodatage 1ʳᵉ image, nombre d'images, taille).
    pub fn stop(&mut self) -> Result<(i64, u64, (u32, u32)), String> {
        if let Some(control) = self.control.take() {
            let encoder = control.callback().lock().encoder.take();
            let _ = control.stop();
            if let Some(enc) = encoder {
                enc.finish().map_err(|e| format!("finalisation de la vidéo : {e}"))?;
            }
        }
        let frames = self.frames.load(Ordering::Acquire);
        if frames == 0 {
            return Err("aucune image reçue de Windows (contenu protégé ou capture refusée)".into());
        }
        Ok((self.first_ts.load(Ordering::Acquire), frames, self.size))
    }
}

fn settings<T: TryInto<windows_capture::settings::GraphicsCaptureItemType>>(
    item: T,
    interval: MinimumUpdateIntervalSettings,
    flags: Flags,
) -> Settings<Flags, T> {
    Settings::new(
        item,
        // Le curseur est redessiné à l'export (effets éditables).
        CursorCaptureSettings::WithoutCursor,
        // Pas de cadre jaune autour de la zone capturée (Windows 11 ; ignoré sur Windows 10).
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        interval,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        flags,
    )
}
