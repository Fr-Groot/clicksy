//! État partagé entre le moteur, le rendu, les hooks et l'icône de notification.

use crate::{Color, Config, DrawTool};
use arc_swap::ArcSwap;
use fx_bus::MouseButton;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering};
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
    /// Dessins de l'écran mis de côté pendant que le tableau est ouvert (et inversement).
    pub stash: Vec<Shape>,
    /// Incrémenté à chaque modification des formes (le rendu sait quand redessiner).
    pub generation: u64,
}

impl Default for DrawState {
    fn default() -> Self {
        DrawState {
            tool: DrawTool::Pen,
            color_index: 0,
            width: 5.0,
            shapes: Vec::new(),
            current: None,
            stash: Vec::new(),
            generation: 0,
        }
    }
}

#[derive(Debug, Default)]
pub struct FxState {
    pub ripples: Vec<Ripple>,
    /// Boutons gauche, droit, milieu enfoncés.
    pub held: [bool; 3],
    pub draw: DrawState,
    pub banner: Option<Banner>,
    /// Compte à rebours avant enregistrement : début, et rectangle visé (bureau).
    pub countdown: Option<(Instant, [i32; 4])>,
    /// Zone en cours de tracé (sélection « zone ») : [gauche, haut, droite, bas].
    pub select_rect: Option<[i32; 4]>,
    /// Enregistrement en cours : début et rectangle enregistré (pour le cadre rouge).
    pub rec: Option<(Instant, [i32; 4])>,
}

/// Modes temporaires (non enregistrés), lus sans verrou par les hooks.
#[derive(Debug, Default)]
pub struct Modes {
    pub draw: AtomicBool,
    pub laser: AtomicBool,
    /// Tableau plein écran : 0 aucun, 1 blanc, 2 noir.
    pub board: AtomicU8,
    /// Un point de l'écran qui affiche le tableau (position du curseur à l'ouverture).
    pub board_x: AtomicI32,
    pub board_y: AtomicI32,
    /// Rectangle de la barre d'outils (gauche, haut, droite, bas ; pixels physiques), vide si masquée.
    pub toolbar_rect: [AtomicI32; 4],
    /// La barre d'outils suit la souris (déplacement en cours).
    pub toolbar_drag: AtomicBool,
    /// Enregistrement vidéo en cours (chrono, cadre rouge).
    pub recording: AtomicBool,
    /// Pendant l'enregistrement, la surcouche est exclue de la vidéo (mode « éditable »).
    pub rec_exclude: AtomicBool,
    /// Choix de la source à la souris : 0 aucun, 1 fenêtre, 2 zone.
    pub select: AtomicU8,
}

impl Modes {
    pub fn set_toolbar_rect(&self, r: Option<(i32, i32, i32, i32)>) {
        let (l, t, rr, b) = r.unwrap_or((0, 0, 0, 0));
        for (a, v) in self.toolbar_rect.iter().zip([l, t, rr, b]) {
            a.store(v, Ordering::Release);
        }
    }

    /// Le point est-il sur la barre d'outils (ou la barre est-elle en cours de déplacement) ?
    pub fn on_toolbar(&self, x: i32, y: i32) -> bool {
        if self.toolbar_drag.load(Ordering::Acquire) {
            return true;
        }
        let r: Vec<i32> = self.toolbar_rect.iter().map(|a| a.load(Ordering::Acquire)).collect();
        x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
    }
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
