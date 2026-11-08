//! Bus d'événements interne.
//!
//! Tous les producteurs (hooks, icône de notification, futur enregistreur) publient
//! des [`Event`] horodatés ; chaque abonné reçoit sa propre copie via un canal.
//! Les événements sont sérialisables : le futur journal JSON (V2) sera un simple abonné.

use crossbeam_channel::{unbounded, Receiver, Sender};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

/// Actions de haut niveau, déclenchées par raccourci, icône ou réglages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    ToggleAll,
    CycleSpotlight,
    SetSpotlight(SpotlightModeId),
    ToggleRing,
    TogglePulse,
    ToggleClicks,
    ToggleClickSound,
    /// Mode dessin (les clics servent à dessiner au lieu d'aller à l'app).
    ToggleDraw,
    ToggleLaser,
    ToggleTrail,
    /// Outil de dessin (0 stylo, 1 cadre, 2 rond, 3 flèche) : active aussi le mode dessin.
    SetTool(u8),
    /// Couleur de dessin 0, 1 ou 2.
    SetColor(u8),
    /// Épaisseur suivante (fin → moyen → épais → très épais).
    CycleWidth,
    Undo,
    ClearDrawings,
    /// Revenir à la souris normale (quitte le mode dessin et le tableau).
    ExitDraw,
    /// Tableau : désactivé → blanc → noir → désactivé.
    CycleBoard,
    /// Tableau 1 blanc, 2 noir ; redemander le même le ferme.
    SetBoard(u8),
    /// Spotlight allumé / éteint, avec la forme choisie dans les réglages.
    ToggleSpotlight,
    Screenshot,
    ToggleToolbar,
    OpenSettings,
    /// La barre d'outils a été déplacée (coin haut-gauche, pixels physiques).
    ToolbarMoved { x: i32, y: i32 },
    /// Enregistrer : démarre (après compte à rebours) ou, pour fenêtre/zone, passe en sélection.
    RecordStart(RecordTarget),
    /// Choisir la source à la souris : 1 fenêtre, 2 zone.
    RecordPick(u8),
    /// Fin du compte à rebours : l'enregistrement démarre vraiment.
    RecordGo(RecordTarget),
    RecordStop,
    /// Raccourci : arrête si on enregistre, sinon enregistre l'écran de la souris.
    ToggleRecord,
    /// Active/désactive une piste son pour les prochains enregistrements (0 PC, 1 micro).
    ToggleRecordAudio(u8),
    OpenVideosFolder,
    /// Affichage des touches et combinaisons pressées.
    ToggleKeys,
    /// Démarre une nouvelle session : le prochain élément enregistré ouvre un nouveau dossier.
    NewSession,
    /// Ouvre le dossier de la session en cours (ou le dossier des sessions).
    OpenSessionFolder,
    ReloadConfig,
    Quit,
}

/// Source d'un enregistrement (poignées Win32 en entiers, coordonnées bureau en pixels physiques).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordTarget {
    /// L'écran où se trouve la souris.
    Screen,
    Window { hwnd: isize },
    Region { left: i32, top: i32, right: i32, bottom: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpotlightModeId {
    Off,
    Circle,
    HBand,
    VBand,
}

impl SpotlightModeId {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Circle,
            Self::Circle => Self::HBand,
            Self::HBand => Self::VBand,
            Self::VBand => Self::Off,
        }
    }

    pub fn label_fr(self) -> &'static str {
        match self {
            Self::Off => "Désactivé",
            Self::Circle => "Cercle",
            Self::HBand => "Bande horizontale",
            Self::VBand => "Bande verticale",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EventKind {
    /// Déplacement (échantillonné, pas chaque événement matériel).
    MouseMove { x: i32, y: i32 },
    MouseDown { button: MouseButton, x: i32, y: i32 },
    MouseUp { button: MouseButton, x: i32, y: i32 },
    Wheel { delta: i32, horizontal: bool, x: i32, y: i32 },
    /// Touche physique. `injected` : touche simulée par un logiciel.
    KeyDown { vk: u32, scan: u32, injected: bool },
    KeyUp { vk: u32, scan: u32, injected: bool },
    Action(Action),
    // --- Réservés pour la V2 ---
    Chapter { title: String },
    Zoom { factor: f32, x: i32, y: i32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Microsecondes depuis le démarrage du bus (horloge monotone).
    pub t_us: u64,
    pub kind: EventKind,
}

struct BusInner {
    start: Instant,
    subscribers: Mutex<Vec<Sender<Event>>>,
}

static BUS: OnceLock<BusInner> = OnceLock::new();

fn bus() -> &'static BusInner {
    BUS.get_or_init(|| BusInner {
        start: Instant::now(),
        subscribers: Mutex::new(Vec::new()),
    })
}

/// Initialise l'horloge du bus (à appeler au tout début du programme).
pub fn init() {
    let _ = bus();
}

/// Horodatage courant du bus, en microsecondes.
pub fn now_us() -> u64 {
    bus().start.elapsed().as_micros() as u64
}

/// S'abonne au bus. Chaque abonné reçoit tous les événements publiés après l'abonnement.
pub fn subscribe() -> Receiver<Event> {
    let (tx, rx) = unbounded();
    bus().subscribers.lock().unwrap().push(tx);
    rx
}

/// Publie un événement. Non bloquant ; les abonnés disparus sont retirés.
pub fn publish(kind: EventKind) {
    let ev = Event { t_us: now_us(), kind };
    let mut subs = bus().subscribers.lock().unwrap();
    subs.retain(|s| s.send(ev.clone()).is_ok());
}

/// Raccourci pour publier une action.
pub fn action(a: Action) {
    publish(EventKind::Action(a));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fan_out() {
        let a = subscribe();
        let b = subscribe();
        action(Action::ToggleRing);
        assert!(matches!(a.try_recv().unwrap().kind, EventKind::Action(Action::ToggleRing)));
        assert!(matches!(b.try_recv().unwrap().kind, EventKind::Action(Action::ToggleRing)));
    }
}
