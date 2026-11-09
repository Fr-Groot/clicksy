//! Configuration persistée en JSON. Toutes les tailles sont en DIP
//! (pixels logiques à 100 %) et multipliées par l'échelle de chaque écran au rendu.

use crate::{Color, Hotkey};
pub use fx_bus::SpotlightModeId as SpotlightMode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    /// Interrupteur général.
    pub enabled: bool,
    pub spotlight: SpotlightCfg,
    pub ring: RingCfg,
    pub clicks: ClicksCfg,
    pub draw: DrawCfg,
    pub trail: TrailCfg,
    pub laser: LaserCfg,
    /// Bulle de confirmation en haut de l'écran quand on utilise un raccourci.
    pub show_banner: bool,
    pub toolbar: ToolbarCfg,
    pub capture: CaptureCfg,
    pub record: RecordCfg,
    pub keys: KeysCfg,
    pub session: SessionCfg,
    pub hotkeys: HotkeysCfg,
    /// Langue des menus, de la barre et des réglages.
    pub language: crate::i18n::Language,
    /// Profils d'utilisation (Tuto, Classe virtuelle…) : des jeux de réglages appliqués en un clic.
    pub profiles: Vec<Profile>,
    /// Nom du dernier profil appliqué (vide : aucun).
    pub active_profile: String,
}

/// Un profil : nom + réglages des effets, du dessin, des touches et de l'enregistrement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub settings: ProfileSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileSettings {
    pub enabled: bool,
    pub show_banner: bool,
    pub spotlight: SpotlightCfg,
    pub ring: RingCfg,
    pub clicks: ClicksCfg,
    pub draw: DrawCfg,
    pub trail: TrailCfg,
    pub laser: LaserCfg,
    pub keys: KeysCfg,
    pub record: RecordCfg,
}

impl Default for ProfileSettings {
    fn default() -> Self {
        Config::default().snapshot()
    }
}

/// Noms des profils proposés d'origine (traduits à l'affichage).
pub const BUILTIN_PROFILES: [&str; 4] = ["Tuto", "Classe virtuelle", "Stream", "Démo logiciel"];

impl Config {
    /// Réglages actuels, pour créer ou mettre à jour un profil.
    pub fn snapshot(&self) -> ProfileSettings {
        ProfileSettings {
            enabled: self.enabled,
            show_banner: self.show_banner,
            spotlight: self.spotlight.clone(),
            ring: self.ring.clone(),
            clicks: self.clicks.clone(),
            draw: self.draw.clone(),
            trail: self.trail.clone(),
            laser: self.laser.clone(),
            keys: self.keys.clone(),
            record: self.record.clone(),
        }
    }

    /// Applique un profil (le dossier des vidéos choisi par l'utilisateur est conservé).
    pub fn apply_profile(&mut self, p: &Profile) {
        let s = &p.settings;
        let folder = std::mem::take(&mut self.record.folder);
        self.enabled = s.enabled;
        self.show_banner = s.show_banner;
        self.spotlight = s.spotlight.clone();
        self.ring = s.ring.clone();
        self.clicks = s.clicks.clone();
        self.draw = s.draw.clone();
        self.trail = s.trail.clone();
        self.laser = s.laser.clone();
        self.keys = s.keys.clone();
        self.record = s.record.clone();
        self.record.folder = folder;
        self.active_profile = p.name.clone();
        self.sanitize();
    }

