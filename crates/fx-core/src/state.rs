//! État partagé entre le moteur, le rendu, les hooks et l'icône de notification.

use crate::{Color, Config, DrawTool};
use arc_swap::ArcSwap;
use fx_bus::MouseButton;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct Ripple {
    pub x: i32,
    pub y: i32,
    pub button: MouseButton,
    pub start: Instant,
}

/// Une forme dessinée à l'écran (coordonnées bureau, pixels physiques).
#[derive(Debug, Clone)]
pub struct Shape {
    pub tool: DrawTool,
    pub color: Color,
    /// Épaisseur en DIP.
    pub width: f32,
    /// Stylo : tous les points. Autres outils : [départ, arrivée].
    pub points: Vec<(f32, f32)>,
    /// Maj maintenue : carré, cercle, ou flèche à 45°.
    pub constrained: bool,
}

/// Bulle de confirmation affichée brièvement en haut de l'écran.
#[derive(Debug, Clone)]
pub struct Banner {
    pub text: String,
    pub start: Instant,
}

#[derive(Debug)]
pub struct DrawState {
    pub tool: DrawTool,
    pub color_index: usize,
    /// Épaisseur courante (DIP), modifiable à la molette.
    pub width: f32,
    pub shapes: Vec<Shape>,
    pub current: Option<Shape>,
    /// Incrémenté à chaque modification des formes (le rendu sait quand redessiner).
    pub generation: u64,
}

impl Default for DrawState {
    fn default() -> Self {
        DrawState { tool: DrawTool::Pen, color_index: 0, width: 5.0, shapes: Vec::new(), current: None, generation: 0 }
    }
}

#[derive(Debug, Default)]
pub struct FxState {
    pub ripples: Vec<Ripple>,
    /// Boutons gauche, droit, milieu enfoncés.
    pub held: [bool; 3],
    pub draw: DrawState,
    pub banner: Option<Banner>,
}

/// Modes temporaires (non enregistrés), lus sans verrou par les hooks.
#[derive(Debug, Default)]
pub struct Modes {
    pub draw: AtomicBool,
    pub laser: AtomicBool,
}

pub struct Shared {
    pub config: ArcSwap<Config>,
    pub fx: Mutex<FxState>,
    pub modes: Modes,
    pub data_dir: PathBuf,
    waker: OnceLock<Box<dyn Fn() + Send + Sync>>,
}

impl Shared {
    pub fn new(config: Config, data_dir: PathBuf) -> Arc<Self> {
        Arc::new(Shared {
            config: ArcSwap::from_pointee(config),
            fx: Mutex::new(FxState::default()),
            modes: Modes::default(),
            data_dir,
            waker: OnceLock::new(),
        })
    }

    /// Le moteur de rendu enregistre ici sa fonction de réveil.
    pub fn set_waker(&self, f: impl Fn() + Send + Sync + 'static) {
        let _ = self.waker.set(Box::new(f));
    }

    /// Demande un nouveau rendu (appelable depuis n'importe quel thread).
    pub fn wake(&self) {
        if let Some(f) = self.waker.get() {
            f();
        }
    }

    pub fn update_config(&self, f: impl FnOnce(&mut Config)) {
        let mut c = (**self.config.load()).clone();
        f(&mut c);
        c.sanitize();
        self.config.store(Arc::new(c));
        self.wake();
    }

    /// Affiche une bulle de confirmation (si l'option est active).
    pub fn banner(&self, text: impl Into<String>) {
        if !self.config.load().show_banner {
            return;
        }
        self.fx.lock().unwrap().banner = Some(Banner { text: text.into(), start: Instant::now() });
        self.wake();
    }
}
