//! Essai : enregistre l'écran principal pendant 5 s (son du PC + micro) dans le dossier donné.
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTOPRIMARY};

fn main() {
    unsafe { let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2); }
    fx_bus::init();
    let folder = std::env::args().nth(1).expect("dossier");
    let hmon = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    let rec = fx_record::Recording::start(fx_record::Options {
        target: fx_record::Target::Monitor { hmonitor: hmon.0 as isize },
        fps: 30,
        system_audio: true,
        mic: true,
        cursor: true,
        folder: folder.into(),
        name: None,
    })
    .expect("démarrage");
    std::thread::sleep(std::time::Duration::from_secs(5));
    let (dir, warnings) = rec.stop().expect("arrêt");
    println!("{}", dir.display());
    for w in warnings { println!("avertissement : {w}"); }
}