    /// Profils proposés à la première utilisation.
    pub fn builtin_profiles() -> Vec<Profile> {
        let base = Config::default().snapshot();
        let mk = |name: &str, f: &dyn Fn(&mut ProfileSettings)| {
            let mut s = base.clone();
            f(&mut s);
            Profile { name: name.into(), settings: s }
        };
        vec![
            // Tuto : on voit les raccourcis et on entend les clics (vidéo sans image de la souris).
            mk("Tuto", &|s| {
                s.keys.enabled = true;
                s.clicks.sound = true;
                s.record.burn_effects = true;
            }),
            // Classe virtuelle : repérage du curseur pour les participants, rien de bruyant.
            mk("Classe virtuelle", &|s| {
                s.ring.pulse = true;
                s.clicks.sound = false;
                s.keys.enabled = false;
            }),
            // Stream : mouvements fluides et visibles, pas de bulles ni de touches.
            mk("Stream", &|s| {
                s.ring.enabled = false;
                s.trail.enabled = true;
                s.show_banner = false;
                s.keys.enabled = false;
                s.record.fps = 60;
            }),
            // Démo logiciel : spotlight sur la zone utile et raccourcis affichés.
            mk("Démo logiciel", &|s| {
                s.spotlight.mode = SpotlightMode::Circle;
                s.spotlight.shape = SpotlightMode::Circle;
                s.spotlight.dim = 0.45;
                s.keys.enabled = true;
            }),
        ]
    }
}

/// Où s'affichent les touches pressées (en bas de l'écran de la souris).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeysPosition {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeysCfg {
    pub enabled: bool,
    /// `false` : raccourcis seulement (Ctrl+C, Alt+Tab, F5…). `true` : toutes les touches,
    /// sauf dans les champs mot de passe.
    pub all_keys: bool,
    /// Afficher aussi les clics faits avec Ctrl, Alt, Maj ou Win (« Maj + Clic gauche »).
    pub mouse: bool,
    pub position: KeysPosition,
    /// Durée d'affichage.
    pub duration_ms: u32,
    /// Taille du texte (DIP).
    pub size: f32,
}

