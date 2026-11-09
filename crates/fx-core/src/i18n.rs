//! Langue de l'interface native (barre d'outils, menus, bulles, touches affichées).
//!
//! Le texte français sert de clé : `t("Souris normale")` renvoie « Normal mouse » en anglais.
//! Un texte absent de la table reste en français. La fenêtre de réglages a sa propre table (ui/i18n.js).

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Language {
    /// Langue de Windows : français si Windows est en français, anglais sinon.
    #[default]
    Auto,
    Fr,
    En,
}

static ENGLISH: AtomicBool = AtomicBool::new(false);
static SYSTEM_FRENCH: AtomicBool = AtomicBool::new(true);

/// À appeler au démarrage : Windows est-il en français ?
pub fn set_system_french(fr: bool) {
    SYSTEM_FRENCH.store(fr, Ordering::Release);
}

pub fn apply(lang: Language) {
    let en = match lang {
        Language::Auto => !SYSTEM_FRENCH.load(Ordering::Acquire),
        Language::Fr => false,
        Language::En => true,
    };
    ENGLISH.store(en, Ordering::Release);
}

pub fn english() -> bool {
    ENGLISH.load(Ordering::Acquire)
}

/// Code court de la langue active (« fr » ou « en »).
pub fn code() -> &'static str {
    if english() {
        "en"
    } else {
        "fr"
    }
}

