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
    pub hotkeys: HotkeysCfg,
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
            hotkeys: HotkeysCfg::default(),
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
        self.record.fps = if self.record.fps >= 45 { 60 } else { 30 };
    }
}
