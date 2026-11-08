# Clicksy

Effets de pointeur pour formateurs sous Windows 10/11 : spotlight, anneau autour du curseur,
effets de clic, traînée, pointeur laser et dessin à l'écran, pour les tutos e-learning, les démos
de logiciels et les classes virtuelles (Teams, Zoom, Meet).

**Aucun appel réseau, aucune télémétrie, aucune touche enregistrée.** Interface en français.

> Version 0.2.0 : barre d'outils flottante, tableau blanc / noir, capture d'écran. L'enregistrement vidéo avec éditeur est la prochaine étape.

## Installer

Deux versions, à télécharger dans l'onglet **Releases** du dépôt :

| Version | Pour qui | Droits admin |
|---|---|---|
| `Clicksy_x.y.z_x64_fr-FR.msi` | PC d'entreprise, déploiement Intune / SCCM | Oui, à l'installation seulement |
| `Clicksy-x.y.z-portable.zip` | Test rapide, clé USB, PC verrouillé | Non |

- **MSI** : installe dans `C:\Program Files\Clicksy`, ajoute Clicksy au menu Démarrer et se désinstalle
  depuis Paramètres › Applications. Les réglages vont dans `%APPDATA%\Clicksy`.
  Installation silencieuse : `msiexec /i Clicksy_0.2.0_x64_fr-FR.msi /qn`.
