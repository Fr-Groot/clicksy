//! Enregistrement d'écran de Clicksy.
//!
//! Un enregistrement produit un **dossier projet** :
//! - `screen.mp4` : la vidéo, **sans curseur ni effets** (ils seront redessinés à l'export,
//!   donc modifiables après coup) ;
//! - `system.wav` / `mic.wav` : son du PC et micro, pistes séparées, alignées sur la vidéo ;
//! - `events.jsonl` : positions et forme du curseur, clics, molette (jamais les touches) ;
//! - `cursors/` : images des formes de curseur rencontrées ;
//! - `project.json` : métadonnées (source, tailles, horloges).
//!
//! Horloge commune : unités de 100 ns sur la base QueryPerformanceCounter, la même que les
//! horodatages des images de Windows.Graphics.Capture et des paquets WASAPI.

mod audio;
mod clock;
mod journal;
mod png;
mod project;
mod video;

pub use project::{Project, SourceInfo};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

/// Ce que l'on enregistre. Les poignées Win32 sont passées en entiers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// Écran entier (HMONITOR).
    Monitor { hmonitor: isize },
    /// Une fenêtre (HWND).
    Window { hwnd: isize },
    /// Une zone : on enregistre l'écran qui la contient, la zone est appliquée à l'export.
    Region { hmonitor: isize, left: i32, top: i32, right: i32, bottom: i32 },
}

#[derive(Debug, Clone)]
pub struct Options {
    pub target: Target,
    pub fps: u32,
    pub system_audio: bool,
    pub mic: bool,
    /// Filmer aussi le curseur de Windows.
    pub cursor: bool,
    /// Dossier parent ; le projet est créé dedans.
    pub folder: PathBuf,
}

pub struct Recording {
    dir: PathBuf,
    project: Project,
    stop: Arc<AtomicBool>,
    video: video::VideoCapture,
    workers: Vec<(&'static str, JoinHandle<Result<(), String>>)>,
    started: Instant,
}

impl Recording {
    pub fn start(opts: Options) -> Result<Recording, String> {
        let dir = project::new_dir(&opts.folder)?;
        let t0 = clock::now_100ns();
        let stop = Arc::new(AtomicBool::new(false));
        let source = project::describe(&opts.target)?;

        let video = video::VideoCapture::start(&opts.target, &source, opts.fps, opts.cursor, &dir.join("screen.mp4"))?;

        let mut workers = Vec::new();
        if opts.system_audio {
            let (s, p) = (stop.clone(), dir.join("system.wav"));
            workers.push(("son du PC", std::thread::spawn(move || audio::record(audio::Kind::System, &p, t0, &s))));
        }
        if opts.mic {
            let (s, p) = (stop.clone(), dir.join("mic.wav"));
            workers.push(("micro", std::thread::spawn(move || audio::record(audio::Kind::Mic, &p, t0, &s))));
        }
        {
            let (s, d, src) = (stop.clone(), dir.clone(), source.clone());
            let rx = fx_bus::subscribe();
            workers.push(("journal", std::thread::spawn(move || journal::record(&d, t0, &src, rx, &s))));
        }

        let project = Project::new(t0, source, opts.fps, opts.system_audio, opts.mic);
        Ok(Recording { dir, project, stop, video, workers, started: Instant::now() })
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.started.elapsed()
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Arrête tout, finalise les fichiers et écrit `project.json`. Renvoie le dossier projet
    /// et les avertissements éventuels (une piste son indisponible, par exemple).
    pub fn stop(mut self) -> Result<(PathBuf, Vec<String>), String> {
        self.stop.store(true, Ordering::Release);
        let mut warnings = Vec::new();
        let v = self.video.stop();
        for (name, h) in self.workers.drain(..) {
            match h.join() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => warnings.push(format!("{name} : {e}")),
                Err(_) => warnings.push(format!("{name} : arrêt inattendu")),
            }
        }
        let (first_frame, frames, size) = v?;
        self.project.finish(first_frame, frames, size, self.started.elapsed(), &self.dir);
        self.project.save(&self.dir)?;
        Ok((self.dir, warnings))
    }
}
