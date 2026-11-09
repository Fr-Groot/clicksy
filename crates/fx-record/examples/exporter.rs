//! Essai d'export : `cargo run --release -p fx-record --example exporter -- <dossier projet> <sortie.mp4>`.
use std::sync::atomic::AtomicBool;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().expect("dossier projet"));
    let out = std::path::PathBuf::from(args.next().expect("fichier de sortie"));
    let view = fx_record::export::load(&dir).expect("projet");
    println!("durée {} ms, {} clics, {} zooms auto, zone {:?}", view.duration_ms, view.clicks.len(), view.edit.zooms.len(), view.crop);
    let mut edit = view.edit.clone();
    // Essai : on coupe la 2e à la 4e seconde.
    edit.cuts.push([2000, 4000]);
    let t = std::time::Instant::now();
    fx_record::export::export(&dir, &edit, &out, &AtomicBool::new(false), |p| {
        if (p * 100.0) as u32 % 20 == 0 {
            println!("{:.0} %", p * 100.0)
        }
    })
    .expect("export");
    println!("terminé en {:.1} s", t.elapsed().as_secs_f32());
}