impl Default for KeysCfg {
    fn default() -> Self {
        KeysCfg { enabled: false, all_keys: false, mouse: true, position: KeysPosition::Center, duration_ms: 1600, size: 22.0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionCfg {
    /// Ranger captures, tableaux et vidéos d'une même session dans un seul dossier.
    pub enabled: bool,
    /// Dossier des sessions ; vide = Documents\Clicksy.
    pub folder: String,
    /// Formation en cours (« Excel niveau 1 ») : ses sessions sont rangées dans un sous-dossier à son nom.
    pub formation: String,
    /// Formations récentes, la plus récente en premier.
    pub formations: Vec<String>,
}

impl Default for SessionCfg {
    fn default() -> Self {
        SessionCfg { enabled: true, folder: String::new(), formation: String::new(), formations: Vec::new() }
    }
}

/// Nom utilisable comme nom de dossier Windows (caractères interdits retirés, 60 caractères au plus).
pub fn folder_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { ' ' } else { c })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let mut out: String = words.join(" ").chars().take(60).collect();
    // Windows refuse un nom qui finit par un point ou une espace.
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrawTool {
    Pen,
    Rect,
    Ellipse,
    Arrow,
}

impl DrawTool {
    pub fn label_fr(self) -> &'static str {
        match self {
            DrawTool::Pen => "Stylo",
            DrawTool::Rect => "Cadre",
            DrawTool::Ellipse => "Rond",
            DrawTool::Arrow => "Flèche",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DrawCfg {
    /// Couleurs sur les touches 1, 2 et 3.
    pub colors: [Color; 3],
    /// Épaisseur du trait (DIP) ; la molette la change pendant le dessin.
    pub width: f32,
    /// Outil au démarrage du mode dessin.
    pub default_tool: DrawTool,
    /// Effacer les dessins en quittant le mode dessin.
    pub clear_on_exit: bool,
    /// Enregistrer une image du tableau blanc/noir quand on l'efface ou le ferme.
    pub save_board: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TrailCfg {
    pub enabled: bool,
    pub color: Color,
    /// Durée de vie d'un point de la traînée.
    pub duration_ms: u32,
    pub width: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolbarCfg {
    pub visible: bool,
    /// Invisible dans les captures d'écran, enregistrements et partages (Teams, Zoom…).
    pub hide_from_capture: bool,
    /// Position du coin haut-gauche (pixels physiques) ; absent = bord droit de l'écran principal.
    pub x: Option<i32>,
    pub y: Option<i32>,
}

impl Default for ToolbarCfg {
    fn default() -> Self {
        ToolbarCfg { visible: true, hide_from_capture: true, x: None, y: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordCfg {
    pub system_audio: bool,
    pub mic: bool,
    /// 30 ou 60 images par seconde.
    pub fps: u32,
    /// Compte à rebours de 3 s avant de démarrer.
    pub countdown: bool,
    /// Dossier des enregistrements ; vide = Vidéos\Clicksy.
    pub folder: String,
    /// Ouvrir le dossier du projet à la fin.
    pub open_folder_after: bool,
    /// Incruster la souris et les effets Clicksy dans la vidéo (ce que voit le formateur).
    /// Sinon : vidéo « propre », effets à redessiner dans l'éditeur.
    pub burn_effects: bool,
}

impl Default for RecordCfg {
    fn default() -> Self {
        RecordCfg { system_audio: true, mic: true, fps: 30, countdown: true, folder: String::new(), open_folder_after: true, burn_effects: true }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureCfg {
    /// Dossier des captures ; vide = Images\Clicksy.
    pub folder: String,
    /// Copier aussi la capture dans le presse-papiers.
    pub copy_to_clipboard: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LaserCfg {
    pub color: Color,
    /// Rayon du point (DIP).
    pub size: f32,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SpotlightCfg {
    /// Affiché en ce moment : `Off`, ou la forme `shape`.
    pub mode: SpotlightMode,
    /// Forme choisie dans les réglages (cercle, bande horizontale ou verticale).
    pub shape: SpotlightMode,
    /// Rayon du cercle (DIP).
    pub radius: f32,
    /// Hauteur (bande horizontale) ou largeur (bande verticale), en DIP.
    pub band_size: f32,
    /// Assombrissement du reste de l'écran : 0 = aucun, 1 = noir total.
    pub dim: f32,
    /// Couleur du voile (son alpha est ignoré : c'est `dim` qui compte).
    pub dim_color: Color,
    /// Largeur du fondu sur le bord (DIP).
    pub feather: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RingCfg {
    pub enabled: bool,
    pub radius: f32,
    pub thickness: f32,
    pub color: Color,
    /// Opacité de l'anneau, 0..1.
    pub opacity: f32,
    pub pulse: bool,
    pub pulse_period_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClicksCfg {
    pub enabled: bool,
    pub left_color: Color,
    pub right_color: Color,
    pub middle_color: Color,
    /// Rayon final de l'onde (DIP).
    pub radius: f32,
    pub thickness: f32,
    pub duration_ms: u32,
    /// Pastille pleine affichée tant que le bouton est enfoncé.
    pub show_held: bool,
    pub sound: bool,
    /// Volume du son de clic, 0..1.
    pub sound_volume: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeysCfg {
    pub toggle_all: Option<Hotkey>,
    pub cycle_spotlight: Option<Hotkey>,
    pub toggle_ring: Option<Hotkey>,
    pub toggle_clicks: Option<Hotkey>,
    pub toggle_draw: Option<Hotkey>,
    pub toggle_laser: Option<Hotkey>,
    pub toggle_trail: Option<Hotkey>,
    pub toggle_toolbar: Option<Hotkey>,
    pub cycle_board: Option<Hotkey>,
    pub screenshot: Option<Hotkey>,
    pub toggle_record: Option<Hotkey>,
    pub toggle_keys: Option<Hotkey>,
    pub next_profile: Option<Hotkey>,
}

impl HotkeysCfg {
    /// Tous les raccourcis de Clicksy et l'action de chacun.
    pub fn table(&self) -> [(Option<Hotkey>, fx_bus::Action); 13] {
        use fx_bus::Action;
        [
            (self.toggle_all, Action::ToggleAll),
            (self.cycle_spotlight, Action::ToggleSpotlight),
            (self.toggle_ring, Action::ToggleRing),
            (self.toggle_clicks, Action::ToggleClicks),
            (self.toggle_draw, Action::ToggleDraw),
            (self.toggle_laser, Action::ToggleLaser),
            (self.toggle_trail, Action::ToggleTrail),
            (self.toggle_toolbar, Action::ToggleToolbar),
            (self.cycle_board, Action::CycleBoard),
            (self.screenshot, Action::Screenshot),
            (self.toggle_record, Action::ToggleRecord),
            (self.toggle_keys, Action::ToggleKeys),
            (self.next_profile, Action::NextProfile),
        ]
    }
}

impl Default for DrawCfg {
    fn default() -> Self {
        DrawCfg {
            colors: [
                Color::rgba8(0xFF, 0x3B, 0x30, 255),
                Color::rgba8(0xFF, 0xCC, 0x00, 255),
                Color::rgba8(0x1E, 0x90, 0xFF, 255),
            ],
            width: 5.0,
            default_tool: DrawTool::Pen,
            clear_on_exit: true,
            save_board: true,
        }
    }
}

impl Default for TrailCfg {
    fn default() -> Self {
        TrailCfg { enabled: false, color: Color::rgba8(0xFF, 0xC4, 0x00, 255), duration_ms: 350, width: 6.0 }
    }
}

impl Default for LaserCfg {
    fn default() -> Self {
        LaserCfg { color: Color::rgba8(0xFF, 0x22, 0x22, 255), size: 7.0, duration_ms: 600 }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: 1,
            enabled: true,
            spotlight: SpotlightCfg::default(),
            ring: RingCfg::default(),
            clicks: ClicksCfg::default(),
            draw: DrawCfg::default(),
            trail: TrailCfg::default(),
            laser: LaserCfg::default(),
            show_banner: true,
            toolbar: ToolbarCfg::default(),
            capture: CaptureCfg { folder: String::new(), copy_to_clipboard: true },
            record: RecordCfg::default(),
            keys: KeysCfg::default(),
            session: SessionCfg::default(),
            hotkeys: HotkeysCfg::default(),
            language: crate::i18n::Language::Auto,
            // Remplis par `sanitize` avec les profils proposés.
            profiles: Vec::new(),
            active_profile: String::new(),
        }
    }
}

impl Default for SpotlightCfg {
    fn default() -> Self {
        SpotlightCfg {
            mode: SpotlightMode::Off,
            shape: SpotlightMode::Circle,
            radius: 140.0,
            band_size: 160.0,
            dim: 0.55,
            dim_color: Color::rgba8(0, 0, 0, 255),
            feather: 24.0,
        }
    }
}

impl Default for RingCfg {
    fn default() -> Self {
        RingCfg {
            enabled: true,
            radius: 26.0,
            thickness: 4.0,
            color: Color::rgba8(0xFF, 0xC4, 0x00, 255),
            opacity: 0.9,
            pulse: false,
            pulse_period_ms: 1400,
        }
    }
}

impl Default for ClicksCfg {
    fn default() -> Self {
        ClicksCfg {
            enabled: true,
            left_color: Color::rgba8(0x1E, 0x90, 0xFF, 255),
            right_color: Color::rgba8(0xFF, 0x3B, 0x5C, 255),
            middle_color: Color::rgba8(0x2E, 0xCC, 0x71, 255),
            radius: 42.0,
            thickness: 4.0,
            duration_ms: 450,
            show_held: true,
            sound: false,
            sound_volume: 0.5,
        }
    }
}

impl Default for HotkeysCfg {
    fn default() -> Self {
        HotkeysCfg {
            toggle_all: Hotkey::parse("Ctrl+Alt+Maj+A"),
            cycle_spotlight: Hotkey::parse("Ctrl+Alt+Maj+S"),
            toggle_ring: Hotkey::parse("Ctrl+Alt+Maj+R"),
            toggle_clicks: Hotkey::parse("Ctrl+Alt+Maj+C"),
            toggle_draw: Hotkey::parse("Ctrl+Alt+Maj+D"),
            toggle_laser: Hotkey::parse("Ctrl+Alt+Maj+L"),
            toggle_trail: Hotkey::parse("Ctrl+Alt+Maj+T"),
            toggle_toolbar: Hotkey::parse("Ctrl+Alt+Maj+O"),
            cycle_board: Hotkey::parse("Ctrl+Alt+Maj+W"),
            screenshot: Hotkey::parse("Ctrl+Alt+Maj+P"),
            toggle_record: Hotkey::parse("Ctrl+Alt+Maj+E"),
            toggle_keys: Hotkey::parse("Ctrl+Alt+Maj+K"),
            next_profile: Hotkey::parse("Ctrl+Alt+Maj+Y"),
        }
    }
}

impl Config {
    /// Ramène les valeurs saisies à la main dans des bornes raisonnables.
    pub fn sanitize(&mut self) {
        let s = &mut self.spotlight;
        // La forme n'est jamais « désactivé » ; un spotlight allumé utilise toujours la forme choisie.
        if s.shape == SpotlightMode::Off {
            s.shape = SpotlightMode::Circle;
        }
        if s.mode != SpotlightMode::Off {
            s.shape = s.mode;
        }
        s.radius = s.radius.clamp(20.0, 2000.0);
        s.band_size = s.band_size.clamp(20.0, 2000.0);
        s.dim = s.dim.clamp(0.0, 1.0);
        s.feather = s.feather.clamp(0.0, 400.0);
        let r = &mut self.ring;
        r.radius = r.radius.clamp(4.0, 400.0);
        r.thickness = r.thickness.clamp(1.0, 60.0);
        r.opacity = r.opacity.clamp(0.0, 1.0);
        r.pulse_period_ms = r.pulse_period_ms.clamp(200, 10_000);
        let c = &mut self.clicks;
        c.radius = c.radius.clamp(4.0, 400.0);
        c.thickness = c.thickness.clamp(1.0, 60.0);
        c.duration_ms = c.duration_ms.clamp(50, 5000);
        c.sound_volume = c.sound_volume.clamp(0.0, 1.0);
        self.draw.width = self.draw.width.clamp(1.0, 40.0);
        let t = &mut self.trail;
        t.duration_ms = t.duration_ms.clamp(50, 3000);
        t.width = t.width.clamp(1.0, 40.0);
        let l = &mut self.laser;
        l.size = l.size.clamp(2.0, 40.0);
        l.duration_ms = l.duration_ms.clamp(50, 3000);
        // Profils : noms non vides et uniques ; sans aucun profil, on repropose ceux d'origine.
        let mut seen = std::collections::HashSet::new();
        self.profiles.retain_mut(|p| {
            p.name = p.name.trim().chars().take(40).collect();
            !p.name.is_empty() && seen.insert(p.name.to_lowercase())
        });
        if self.profiles.is_empty() {
            self.profiles = Config::builtin_profiles();
        }
        for p in &mut self.profiles {
            p.settings.spotlight.radius = p.settings.spotlight.radius.clamp(20.0, 2000.0);
            p.settings.keys.duration_ms = p.settings.keys.duration_ms.clamp(500, 6000);
            p.settings.keys.size = p.settings.keys.size.clamp(12.0, 48.0);
        }
        if !self.profiles.iter().any(|p| p.name == self.active_profile) {
            self.active_profile.clear();
        }
        // Formation : nom de dossier valide, et liste des récentes sans doublon (10 au plus).
        let s = &mut self.session;
        s.formation = folder_name(&s.formation);
        let mut seen = std::collections::HashSet::new();
        s.formations = s.formations.iter().map(|f| folder_name(f)).filter(|f| !f.is_empty() && seen.insert(f.to_lowercase())).take(10).collect();
        self.keys.duration_ms = self.keys.duration_ms.clamp(500, 6000);
        self.keys.size = self.keys.size.clamp(12.0, 48.0);
        self.record.fps = if self.record.fps >= 45 { 60 } else { 30 };
    }
}