- **Portable** : dézipper puis lancer `clicksy.exe`. Les réglages restent dans le dossier `data\` à côté.

Windows peut afficher « Windows a protégé votre ordinateur » : les binaires ne sont **pas encore signés**
(voir [Signature](#signature)). Cliquer sur *Informations complémentaires › Exécuter quand même*.

WebView2 (fenêtre de réglages) est présent d'office sur Windows 11 et sur la plupart des Windows 10 ;
s'il manque, l'installeur MSI le récupère auprès de Microsoft.

## Utilisation

L'icône (un anneau jaune) apparaît dans la zone de notification, à côté de l'horloge.
**Clic gauche : réglages. Clic droit : menu rapide.** Relancer Clicksy alors qu'il tourne ouvre les réglages.

### Raccourcis par défaut

| Raccourci | Action |
|---|---|
| Ctrl + Alt + Maj + A | Tout activer / désactiver |
| Ctrl + Alt + Maj + S | Spotlight allumé / éteint (forme choisie dans Réglages › Spotlight) |
| Ctrl + Alt + Maj + R | Anneau autour du curseur |
| Ctrl + Alt + Maj + C | Effets de clic |
| Ctrl + Alt + Maj + T | Traînée derrière le curseur |
| Ctrl + Alt + Maj + L | Pointeur laser |
| Ctrl + Alt + Maj + D | Mode dessin |
| Ctrl + Alt + Maj + W | Tableau : blanc → noir → fermé |
| Ctrl + Alt + Maj + P | Capture d'écran (ImagesClicksy + presse-papiers) |
| Ctrl + Alt + Maj + O | Afficher / masquer la barre d'outils |

Tous se modifient dans **Réglages › Raccourcis**. Clicksy signale les doublons, les raccourcis déjà pris
par Teams, Office ou Windows, et les combinaisons qui correspondent à AltGr sur un clavier AZERTY.
Une bulle de confirmation (« Spotlight : cercle »…) s'affiche brièvement en haut de l'écran.

### Barre d'outils

Une barre flottante (bord droit par défaut) donne accès à tout sans raccourci : Souris, Stylo, Cadre,
Rond, Flèche, épaisseur du trait, 3 couleurs, Annuler, Effacer, tableau Blanc, tableau Noir, Spot, Laser,
Capture, Réglages, et un petit bouton « réduire » dans la poignée. Les réglages (forme du spotlight, couleurs…)
se choisissent dans la fenêtre Réglages ; la barre sert à les appliquer.

- **Ne prend jamais le focus** : cliquer dessus ne fait pas sortir PowerPoint du diaporama.
- **Déplaçable** par sa poignée (les six points en haut) ; elle s'aimante aux bords gauche/droit et retient sa place.
- **Invisible pour les participants** (option active par défaut) : absente des partages Teams/Zoom/Meet,
  des captures et des enregistrements (Windows 10 version 2004 ou plus récent).
- Les boutons **Blanc** et **Noir** ouvrent le tableau sur l'écran de la souris (re-cliquer le ferme) ; les dessins faits dessus
  sont séparés de ceux de l'écran. « Souris » ferme le tableau et le mode dessin.

### Mode dessin

Pendant le mode dessin, les clics servent à dessiner et **ne vont plus à l'application en dessous**.

| Touche | Effet |
|---|---|
| P · R · E · F | Stylo · Cadre · Rond · Flèche |
| Clic droit | Outil suivant |
| 1 · 2 · 3 | Couleurs (réglables) |
| Molette | Épaisseur du trait |
| Maj (pendant le tracé) | Carré, cercle, flèche alignée à 45° |
| Ctrl + Z | Annuler la dernière forme |
| Suppr | Tout effacer |
| Échap | Tout effacer ; s'il n'y a plus rien, quitter le mode dessin |

### Ligne de commande (Stream Deck, scripts)

Avec Clicksy déjà lancé, `clicksy.exe --action <nom>` déclenche une action :
`tout`, `spotlight`, `anneau`, `clics`, `trainee`, `laser`, `dessin`, `pulsation`, `son`,
`tableau`, `capture`, `barre`.
Pratique pour un bouton Stream Deck (action « Ouvrir » avec ces arguments).

## Test manuel

| # | Étapes | Résultat attendu |
|---|---|---|
| 1 | Lancer Clicksy | L'icône apparaît en moins de 2 s ; l'anneau jaune suit le curseur |
| 2 | Cliquer gauche, puis droit | Onde bleue (gauche) ou rouge (droit) ; le clic **atteint bien l'app en dessous** |
| 3 | Maintenir le bouton gauche | Pastille tant que le bouton est enfoncé ; le glisser-déposer fonctionne |
| 4 | Réglages › Spotlight › Forme « Bande horizontale », puis Ctrl + Alt + Maj + S deux fois | Bande allumée puis éteinte ; bulle de confirmation à chaque fois |
| 5 | Taper dans Word ou Notepad pendant le test 4 | La lettre S n'est jamais tapée ; les raccourcis Office/Teams marchent toujours |
| 6 | Menu › Pulsation de l'anneau | L'anneau « respire » doucement |
| 7 | Menu › Son de clic, puis cliquer | Petit « clic » sec à chaque appui |
| 8 | PowerPoint › diaporama (F5) | Les effets restent visibles par-dessus le diaporama |
| 9 | Teams › partager **l'écran entier** | Les participants voient les effets |
| 10 | 2 écrans à 100 % et 150 % — *non testé, faute de 2ᵉ écran* | Effets sur les deux écrans, même taille apparente |
| 11 | Changer l'échelle (Paramètres › Affichage) | La surcouche se recrée ; les effets gardent leur taille apparente |
| 12 | Gestionnaire des tâches, souris immobile | CPU ≈ 0 % (mesuré : 0,3 à 0,65 %) |
| 13 | Clic gauche sur l'icône | Réglages ouverts en moins d'une seconde, thème clair ou sombre comme Windows |
| 14 | Réglages › Spotlight › Cercle, glisser « Taille du cercle » | Le cercle change **pendant** le glissement |
| 15 | Réglages › Raccourcis, taper Ctrl + Maj + M | Avertissement « déjà utilisé par Teams (micro) » |
| 16 | Ctrl + Alt + Maj + T, bouger la souris | Une traînée suit le curseur puis s'efface |
| 17 | Ctrl + Alt + Maj + L | Point rouge lumineux avec traînée ; l'anneau disparaît. Rappuyer : retour normal |
| 18 | Ctrl + Alt + Maj + D, puis dessiner avec P, R, E, F | Stylo lissé, cadre, rond, flèche ; les clics ne vont pas à l'app dessous |
| 19 | En mode dessin : 2, molette, Maj pendant un cadre | Couleur 2, trait plus épais, carré parfait |
| 20 | En mode dessin : Ctrl + Z, puis Échap deux fois | Dernière forme retirée ; tout effacé ; mode dessin quitté |
| 21 | Fermer les réglages, quitter et relancer | Effets actifs après fermeture ; réglages conservés |
| 22 | `clicksy.exe --action laser` | Le laser s'allume dans l'instance déjà lancée |
| 23 | MSI : installer, lancer depuis le menu Démarrer, désinstaller | Installation dans Program Files ; désinstallation propre ; réglages dans `%APPDATA%\Clicksy` |
| 24 | Barre d'outils : Stylo, dessiner, puis Souris | Le dessin marche ; « Souris » rend les clics aux applis |
| 25 | PowerPoint en diaporama, cliquer sur des boutons de la barre | Le diaporama reste au premier plan et répond toujours aux flèches du clavier |
| 26 | Glisser la barre par sa poignée vers le bord gauche, relancer Clicksy | Elle s'aimante au bord gauche et y revient au redémarrage |
| 27 | Teams : partager l'écran entier, regarder depuis un 2ᵉ appareil | Les effets sont visibles, la barre d'outils **ne l'est pas** |
| 28 | Bouton Blanc, dessiner, bouton Noir, re-cliquer Noir | Blanc puis noir plein écran (mêmes dessins) ; fermé au 2ᵉ clic, les dessins de l'écran reviennent |
| 29 | Bouton Capture | Bulle « Capture enregistrée et copiée » ; fichier PNG dans ImagesClicksy ; collable dans Word ou Teams |

## Limites Windows connues

- **Applis lancées en administrateur** : l'anneau et le spotlight suivent la souris, mais les clics,
  les raccourcis et le dessin ne fonctionnent pas tant qu'une app admin a le focus (isolation UIPI).
  La version MSI **signée** avec `uiAccess` lèvera cette limite.
- **Menu Démarrer, Alt+Tab, centre de notifications** : peuvent passer au-dessus des effets (même raison).
- **Écran UAC, Ctrl+Alt+Suppr, écran de verrouillage** : aucun effet possible (bureau sécurisé).
- **Jeux en plein écran exclusif** : surcouche invisible (les jeux en fenêtre sans bordure fonctionnent).
- **Partage d'une seule fenêtre** dans Teams/Zoom : les effets ne sont pas transmis (partager l'écran entier).

## Signature

Les binaires ne sont pas encore signés. Pistes, par ordre de préférence pour des PC d'entreprise :
PKI interne de l'entreprise (déploiement du certificat par GPO), signature gratuite
[SignPath Foundation](https://signpath.org) pour les projets open source, certificat Certum Open Source.
Une fois signé, le MSI pourra activer `uiAccess` (premier plan au-dessus des apps admin).

## Compiler

Prérequis : Rust (toolchain `stable-x86_64-pc-windows-msvc`) et les Build Tools Visual Studio (C++).

```bash
cargo build --release
```

`target/release/clicksy.exe` est la version portable. Pour le MSI :

```bash
cargo install tauri-cli --version "^2" --locked
cd app && cargo tauri build
```

Le MSI sort dans `target/release/bundle/msi/`. Le workflow GitHub Actions (`.github/workflows/release.yml`)
produit les deux versions et les publie à chaque tag `v*`.

## Architecture

```
app/              exe : icône de notification, moteur, réglages (Tauri v2), ligne de commande
ui/               interface des réglages (HTML/CSS/JS, sans étape de build, polices système)
crates/fx-bus     bus d'événements typé et horodaté (base du futur journal JSON)
crates/fx-core    config JSON, raccourcis, chemins portables, état partagé (pur Rust)
crates/fx-input   hooks WH_MOUSE_LL / WH_KEYBOARD_LL sur un thread dédié
crates/fx-overlay une fenêtre par écran, rendu D3D11 + Direct2D + DirectComposition + DirectWrite
crates/fx-audio   son de clic synthétisé en mémoire
```

Threads : principal (Tauri), icône, hooks, moteur, rendu. Le rendu dort tant que rien ne change
et ne redessine que les écrans concernés, au plus une fois par rafraîchissement.

## Licence

MIT. Projet indépendant, sans lien avec d'autres outils d'effets de pointeur.