/// Traduit un texte de l'interface.
pub fn t(fr: &'static str) -> &'static str {
    if !english() {
        return fr;
    }
    EN.iter().find(|(f, _)| *f == fr).map(|(_, e)| *e).unwrap_or(fr)
}

/// Traduit un modèle puis remplace chaque `{}` par l'argument suivant.
pub fn tf(fr: &'static str, args: &[&str]) -> String {
    let mut out = String::new();
    let mut it = args.iter();
    let mut rest = t(fr);
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        out.push_str(it.next().copied().unwrap_or(""));
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

/// « activé » / « désactivé ».
pub fn on_off(on: bool) -> &'static str {
    t(if on { "activé" } else { "désactivé" })
}

/// Libellé d'un raccourci pour l'affichage (« Ctrl+Alt+Maj+K » → « Ctrl+Alt+Shift+K » en anglais).
pub fn hotkey_label(s: &str) -> String {
    if !english() {
        return s.to_string();
    }
    s.split('+')
        .map(|p| match p {
            "Maj" => "Shift",
            "Échap" => "Esc",
            "Espace" => "Space",
            "Entrée" => "Enter",
            "Inser" => "Insert",
            "Suppr" => "Delete",
            "Début" => "Home",
            "Fin" => "End",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Français → anglais.
static EN: &[(&str, &str)] = &[
    // États
    ("activé", "on"),
    ("désactivé", "off"),
    ("activée", "on"),
    ("désactivée", "off"),
    ("affichée", "shown"),
    ("masquée", "hidden"),
    // Spotlight et outils
    ("Désactivé", "Off"),
    ("Cercle", "Circle"),
    ("Bande horizontale", "Horizontal band"),
    ("Bande verticale", "Vertical band"),
    ("Stylo", "Pen"),
    ("Cadre", "Box"),
    ("Rond", "Circle"),
    ("Flèche", "Arrow"),
    // Barre d'outils
    ("Souris", "Mouse"),
    ("Trait", "Width"),
    ("Annuler", "Undo"),
    ("Effacer", "Clear"),
    ("Blanc", "White"),
    ("Noir", "Black"),
    ("Vidéo", "Video"),
    ("Capture", "Capture"),
    ("Laser", "Laser"),
    ("Spot", "Spot"),
    ("Touches", "Keys"),
    ("Profil", "Profile"),
    ("Réglages", "Settings"),
    ("Masquer", "Hide"),
    ("Enregistrer l'écran entier", "Record the whole screen"),
    ("Enregistrer une fenêtre…", "Record a window…"),
    ("Enregistrer une zone…", "Record an area…"),
    ("Son du PC", "PC sound"),
    ("Micro", "Microphone"),
    ("Ouvrir le dossier de la session", "Open the session folder"),
    ("Ouvrir le dossier des vidéos", "Open the videos folder"),
    // Bulles
    ("Épaisseur : {}", "Width: {}"),
    ("Outil : {}", "Tool: {}"),
    ("Couleur {}", "Colour {}"),
    ("Annulé", "Undone"),
    ("Dessins effacés", "Drawings cleared"),
    ("Tableau enregistré puis effacé", "Board saved, then cleared"),
    ("Image du tableau impossible : {}", "Could not save the board: {}"),
    (
        "Mode dessin · 1 2 3 couleurs · P stylo · R cadre · E rond · F flèche · Échap efface",
        "Draw mode · 1 2 3 colours · P pen · R box · E circle · F arrow · Esc clears",
    ),
    ("Souris normale · image du tableau enregistrée", "Normal mouse · board image saved"),
    ("Souris normale", "Normal mouse"),
    ("Tableau blanc · dessine directement · Échap efface", "Whiteboard · draw right away · Esc clears"),
    ("Tableau noir", "Blackboard"),
    ("Tableau fermé · image enregistrée", "Board closed · image saved"),
    ("Tableau fermé", "Board closed"),
    ("Pointeur laser {}", "Laser pointer {}"),
    ("Clicksy : effets activés", "Clicksy: effects on"),
    ("Clicksy : effets en pause", "Clicksy: effects paused"),
    ("Spotlight : {}", "Spotlight: {}"),
    ("Anneau {}", "Ring {}"),
    ("Pulsation {}", "Pulse {}"),
    ("Effets de clic {}", "Click effects {}"),
    ("Son de clic {}", "Click sound {}"),
    ("Traînée {}", "Trail {}"),
    ("Barre d'outils {}", "Toolbar {}"),
    ("Affichage des touches {}", "Key display {}"),
    ("Profil : {}", "Profile: {}"),
    ("Formation : {}", "Course: {}"),
    ("Aucune formation", "No course"),
    ("Capture enregistrée : {}", "Screenshot saved: {}"),
    ("Capture enregistrée et copiée : {}", "Screenshot saved and copied: {}"),
    ("Capture impossible : {}", "Screenshot failed: {}"),
    ("Enregistrement impossible : {}", "Could not start recording: {}"),
    ("Enregistrement annulé", "Recording cancelled"),
    ("Enregistrement terminé ({})", "Recording finished ({})"),
    ("Enregistrement incomplet : {}", "Recording incomplete: {}"),
    ("Clique sur la fenêtre à enregistrer · Échap annule", "Click the window to record · Esc cancels"),
    ("Trace la zone à enregistrer · Échap annule", "Drag the area to record · Esc cancels"),
    ("Zone trop petite : enregistrement annulé", "Area too small: recording cancelled"),
    (
        "Nouvelle session : les prochains éléments iront dans un nouveau dossier",
        "New session: the next items will go to a new folder",
    ),
    // Noms de fichiers et de dossiers
    ("Tableau", "Board"),
    ("Enregistrement", "Recording"),
    // Touches
    ("Saisie masquée", "Hidden input"),
    ("Clic gauche", "Left click"),
    ("Clic droit", "Right click"),
    ("Clic molette", "Middle click"),
    ("Bouton précédent", "Back button"),
    ("Bouton suivant", "Forward button"),
    ("Molette ↑", "Wheel ↑"),
    ("Molette ↓", "Wheel ↓"),
    ("Maj", "Shift"),
    ("Retour arrière", "Backspace"),
    ("Entrée", "Enter"),
    ("Verr. Maj", "Caps Lock"),
    ("Échap", "Esc"),
    ("Espace", "Space"),
    ("Page préc.", "Page Up"),
    ("Page suiv.", "Page Down"),
    ("Fin", "End"),
    ("Début", "Home"),
    ("Impr. écran", "Print Screen"),
    ("Inser", "Insert"),
    ("Suppr", "Delete"),
    ("Verr. Num", "Num Lock"),
    ("Arrêt défil", "Scroll Lock"),
    ("Muet", "Mute"),
    ("Piste suivante", "Next track"),
    ("Piste précédente", "Previous track"),
    ("Lecture/Pause", "Play/Pause"),
    ("Touche {}", "Key {}"),
    // Icône de notification
    ("Clicksy : actif (spotlight : {})", "Clicksy: active (spotlight: {})"),
    ("Clicksy : en pause", "Clicksy: paused"),
    ("Réglages…", "Settings…"),
    ("Effets activés", "Effects on"),
    ("Spotlight", "Spotlight"),
    ("Anneau autour du curseur", "Ring around the cursor"),
    ("Pulsation de l'anneau", "Ring pulse"),
    ("Effets de clic", "Click effects"),
    ("Son de clic", "Click sound"),
    ("Traînée du curseur", "Cursor trail"),
    ("Afficher les touches", "Show keys"),
    ("Pointeur laser", "Laser pointer"),
    ("Mode dessin", "Draw mode"),
    ("Tableau blanc / noir", "White / black board"),
    ("Capture d'écran", "Screenshot"),
    ("Arrêter l'enregistrement", "Stop recording"),
    ("Enregistrer l'écran", "Record the screen"),
    ("Barre d'outils", "Toolbar"),
    ("Profils", "Profiles"),
    ("Gérer les profils…", "Manage profiles…"),
    ("Clicksy – Éditeur vidéo", "Clicksy – Video editor"),
    ("Un export est déjà en cours", "An export is already running"),
    ("Vidéo finale", "Final video"),
    ("Ouvrir l'éditeur vidéo…", "Open the video editor…"),
    ("Éditeur vidéo…", "Video editor…"),
    ("Guide", "Guide"),
    ("Guide pas à pas", "Step-by-step guide"),
    ("Guide impossible : {}", "Could not create the guide: {}"),
    ("Guide pas à pas : chaque clic devient une étape · Guide pour terminer", "Step-by-step guide: every click becomes a step · Guide to finish"),
    ("Guide : étape {}", "Guide: step {}"),
    ("Appuyer sur {}", "Press {}"),
    ("Saisir le texte voulu", "Type the required text"),
    ("étape", "step"),
    ("Étape", "Step"),
    ("Faire un clic droit à l'endroit indiqué", "Right-click where shown"),
    ("Double-cliquer à l'endroit indiqué", "Double-click where shown"),
    ("Cliquer à l'endroit indiqué", "Click where shown"),
    ("dans « {} »", "in “{}”"),
    ("Faire un clic droit sur « {} »", "Right-click “{}”"),
    ("Double-cliquer sur « {} »", "Double-click “{}”"),
    ("Cliquer sur le bouton « {} »", "Click the “{}” button"),
    ("Cliquer sur le menu « {} »", "Click the “{}” menu"),
    ("Cliquer sur l'onglet « {} »", "Click the “{}” tab"),
    ("Cliquer sur le lien « {} »", "Click the “{}” link"),
    ("Cocher la case « {} »", "Tick the “{}” box"),
    ("Choisir l'option « {} »", "Choose the “{}” option"),
    ("Cliquer dans le champ « {} »", "Click in the “{}” field"),
    ("Ouvrir la liste « {} »", "Open the “{}” list"),
    ("Sélectionner « {} »", "Select “{}”"),
    ("Cliquer sur « {} »", "Click “{}”"),
    ("Guide annulé : aucune étape", "Guide cancelled: no steps"),
    ("Guide terminé : {} étapes", "Guide finished: {} steps"),
    ("Imprimer ou enregistrer en PDF", "Print or save as PDF"),
    ("Tu peux modifier les textes avant d'imprimer. Clique sur une image pour voir l'écran entier.", "You can edit the texts before printing. Click a picture to see the whole screen."),
    ("Créé avec Clicksy le", "Created with Clicksy on"),
    ("Formation", "Course"),
    ("Nouvelle session", "New session"),
    ("Ouvrir le dossier de configuration", "Open the settings folder"),
    ("Recharger la configuration", "Reload settings"),
    ("Quitter Clicksy", "Quit Clicksy"),
    ("Clicksy – Réglages", "Clicksy – Settings"),
    // Profils proposés
    ("Tuto", "Tutorial"),
    ("Classe virtuelle", "Virtual class"),
    ("Stream", "Stream"),
    ("Démo logiciel", "Software demo"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traduction() {
        apply(Language::En);
        assert_eq!(t("Souris normale"), "Normal mouse");
        assert_eq!(tf("Outil : {}", &["Pen"]), "Tool: Pen");
        assert_eq!(t("texte inconnu"), "texte inconnu");
        assert_eq!(hotkey_label("Ctrl+Alt+Maj+K"), "Ctrl+Alt+Shift+K");
        apply(Language::Fr);
        assert_eq!(tf("Outil : {}", &["Stylo"]), "Outil : Stylo");
        // Pas de doublon de clé dans la table.
        let mut keys: Vec<_> = EN.iter().map(|(f, _)| *f).collect();
        keys.sort();
        let n = keys.len();
        keys.dedup();
        assert_eq!(n, keys.len());
    }
}

/// Nom d'un profil à l'affichage : les profils proposés d'origine sont traduits.
pub fn profile_name(name: &str) -> String {
    match crate::config::BUILTIN_PROFILES.iter().find(|b| **b == name) {
        Some(b) => t(b).to_string(),
        None => name.to_string(),
    }
}
