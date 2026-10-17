//! Menus: title screen, pause, checkpoint (travel, equipment), settings, help.
//!
//! Navigation: up/down to choose, left/right to change a value, Enter / (A)
//! to confirm, Esc / (B) to go back. Esc / Start opens the pause menu, Start closes it.
//! The mouse works too: hovering a line selects it (only if the mouse moves,
//! so as not to steal the selection when a page opens under the cursor), click to confirm,
//! click on ‹ / › to change the value.
//!
//! Actions run with full access to the world (`handle`), which allows starting
//! a game, saving or applying equipment directly.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::text::LineBreak;
use bevy::window::{CursorGrabMode, CursorOptions, Monitor, PrimaryMonitor, PrimaryWindow};

use crate::fx::FxState;
use crate::input::Device;
use crate::lang::{Lang, tr};
use crate::render::camera::CameraRig;
use crate::render::{AppState, LocalPlayer};
use crate::save::{SaveData, SaveSlot};
use crate::settings::{
    DisplayMode, INTERNAL_HEIGHTS, MODERN_HEIGHT, PS1_HEIGHT, Settings, exclusive_sizes, refresh_rates,
    windowed_sizes,
};
use crate::sim::data::Tuning;
use crate::sim::encounter::{Encounter, Progress, SimCommand, SimCommands, near_checkpoint};
use crate::sim::fighter::Body;
use crate::sim::items::{Item, Kind, QUICK_SLOTS};
use crate::sim::player::Player;
use crate::sim::{ResetFight, SimEntity, SimEvent, SimEvents};
use crate::render::preview::{CheckpointPreview, PREVIEW_SIZE};
use crate::hud::{GEM_ICON, ITEM_ICON, ItemIcons, MenuIcons, WeaponIcons};
use crate::ui::{Glyph, Hint, Icons, PixelSize, PixelText, Seg, UiFont, hint_node, i, icon_bundle, image_bundle, set_hint, t};

/// The project's repository ("Fork me" badge of the title screen).
pub const REPO_URL: &str = "https://github.com/wormsparty/retro-souls";
/// Longest co-op password.
const PASSWORD_MAX: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Title,
    ConfirmNew,
    Pause,
    Checkpoint,
    /// Fast travel between checkpoints (with a preview of the place).
    Travel,
    /// Rekindle the torch of a defeated boss (`MenuState::reviving`): it will await again.
    Revive,
    Equipment,
    /// Character sheet: HP, stamina, attack…
    Status,
    /// System: settings, help, back to the title screen, quit.
    System,
    /// Choice of the item for a slot (`MenuState::choosing`) among those owned.
    Choose,
    Options,
    Help,
    /// Online co-op, from a brazier: optional password, then the search (`true`: as the host,
    /// otherwise to join one).
    Online(bool),
    /// Language choice on first launch.
    Language,
    /// Graphics style choice (PS1 or modern) on first launch.
    Style,
}

#[derive(Resource)]
pub struct MenuState {
    pub open: bool,
    page: Page,
    /// Previous pages (and selected line), to go back.
    stack: Vec<(Page, usize)>,
    selected: usize,
    /// Navigation repeat while the stick/D-pad is held.
    repeat: f32,
    /// Opened this frame: inputs are ignored (the button that opened it, for example (A)
    /// to rest, must not also confirm the first line).
    fresh: bool,
    /// Last known cursor position (hovering only counts if it has moved).
    cursor: Option<Vec2>,
    /// Time during which cursor movements are ignored after opening (it can
    /// jump when it's released).
    settle: f32,
    /// Slot being chosen (`Choose` page): 0..QUICK_SLOTS, or QUICK_SLOTS for the talisman.
    choosing: u8,
    /// Boss whose torch is being rekindled (`Revive` page).
    reviving: u8,
    /// Co-op password being typed (`Online` page).
    password: String,
}

impl Default for MenuState {
    fn default() -> Self {
        Self { open: false, page: Page::Pause, stack: Vec::new(), selected: 0, repeat: 0.0, fresh: false, cursor: None, settle: 0.0, choosing: 0, reviving: 0, password: String::new() }
    }
}

impl MenuState {
    pub fn on_title(&self) -> bool {
        self.open && matches!(self.page, Page::Title | Page::Language | Page::Style)
    }

    /// Opens the menu on a page (without touching the cursor).
    pub fn open(&mut self, page: Page) {
        self.open = true;
        self.fresh = true;
        self.settle = 0.25;
        self.page = page;
        self.stack.clear();
        self.selected = 0;
    }

    fn push(&mut self, page: Page) {
        self.stack.push((self.page, self.selected));
        self.page = page;
        self.selected = 0;
    }

    /// Goes back to the previous page; false if there is none.
    fn back(&mut self) -> bool {
        match self.stack.pop() {
            Some((p, s)) => {
                self.page = p;
                self.selected = s;
                true
            }
            None => false,
        }
    }
}

pub fn menu_closed(menu: Res<MenuState>) -> bool {
    !menu.open
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Leave,
    NewGame,
    ConfirmNew,
    Load,
    Open(Page),
    Back,
    ToTitle,
    Quit,
    /// At a brazier: host a co-op game, join one (the password page).
    OnlineHost,
    OnlineJoin,
    /// Starts the search (password page).
    Search,
    /// Ends the co-op: the host stays, the guest goes back to their world.
    LeaveCoop,
}

/// Setting option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Opt {
    Display,
    Resolution,
    Refresh,
    VSync,
    Internal,
    Master,
    Effects,
    Sensitivity,
    InvertY,
    Shake,
    ShowFps,
    Language,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Entry {
    Act(Act),
    Opt(Opt),
    /// Item quick slot.
    Slot(u8),
    /// Worn talisman.
    Talisman,
    /// Weapon (index in `Tuning::weapons`): shown in the equipment, weapons are switched in game.
    Weapon(u8),
    /// Item offered for the slot being chosen (`None`: empty it).
    Pick(Option<Item>),
    /// Travel destination (checkpoint index).
    Place(u8),
    /// Defeated boss (index in `Tuning::encounters`), to revive.
    Boss(u8),
    /// Help line (index in `help_lines`), not selectable.
    Line(u8),
    /// Language offered (language choice page).
    Lang(Lang),
    /// Graphics style offered: its internal resolution.
    Style(u32),
    /// Line of the character sheet, not selectable.
    Stat(Stat),
    /// Co-op password (typed on the keyboard).
    Password,
}

/// Line of the character sheet (status page).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stat {
    Hp,
    Stamina,
    Attack,
    Defense,
    Flasks,
}

impl Stat {
    const ALL: [Stat; 5] = [Stat::Hp, Stat::Stamina, Stat::Attack, Stat::Defense, Stat::Flasks];

    fn label(self) -> &'static str {
        match self {
            Stat::Hp => tr("Hit points", "Points de vie"),
            Stat::Stamina => tr("Stamina", "Endurance"),
            Stat::Attack => tr("Attack (weapon in hand)", "Attaque (arme en main)"),
            Stat::Defense => tr("Damage reduction", "Réduction des dégâts"),
            Stat::Flasks => tr("Healing flasks", "Fioles de soin"),
        }
    }

    fn value(self, p: &Player, t: &Tuning) -> String {
        match self {
            Stat::Hp => format!("{}", t.player.max_hp.round()),
            Stat::Stamina => format!("{}", t.player.max_stamina.round()),
            Stat::Attack => {
                let w = t.weapons.get(p.weapon as usize);
                // The first light attack's hit.
                let damage = w.and_then(|w| w.light.first()).and_then(|m| m.hits.first()).map_or(0.0, |h| h.damage);
                format!("{} ({})", damage.round(), w.map_or("", |w| w.name.get()))
            }
            Stat::Defense => pct(1.0 - p.defense_mult()),
            Stat::Flasks => p.inventory.refill_amount(Item::HealFlask, t).unwrap_or(0).to_string(),
        }
    }
}

/// What the content of the pages depends on.
#[derive(Clone, Copy, PartialEq)]
struct PageCtx {
    has_save: bool,
    /// Boss whose torch is being rekindled.
    reviving: u8,
    /// Number of weapons.
    weapons: u8,
    device: Device,
    /// Checkpoints discovered by the local player (bits), and the one they're at.
    found: u32,
    here: Option<u8>,
    /// Owned items (bits, in the order of `Item::ALL`), and slot being chosen.
    owned: u16,
    choosing: u8,
    /// In a co-op game.
    coop: bool,
}

fn owned_bits(p: Option<&Player>) -> u16 {
    p.map_or(0, |p| Item::ALL.iter().enumerate().filter(|(_, it)| p.inventory.owns(**it)).fold(0, |b, (i, _)| b | 1 << i))
}

/// Grid pages (left/right and up/down move around them): number of cells in each row,
/// for `n` entries.
fn grid_rows(page: Page, n: usize, weapons: usize) -> Option<Vec<usize>> {
    match page {
        Page::Revive => Some(vec![n]),
        // The important equipment (talisman, weapons), then the consumables.
        Page::Equipment => Some(vec![weapons + 1, QUICK_SLOTS]),
        // The pause menu: a row of icons.
        Page::Pause => Some(vec![n]),
        _ => None,
    }
}

/// Row and column of entry `i` of a grid.
fn grid_pos(rows: &[usize], i: usize) -> (usize, usize) {
    let mut start = 0;
    for (r, &len) in rows.iter().enumerate() {
        if i < start + len {
            return (r, i - start);
        }
        start += len;
    }
    (rows.len().saturating_sub(1), 0)
}

/// Boss portrait (revive page), in dots.
const PORTRAIT: u32 = 64;
/// Inner margin of the menu panel.
const PANEL_PADDING: f32 = 16.0;

const NATIVE: bool = !cfg!(target_arch = "wasm32");

/// Help page lines: action, keys.
fn help_lines(device: Device) -> Vec<(&'static str, Vec<Seg>)> {
    let or = || t(tr("or", "ou"));
    if device == Device::Gamepad {
        vec![
            (tr("Move / camera", "Déplacement / caméra"), vec![i(Glyph::StickL), t("/"), i(Glyph::StickR)]),
            (tr("Light attack", "Attaque légère"), vec![i(Glyph::PadRB)]),
            (tr("Heavy attack (hold: charge)", "Attaque lourde (maintenir : charge)"), vec![i(Glyph::PadRT)]),
            (tr("Guard (well-timed: perfect)", "Garde (au bon moment : parfaite)"), vec![i(Glyph::PadLB)]),
            (tr("Special attack", "Attaque spéciale"), vec![i(Glyph::PadLT)]),
            (tr("Dodge", "Esquive"), vec![i(Glyph::PadB)]),
            (tr("Sprint", "Course"), vec![t(tr("hold", "maintenir")), i(Glyph::PadB), or(), i(Glyph::StickL3)]),
            (tr("Use item", "Utiliser l'objet"), vec![i(Glyph::PadX)]),
            (tr("Next item", "Objet suivant"), vec![i(Glyph::DpadDown)]),
            (tr("Switch weapon", "Changer d'arme"), vec![i(Glyph::DpadRight)]),
            (tr("Lock on", "Verrouillage"), vec![i(Glyph::StickR3)]),
            (tr("Switch target (locked on)", "Changer de cible (verrouillé)"), vec![i(Glyph::StickR), t(tr("4 directions (up: higher)", "4 directions (haut : plus haut)"))]),
            (tr("Rest (checkpoint)", "Se reposer (checkpoint)"), vec![i(Glyph::PadA)]),
            (tr("Jump, then attack: jump attack", "Sauter, puis frapper : attaque sautée"), vec![i(Glyph::PadA), t("+"), i(Glyph::PadRB)]),
            (tr("Menu", "Menu"), vec![i(Glyph::PadMenu)]),
            (tr("Back (menus)", "Retour (menus)"), vec![i(Glyph::PadB)]),
        ]
    } else {
        vec![
            (
                tr("Move", "Déplacement"),
                vec![i(Glyph::Key("W")), i(Glyph::Key("A")), i(Glyph::Key("S")), i(Glyph::Key("D")), t("AZERTY"), i(Glyph::Key("Z")), i(Glyph::Key("Q"))],
            ),
            (tr("Camera", "Caméra"), vec![i(Glyph::MouseMove), t(tr("(click to capture)", "(clic pour capturer)"))]),
            (tr("Light attack", "Attaque légère"), vec![i(Glyph::MouseLeft)]),
            (tr("Heavy attack (hold: charge)", "Attaque lourde (maintenir : charge)"), vec![i(Glyph::Key(tr("SHIFT", "MAJ"))), t("+"), i(Glyph::MouseLeft)]),
            (tr("Guard (well-timed: perfect)", "Garde (au bon moment : parfaite)"), vec![i(Glyph::MouseRight)]),
            (tr("Special attack", "Attaque spéciale"), vec![i(Glyph::Key(tr("SHIFT", "MAJ"))), t("+"), i(Glyph::MouseRight)]),
            (tr("Dodge (hold: sprint)", "Esquive (maintenir : course)"), vec![i(Glyph::Key(tr("SPACE", "ESPACE")))]),
            (tr("Use item", "Utiliser l'objet"), vec![i(Glyph::Key("R"))]),
            (tr("Next item", "Objet suivant"), vec![i(Glyph::Key("↓"))]),
            (tr("Switch weapon (1: rapier, 2: greatsword)", "Changer d'arme (1 : rapière, 2 : espadon)"), vec![i(Glyph::Key("1/2"))]),
            (
                tr("Lock on", "Verrouillage"),
                vec![i(Glyph::Key("Q")), or(), i(Glyph::MouseMiddle), t("AZERTY"), i(Glyph::Key("A"))],
            ),
            (tr("Switch target (locked on)", "Changer de cible (verrouillé)"), vec![i(Glyph::MouseMove), t(tr("4 directions (up: higher)", "4 directions (haut : plus haut)"))]),
            (tr("Interact, rest (checkpoint)", "Interagir, se reposer (checkpoint)"), vec![i(Glyph::Key("E"))]),
            (tr("Jump, then attack: jump attack", "Sauter, puis frapper : attaque sautée"), vec![i(Glyph::Key("F")), t("+"), i(Glyph::MouseLeft)]),
            (tr("Menu", "Menu"), vec![i(Glyph::Key("ESC"))]),
            (tr("Tuning (debug)", "Réglages (debug)"), vec![i(Glyph::Key("F1")), t(tr("to", "à")), i(Glyph::Key("F5"))]),
        ]
    }
}

fn entries(page: Page, c: &PageCtx) -> Vec<Entry> {
    use Act::*;
    let mut v = match page {
        // With a save, "Continue" comes first (default choice).
        Page::Title if c.has_save => vec![Entry::Act(Load), Entry::Act(NewGame), Entry::Act(Open(Page::Options))],
        Page::Title => vec![Entry::Act(NewGame), Entry::Act(Load), Entry::Act(Open(Page::Options))],
        Page::Online(_) => vec![Entry::Password, Entry::Act(Search), Entry::Act(Back)],
        Page::ConfirmNew => vec![Entry::Act(Back), Entry::Act(ConfirmNew)],
        // Icons: the equipment, then the system (cogwheel).
        Page::Pause => vec![Entry::Act(Open(Page::Equipment)), Entry::Act(Open(Page::Status)), Entry::Act(Open(Page::System))],
        Page::System => vec![Entry::Act(Open(Page::Options)), Entry::Act(Open(Page::Help)), Entry::Act(ToTitle)],
        // "Leave" first: it's the line selected on opening.
        Page::Checkpoint if c.coop => vec![Entry::Act(Leave), Entry::Act(Open(Page::Travel)), Entry::Act(Open(Page::Equipment)), Entry::Act(LeaveCoop)],
        Page::Checkpoint => vec![
            Entry::Act(Leave),
            Entry::Act(Open(Page::Travel)),
            Entry::Act(Open(Page::Equipment)),
            Entry::Act(OnlineHost),
            Entry::Act(OnlineJoin),
        ],
        Page::Travel => (0..32u8).filter(|i| c.found & (1 << i) != 0).map(Entry::Place).chain([Entry::Act(Back)]).collect(),
        // Grids: go back with Esc / (B).
        Page::Revive => vec![Entry::Boss(c.reviving)],
        Page::Equipment => [Entry::Talisman].into_iter().chain((0..c.weapons).map(Entry::Weapon)).chain((0..QUICK_SLOTS as u8).map(Entry::Slot)).collect(),
        Page::Choose => {
            let kind = if c.choosing as usize == QUICK_SLOTS { Kind::Talisman } else { Kind::Consumable };
            std::iter::once(Entry::Pick(None))
                .chain(
                    Item::ALL
                        .into_iter()
                        .enumerate()
                        .filter(|(i, it)| it.kind() == kind && c.owned & (1 << i) != 0)
                        .map(|(_, it)| Entry::Pick(Some(it))),
                )
                .collect()
        }
        Page::Options => {
            use Opt::*;
            let opts: &[Opt] = if NATIVE {
                &[Language, Display, Resolution, Refresh, VSync, Internal, Master, Effects, Sensitivity, InvertY, Shake, ShowFps]
            } else {
                // In the browser, resolution, refresh rate and VSync are handled by the browser.
                &[Language, Display, Internal, Master, Effects, Sensitivity, InvertY, Shake, ShowFps]
            };
            opts.iter().map(|o| Entry::Opt(*o)).chain([Entry::Act(Back)]).collect()
        }
        Page::Status => Stat::ALL.into_iter().map(Entry::Stat).chain([Entry::Act(Back)]).collect(),
        Page::Help => (0..help_lines(c.device).len() as u8).map(Entry::Line).chain([Entry::Act(Back)]).collect(),
        Page::Language => Lang::ALL.into_iter().map(Entry::Lang).collect(),
        Page::Style => vec![Entry::Style(PS1_HEIGHT), Entry::Style(MODERN_HEIGHT)],
    };
    if NATIVE && matches!(page, Page::Title | Page::System) {
        v.push(Entry::Act(Quit));
    }
    v
}

fn selectable(e: Entry, c: &PageCtx) -> bool {
    match e {
        Entry::Line(..) | Entry::Stat(_) => false,
        Entry::Place(i) => c.here != Some(i),
        Entry::Act(Act::Load) => c.has_save,
        _ => true,
    }
}

fn act_label(a: Act) -> &'static str {
    match a {
        Act::Leave => tr("Leave", "Partir"),
        Act::NewGame => tr("New game", "Nouvelle partie"),
        Act::ConfirmNew => tr("Start a new game", "Commencer une nouvelle partie"),
        Act::Load => tr("Continue", "Continuer"),
        Act::Open(Page::Equipment) => tr("Equipment", "Équipement"),
        Act::Open(Page::Status) => tr("Status", "Statut"),
        Act::Open(Page::System) => tr("System", "Système"),
        Act::Open(Page::Options) => tr("Options", "Options"),
        Act::Open(Page::Help) => tr("Help", "Aide"),
        Act::Open(Page::Travel) => tr("Travel", "Voyager"),
        Act::Open(_) => "…",
        Act::OnlineHost => tr("Kindle a beacon", "Allumer un fanal"),
        Act::OnlineJoin => tr("Heed a beacon", "Répondre à un fanal"),
        Act::Search => tr("Search", "Lancer la recherche"),
        Act::LeaveCoop => tr("Leave the co-op", "Quitter la coopération"),
        Act::Back => tr("Back", "Retour"),
        Act::ToTitle => tr("Return to title screen", "Retour à l'écran titre"),
        Act::Quit => tr("Quit game", "Quitter le jeu"),
    }
}

fn opt_label(o: Opt) -> &'static str {
    match o {
        Opt::Display => tr("Display", "Affichage"),
        Opt::Resolution => tr("Resolution", "Résolution"),
        Opt::Refresh => tr("Refresh rate", "Fréquence"),
        Opt::VSync => tr("Vertical sync", "Synchronisation verticale"),
        Opt::Internal => tr("Internal resolution", "Résolution interne"),
        Opt::Master => tr("Master volume", "Volume général"),
        Opt::Effects => tr("Effects volume", "Volume des effets"),
        Opt::Sensitivity => tr("Camera sensitivity", "Sensibilité de la caméra"),
        Opt::InvertY => tr("Invert vertical axis", "Inverser l'axe vertical"),
        Opt::Shake => tr("Camera shake", "Tremblements de caméra"),
        Opt::ShowFps => tr("Show FPS", "Afficher les FPS"),
        Opt::Language => "Language / Langue",
    }
}

fn page_title(p: Page, device: Device) -> &'static str {
    match p {
        Page::Title => "PSX SOULS",
        Page::ConfirmNew => tr("NEW GAME", "NOUVELLE PARTIE"),
        Page::Pause => "PAUSE",
        Page::Checkpoint => "CHECKPOINT",
        Page::Travel => tr("TRAVEL", "VOYAGER"),
        Page::Revive => tr("REKINDLE THE TORCH", "RAVIVER LA TORCHE"),
        Page::Equipment => tr("EQUIPMENT", "ÉQUIPEMENT"),
        Page::Status => tr("STATUS", "STATUT"),
        Page::System => tr("SYSTEM", "SYSTÈME"),
        Page::Choose => tr("CHOOSE", "CHOISIR"),
        Page::Options => "OPTIONS",
        Page::Help if device == Device::Gamepad => tr("HELP — GAMEPAD", "AIDE — MANETTE"),
        Page::Help => tr("HELP — KEYBOARD AND MOUSE", "AIDE — CLAVIER ET SOURIS"),
        Page::Language => "LANGUAGE / LANGUE",
        Page::Style => tr("GRAPHICS STYLE", "STYLE GRAPHIQUE"),
        Page::Online(true) => tr("KINDLE A BEACON", "ALLUMER UN FANAL"),
        Page::Online(false) => tr("HEED A BEACON", "RÉPONDRE À UN FANAL"),
    }
}

fn page_info(p: Page) -> &'static str {
    match p {
        Page::ConfirmNew => tr("The current save will be overwritten.", "La sauvegarde actuelle sera remplacée."),
        Page::Checkpoint => tr("You rest. HP, stamina and items restored.", "Vous vous reposez. PV, endurance et objets restaurés."),
        Page::Travel => tr("Travel to a brazier you have already kindled.", "Rejoindre un brasier déjà ranimé."),
        Page::Revive => tr(
            "The boss will await you again beyond its fog, at full strength.",
            "Le boss vous attendra de nouveau derrière sa brume, en pleine forme.",
        ),
        Page::Style => tr(
            "Can be changed at any time in Options (Internal resolution).",
            "Modifiable à tout moment dans les Options (Résolution interne).",
        ),
        Page::Equipment => tr(
            "Talisman and weapons, then the quick slots (“Next item” cycles through them in game). Select a slot to change it.",
            "Talisman et armes, puis les emplacements rapides (« Objet suivant » passe de l'un à l'autre en jeu). Choisir un emplacement pour le changer.",
        ),
        Page::Choose => tr("What goes in this slot.", "Ce que contiendra cet emplacement."),
        Page::Online(true) => tr(
            "Your beacon will call another player into your world. Optional password: only a player with the same one will see it. Empty: anyone. It burns for 2 minutes at most.",
            "Votre fanal appellera un autre joueur dans votre monde. Mot de passe facultatif : seul un joueur avec le même le verra. Vide : n'importe qui. Il brûle 2 minutes au plus.",
        ),
        Page::Online(false) => tr(
            "Answer another player's beacon and join their world. Optional password: only the beacon lit with the same one. Empty: anyone's. Searching for 2 minutes at most.",
            "Répondez au fanal d'un autre joueur et rejoignez son monde. Mot de passe facultatif : seul le fanal allumé avec le même. Vide : n'importe lequel. Recherche pendant 2 minutes au plus.",
        ),
        _ => "",
    }
}

/// Possible values of an option, and index of the current value.
struct Choices {
    labels: Vec<String>,
    current: usize,
    /// False if the option doesn't apply in the current mode (shown greyed out).
    enabled: bool,
}

fn pct(v: f32) -> String {
    format!("{} %", (v * 100.0).round() as i32)
}

fn steps01() -> Vec<f32> {
    (0..=10).map(|i| i as f32 / 10.0).collect()
}

fn nearest(values: &[f32], v: f32) -> usize {
    values
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - v).abs().total_cmp(&(b.1 - v).abs()))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Steps of 0.1 up to 2, then coarser.
const SENSITIVITIES: [f32; 24] = [
    0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8, 1.9, 2.0, 2.5, 3.0, 4.0, 5.0,
];

fn display_modes() -> Vec<DisplayMode> {
    if crate::settings::exclusive_supported() {
        vec![DisplayMode::Fullscreen, DisplayMode::Exclusive, DisplayMode::Windowed]
    } else {
        vec![DisplayMode::Fullscreen, DisplayMode::Windowed]
    }
}

fn resolutions(s: &Settings, m: Option<&Monitor>) -> Vec<(u32, u32)> {
    match s.display {
        DisplayMode::Exclusive => m.map(exclusive_sizes).unwrap_or_default(),
        DisplayMode::Windowed => windowed_sizes(m),
        DisplayMode::Fullscreen => Vec::new(),
    }
}

fn current_resolution(s: &Settings, m: Option<&Monitor>) -> (u32, u32) {
    s.resolution.unwrap_or_else(|| match (s.display, m) {
        (DisplayMode::Windowed, _) => (1280, 720),
        (_, Some(m)) => (m.physical_width, m.physical_height),
        _ => (1920, 1080),
    })
}

fn choices(item: Opt, s: &Settings, m: Option<&Monitor>) -> Option<Choices> {
    let yes_no = |b: bool, on: &str, off: &str| Choices {
        labels: vec![off.into(), on.into()],
        current: b as usize,
        enabled: true,
    };
    Some(match item {
                Opt::Display => {
            let modes = display_modes();
            Choices {
                labels: modes
                    .iter()
                    .map(|d| match d {
                        DisplayMode::Fullscreen => tr("Fullscreen", "Plein écran"),
                        DisplayMode::Exclusive => tr("Exclusive fullscreen", "Plein écran exclusif"),
                        DisplayMode::Windowed => tr("Windowed", "Fenêtré"),
                    }.into())
                    .collect(),
                current: modes.iter().position(|d| *d == s.display).unwrap_or(0),
                enabled: true,
            }
        }
        Opt::Resolution => {
            let list = resolutions(s, m);
            if list.is_empty() {
                let (w, h) = current_resolution(s, m);
                return Some(Choices { labels: vec![format!("Native ({w}×{h})")], current: 0, enabled: false });
            }
            let cur = current_resolution(s, m);
            Choices {
                labels: list.iter().map(|(w, h)| format!("{w}×{h}")).collect(),
                current: list.iter().position(|r| *r == cur).unwrap_or(0),
                enabled: true,
            }
        }
        Opt::Refresh => {
            let auto = m.and_then(|m| m.refresh_rate_millihertz).map(|r| format!("Auto ({} Hz)", (r as f32 / 1000.0).round()));
            if s.display != DisplayMode::Exclusive {
                return Some(Choices { labels: vec![auto.unwrap_or("Auto".into())], current: 0, enabled: false });
            }
            let rates = m.map(|m| refresh_rates(m, current_resolution(s, Some(m)))).unwrap_or_default();
            if rates.is_empty() {
                return Some(Choices { labels: vec!["Auto".into()], current: 0, enabled: false });
            }
            let cur = s.refresh_mhz.or(m.and_then(|m| m.refresh_rate_millihertz)).unwrap_or(rates[rates.len() - 1]);
            Choices {
                labels: rates.iter().map(|r| format!("{} Hz", (*r as f32 / 1000.0 * 100.0).round() / 100.0)).collect(),
                current: rates.iter().enumerate().min_by_key(|(_, r)| r.abs_diff(cur)).map(|(i, _)| i).unwrap_or(0),
                enabled: true,
            }
        }
        Opt::VSync => yes_no(s.vsync, tr("On", "Activée"), tr("Off", "Désactivée")),
        Opt::Internal => Choices {
            labels: INTERNAL_HEIGHTS
                .iter()
                .map(|h| if *h == PS1_HEIGHT { format!("{h}p (PS1)") } else { format!("{h}p ({})", tr("Modern", "Moderne")) })
                .collect(),
            current: INTERNAL_HEIGHTS.iter().position(|h| *h == s.internal_height).unwrap_or(0),
            enabled: true,
        },
        Opt::Master => Choices { labels: steps01().iter().map(|v| pct(*v)).collect(), current: nearest(&steps01(), s.master_volume), enabled: true },
        Opt::Effects => Choices { labels: steps01().iter().map(|v| pct(*v)).collect(), current: nearest(&steps01(), s.effects_volume), enabled: true },
        Opt::Sensitivity => Choices {
            labels: SENSITIVITIES.iter().map(|v| format!("{v:.1}")).collect(),
            current: nearest(&SENSITIVITIES, s.sensitivity),
            enabled: true,
        },
        Opt::InvertY => yes_no(s.invert_y, tr("Yes", "Oui"), tr("No", "Non")),
        Opt::Shake => yes_no(s.camera_shake, tr("Yes", "Oui"), tr("No", "Non")),
        Opt::ShowFps => yes_no(s.show_fps, tr("Yes", "Oui"), tr("No", "Non")),
        Opt::Language => Choices {
            labels: Lang::ALL.iter().map(|l| l.native_name().into()).collect(),
            current: Lang::ALL.iter().position(|l| *l == s.language.unwrap_or_default()).unwrap_or(0),
            enabled: true,
        },
    })
}

/// Changes an option's value by `delta` steps (without wrapping).
fn change(item: Opt, delta: i32, s: &mut Settings, m: Option<&Monitor>) {
    let Some(c) = choices(item, s, m) else { return };
    if !c.enabled || c.labels.len() < 2 {
        return;
    }
    let i = (c.current as i32 + delta).clamp(0, c.labels.len() as i32 - 1) as usize;
    if i == c.current {
        return;
    }
    match item {
        Opt::Display => {
            s.display = display_modes()[i];
            // The chosen resolution depends on the mode: go back to automatic.
            s.resolution = None;
            s.refresh_mhz = None;
        }
        Opt::Resolution => {
            s.resolution = Some(resolutions(s, m)[i]);
            s.refresh_mhz = None;
        }
        Opt::Refresh => {
            if let Some(m) = m {
                s.refresh_mhz = Some(refresh_rates(m, current_resolution(s, Some(m)))[i]);
            }
        }
        Opt::VSync => s.vsync = i == 1,
        Opt::Internal => s.internal_height = INTERNAL_HEIGHTS[i],
        Opt::Master => s.master_volume = steps01()[i],
        Opt::Effects => s.effects_volume = steps01()[i],
        Opt::Sensitivity => s.sensitivity = SENSITIVITIES[i],
        Opt::InvertY => s.invert_y = i == 1,
        Opt::Shake => s.camera_shake = i == 1,
        Opt::ShowFps => s.show_fps = i == 1,
        Opt::Language => s.language = Some(Lang::ALL[i]),
    }
}


#[derive(Component)]
struct MenuRoot;
#[derive(Component)]
struct MenuTitle;
/// The title and, next to it, the "Fork me" badge (title screen).
#[derive(Component)]
struct TitleRow;
/// Holds the badge to the right of the title, without moving it off centre.
#[derive(Component)]
struct ForkSlot;
/// "Fork me" badge: a click opens the project page (mouse only).
#[derive(Component)]
struct ForkBadge;
/// Menu column (title, lines, footer): narrower and centred on the title screen.
#[derive(Component)]
struct MenuPanel;
/// Title screen background: glow at the bottom of the screen, rising embers and ash.
#[derive(Component)]
struct TitleBackdrop;
/// An ember (or ash flake) of the title screen background. Position as a fraction of
/// the screen, rising speed (screens/s), sway.
#[derive(Component)]
struct Ash {
    pos: Vec2,
    rise: f32,
    sway: f32,
    phase: f32,
    ember: bool,
}
/// Title screen, with a save: the final door's medallions, lit for the defeated bosses.
#[derive(Component)]
struct TitleGems;
#[derive(Component)]
struct TitleGem(u8);
/// Title screen: note at the bottom recommending a gamepad.
#[derive(Component)]
struct TitleNote;
/// Title screen: tagline slanted under the title's right end.
#[derive(Component)]
struct TitleTagline;
#[derive(Component)]
struct MenuInfo;
#[derive(Component)]
struct MenuList;
/// Preview of the place (travel page).
#[derive(Component)]
struct MenuPreview;
#[derive(Component)]
struct MenuPreviewCaption;
/// Description of the selected item (equipment page).
#[derive(Component)]
struct MenuDetail;
#[derive(Component)]
struct MenuFooter;
#[derive(Component)]
struct Row(usize);
#[derive(Component)]
struct RowLabel(usize);
#[derive(Component)]
struct RowValue(usize);
#[derive(Component)]
struct Arrow(usize, i32);
/// Icon and quantity of a slot's item (equipment page), per line.
#[derive(Component)]
struct SlotIcon(usize);
#[derive(Component)]
struct SlotCount(usize);

/// Portraits of the boss encounters (`assets/ui/boss_<n>.png`, rendered by
/// `tools/blender/boss_icons.py`), in the order of `Tuning::encounters`.
#[derive(Resource)]
struct BossPortraits(Vec<Handle<Image>>);

/// Illustrations of the two graphics styles (game screenshots at 240p and 480p).
#[derive(Resource)]
struct StyleImages {
    ps1: Handle<Image>,
    modern: Handle<Image>,
}

/// Size of the illustrations, in dots.
const STYLE_IMAGE_SIZE: UVec2 = UVec2::new(192, 108);

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuState>()
            .add_systems(Startup, spawn_menu)
            .add_systems(OnEnter(AppState::Title), enter_title)
            .add_systems(
                Update,
                (open_on_rest.run_if(in_state(AppState::Playing)), menu_input, refresh_menu, title_backdrop, title_gems, title_note, title_tagline, fork_badge)
                    .chain()
                    .after(crate::fx::consume_events)
                    .run_if(not(in_state(AppState::Loading))),
            );
    }
}

fn spawn_menu(mut commands: Commands, ui_font: Res<UiFont>, preview: Res<CheckpointPreview>, assets: Res<AssetServer>, tuning: Res<Tuning>) {
    commands.insert_resource(StyleImages { ps1: assets.load("ui/style_ps1.png"), modern: assets.load("ui/style_modern.png") });
    commands.insert_resource(BossPortraits((0..tuning.encounters.len()).map(|i| assets.load(format!("ui/boss_{i}.png"))).collect()));
    let text = Color::srgba(0.85, 0.82, 0.75, 0.75);
    commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.72)),
            Visibility::Hidden,
            ZIndex(50),
        ))
        .with_children(|c| {
            spawn_backdrop(c);
            c.spawn((
                Hint::new(1, Color::srgba(0.85, 0.82, 0.75, 0.6)),
                Node { position_type: PositionType::Absolute, bottom: px(24), display: Display::None, ..hint_node() },
                TitleNote,
            ));
            c.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(6), padding: UiRect::all(px(PANEL_PADDING)), width: px(960), ..default() }, MenuPanel))
                .with_children(|c| {
                    c.spawn((Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, ..default() }, TitleRow)).with_children(|c| {
                        c.spawn((ui_font.text("", 2, Color::srgb(0.9, 0.82, 0.62)), TextLayout::linebreak(LineBreak::NoWrap), Node::default(), MenuTitle));
                        c.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: percent(100),
                                top: px(0),
                                bottom: px(0),
                                margin: UiRect::left(px(28)),
                                align_items: AlignItems::Center,
                                display: Display::None,
                                ..default()
                            },
                            ForkSlot,
                        ))
                        .with_child((Button, Hint::new(2, Color::WHITE), hint_node(), ForkBadge));
                        c.spawn((
                            ui_font.text("", 1, Color::srgb(1.0, 0.78, 0.3)),
                            TextLayout::linebreak(LineBreak::NoWrap),
                            Node { position_type: PositionType::Absolute, top: percent(100), right: px(-12), display: Display::None, ..default() },
                            UiTransform { rotation: Rot2::degrees(-8.0), ..default() },
                            TitleTagline,
                        ));
                    });
                    c.spawn((
                        Node { display: Display::None, align_self: AlignSelf::Center, column_gap: px(10), margin: UiRect::bottom(px(24)), ..default() },
                        TitleGems,
                    ))
                    .with_children(|c| {
                        for i in 0..tuning.encounters.len() as u8 {
                            c.spawn((image_bundle(Handle::default(), UVec2::splat(GEM_ICON as u32)), TitleGem(i)));
                        }
                    });
                    c.spawn((ui_font.text("", 1, text), Node { margin: UiRect::bottom(px(10)), width: px(960.0 - 2.0 * PANEL_PADDING), ..default() }, MenuInfo));
                    c.spawn(Node { flex_direction: FlexDirection::Row, column_gap: px(24), align_items: AlignItems::FlexStart, ..default() })
                        .with_children(|c| {
                            c.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(2), flex_grow: 1.0, ..default() }, MenuList));
                            c.spawn((
                                Node {
                                    display: Display::None,
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(6),
                                    padding: UiRect::all(px(4)),
                                    border: UiRect::all(px(2)),
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                                BorderColor::all(Color::srgb(0.62, 0.55, 0.42)),
                                BackgroundColor(Color::BLACK),
                                MenuPreview,
                            ))
                            .with_children(|c| {
                                c.spawn(image_bundle(preview.image.clone(), PREVIEW_SIZE));
                                c.spawn((ui_font.text("", 1, Color::srgb(0.9, 0.82, 0.62)), MenuPreviewCaption));
                            });
                        });
                    c.spawn((
                        ui_font.text("", 1, Color::srgb(0.9, 0.82, 0.62)),
                        Node { display: Display::None, margin: UiRect::top(px(10)), width: px(960.0 - 2.0 * PANEL_PADDING), ..default() },
                        MenuDetail,
                    ));
                    c.spawn((Hint::new(1, text), Node { margin: UiRect::top(px(10)), ..hint_node() }, MenuFooter));
                });
        });
}

/// Number of embers and flakes in the title screen background.
const ASHES: usize = 150;

/// Pseudo-random for the title screen background (purely visual), in [0, 1].
fn ash_rand(seed: u32) -> f32 {
    let x = seed.wrapping_mul(747796405).wrapping_add(2891336453);
    let x = (x ^ (x >> 15)).wrapping_mul(2246822519);
    ((x >> 9) & 0xffff) as f32 / 65535.0
}

fn spawn_backdrop(c: &mut ChildSpawnerCommands) {
    c.spawn((
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), overflow: Overflow::clip(), ..default() },
        Visibility::Hidden,
        TitleBackdrop,
    ))
    .with_children(|c| {
        // Glow of a brazier below the screen: bands getting warmer towards the bottom.
        const BANDS: u32 = 30;
        for k in 0..BANDS {
            let a = 0.17 * ((k + 1) as f32 / BANDS as f32).powi(3);
            c.spawn((
                Node { position_type: PositionType::Absolute, left: px(0), right: px(0), bottom: percent((BANDS - 1 - k) as f32 * 1.6), height: percent(1.6), ..default() },
                BackgroundColor(Color::srgba(0.75, 0.22, 0.04, a)),
            ));
        }
        for n in 0..ASHES as u32 {
            let r = |k: u32| ash_rand(n * 7 + k);
            let ember = r(0) < 0.55;
            let size = if ember { 1 + (r(1) * 1.6) as u32 } else { 1 + (r(1) * 2.2) as u32 };
            c.spawn((
                Node { position_type: PositionType::Absolute, ..default() },
                PixelSize(UVec2::splat(size)),
                BackgroundColor(Color::NONE),
                Ash {
                    pos: Vec2::new(r(2), r(3) * 1.1),
                    rise: if ember { 0.05 + r(4) * 0.09 } else { 0.025 + r(4) * 0.04 },
                    sway: 0.004 + r(5) * 0.012,
                    phase: r(6) * std::f32::consts::TAU,
                    ember,
                },
            ));
        }
    });
}

/// Title screen: big centred title above a narrower menu; the background embers and
/// ash rise (only if it's displayed).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn title_backdrop(
    time: Res<Time>,
    menu: Res<MenuState>,
    mut backdrop: Single<&mut Visibility, With<TitleBackdrop>>,
    mut panel: Single<&mut Node, (With<MenuPanel>, Without<Ash>)>,
    mut title_text: Single<&mut PixelText, With<MenuTitle>>,
    mut title_row: Single<&mut Node, (With<TitleRow>, Without<MenuPanel>, Without<Ash>)>,
    mut footer: Single<&mut Node, (With<MenuFooter>, Without<TitleRow>, Without<MenuPanel>, Without<Ash>)>,
    mut texts: Query<&mut Node, (Or<(With<MenuInfo>, With<MenuDetail>)>, Without<MenuFooter>, Without<TitleRow>, Without<MenuPanel>, Without<Ash>)>,
    mut ashes: Query<(&mut Ash, &mut Node, &mut BackgroundColor), (Without<MenuPanel>, Without<TitleRow>, Without<MenuFooter>, Without<MenuInfo>, Without<MenuDetail>)>,
    mut seed: Local<u32>,
) {
    let title = menu.open && menu.page == Page::Title;
    let width = if title { 720.0 } else { 960.0 };
    if panel.width != px(width) {
        panel.width = px(width);
        // Multi-line texts with an explicit width: otherwise, for the panel height,
        // they're estimated at one word per line and the panel, too tall, overflows the top of the screen.
        for mut n in &mut texts {
            n.width = px(width - 2.0 * PANEL_PADDING);
        }
    }
    let size = if title { 4 } else { 2 };
    if title_text.0 != size {
        title_text.0 = size;
    }
    // Title screen and pause menu (a single row of cards): title and help centred, like the cards.
    let centered = menu.open && matches!(menu.page, Page::Title | Page::Pause);
    // The title's row hugs it (the badge, next to it, is placed from its right edge).
    let align = if centered { AlignSelf::Center } else { AlignSelf::FlexStart };
    let margin = if title { UiRect::bottom(px(28)) } else { UiRect::ZERO };
    if title_row.align_self != align || title_row.margin != margin {
        title_row.align_self = align;
        title_row.margin = margin;
        footer.align_self = if centered { AlignSelf::Center } else { AlignSelf::Auto };
    }
    let shown = menu.on_title();
    let want = if shown { Visibility::Inherited } else { Visibility::Hidden };
    if **backdrop != want {
        **backdrop = want;
    }
    if !shown {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    let t = time.elapsed_secs();
    for (mut a, mut n, mut bg) in &mut ashes {
        a.pos.y -= a.rise * dt;
        if a.pos.y < -0.02 {
            // Starts again from the bottom, elsewhere.
            *seed = seed.wrapping_add(1);
            a.pos = Vec2::new(ash_rand(*seed ^ 0x9e37), 1.02 + ash_rand(*seed ^ 0x51ed) * 0.08);
        }
        let x = a.pos.x + (t * 0.7 + a.phase).sin() * a.sway + (t * 0.23 + a.phase * 2.0).sin() * a.sway * 0.6;
        n.left = percent(x * 100.0);
        n.top = percent(a.pos.y * 100.0);
        // They fade as they rise; the embers twinkle.
        let fade = (a.pos.y * 1.25).clamp(0.0, 1.0) * ((1.02 - a.pos.y) * 8.0).clamp(0.0, 1.0);
        bg.0 = if a.ember {
            let flicker = 0.75 + 0.25 * (t * 9.0 + a.phase * 5.0).sin();
            let cool = 1.0 - a.pos.y;
            Color::srgba(1.0, 0.72 - 0.3 * cool, 0.25 - 0.18 * cool, fade * flicker)
        } else {
            Color::srgba(0.58, 0.54, 0.5, fade * 0.55)
        };
    }
}

/// Title screen: as many medallions as bosses, in their colour if defeated in the save,
/// dull stone otherwise (like those of the final door).
fn title_gems(
    menu: Res<MenuState>,
    save: Res<SaveSlot>,
    tuning: Res<Tuning>,
    icons: Option<Res<MenuIcons>>,
    mut row: Single<&mut Node, With<TitleGems>>,
    mut gems: Query<(&TitleGem, &mut ImageNode)>,
) {
    let defeated = save.data.as_ref().filter(|_| menu.open && menu.page == Page::Title).map(|d| d.progress.defeated);
    let want = if defeated.is_some() { Display::Flex } else { Display::None };
    if row.display != want {
        row.display = want;
    }
    let (Some(defeated), Some(icons)) = (defeated, icons) else { return };
    for (g, mut img) in &mut gems {
        if img.image != icons.gem {
            img.image = icons.gem.clone();
        }
        let [r, gr, b] = tuning.encounter_color(g.0 as usize);
        let c = Vec3::new(r, gr, b);
        let c = if defeated & (1 << g.0) != 0 { c.lerp(Vec3::ONE, 0.15) } else { c * 0.1 + Vec3::splat(0.22) };
        let want = Color::srgb(c.x, c.y, c.z);
        if img.color != want {
            img.color = want;
        }
    }
}

/// Title screen: "gamepad recommended", with its icon.
fn title_note(menu: Res<MenuState>, mut note: Single<(&mut Node, &mut Hint), With<TitleNote>>) {
    let shown = menu.open && menu.page == Page::Title;
    let want = if shown { Display::Flex } else { Display::None };
    if note.0.display != want {
        note.0.display = want;
    }
    if shown {
        set_hint(&mut note.1, vec![i(Glyph::Gamepad), t(tr("Gamepad recommended", "Manette recommandée"))]);
    }
}

fn title_tagline(menu: Res<MenuState>, mut tagline: Single<(&mut Node, &mut Text), With<TitleTagline>>) {
    let shown = menu.open && menu.page == Page::Title;
    let want = if shown { Display::Flex } else { Display::None };
    if tagline.0.display != want {
        tagline.0.display = want;
    }
    let text = tr("Now with coop!", "Maintenant en coop !");
    if shown && tagline.1.0 != text {
        tagline.1.0 = text.into();
    }
}

/// Title screen: "Fork me" badge next to the title, lit when hovered; a click opens the
/// project page.
fn fork_badge(
    menu: Res<MenuState>,
    mut slot: Single<&mut Node, With<ForkSlot>>,
    mut badge: Single<(&mut Hint, Ref<Interaction>, Option<&Children>), With<ForkBadge>>,
    mut images: Query<&mut ImageNode>,
) {
    let shown = menu.open && menu.page == Page::Title;
    let want = if shown { Display::Flex } else { Display::None };
    if slot.display != want {
        slot.display = want;
    }
    if !shown {
        return;
    }
    let (hint, interaction, children) = &mut *badge;
    set_hint(hint, vec![i(Glyph::ForkMe)]);
    if interaction.is_changed() && **interaction == Interaction::Pressed {
        open_url(REPO_URL);
    }
    let tint = if **interaction == Interaction::None { Color::srgb(0.82, 0.8, 0.76) } else { Color::WHITE };
    for c in children.iter().flat_map(|c| c.iter()) {
        if let Ok(mut img) = images.get_mut(c)
            && img.color != tint
        {
            img.color = tint;
        }
    }
}

fn cursor_free(w: &mut World, free: bool) {
    let mut q = w.query_filtered::<&mut CursorOptions, With<PrimaryWindow>>();
    let Ok(mut c) = q.single_mut(w) else { return };
    if free {
        c.grab_mode = CursorGrabMode::None;
        c.visible = true;
    } else if NATIVE {
        c.grab_mode = CursorGrabMode::Locked;
        c.visible = false;
    }
}

fn open_page(w: &mut World, page: Page) {
    w.resource_mut::<MenuState>().open(page);
    cursor_free(w, true);
}

fn close(w: &mut World) {
    w.resource_mut::<MenuState>().open = false;
    cursor_free(w, false);
}

/// Closes the menu (the co-op game starts).
pub fn close_menu(w: &mut World) {
    close(w);
}



/// Title screen: no more fighters in the world.
pub fn enter_title(mut commands: Commands) {
    commands.queue(|w: &mut World| {
        let sim: Vec<Entity> = w.query_filtered::<Entity, With<SimEntity>>().iter(w).collect();
        for e in sim {
            w.despawn(e);
        }
        *w.resource_mut::<Encounter>() = Encounter::default();
        w.resource_mut::<SimEvents>().0.clear();
        w.resource_mut::<SimCommands>().0.clear();
        w.resource_mut::<CameraRig>().initialized = false;
        let page = first_launch_page(w.resource::<Settings>());
        open_page(w, page);
    });
}

/// First launch: ask for the language first, then the graphics style.
fn first_launch_page(s: &Settings) -> Page {
    if s.language.is_none() {
        Page::Language
    } else if !s.style_chosen {
        Page::Style
    } else {
        Page::Title
    }
}

pub enum Launch {
    New,
    Load,
}

/// Starts a game (new, or from the save).
pub fn launch(w: &mut World, how: Launch) {
    let t = w.resource::<Tuning>().clone();
    let (progress, play_time) = match how {
        Launch::New => (Progress::new_game(&t), 0.0),
        Launch::Load => match w.resource::<SaveSlot>().data.clone() {
            Some(d) => (d.progress, d.play_time),
            None => return,
        },
    };
    {
        let mut slot = w.resource_mut::<SaveSlot>();
        slot.play_time = play_time;
        if matches!(how, Launch::New) {
            slot.store(SaveData { version: crate::save::VERSION, play_time, progress: progress.clone() });
        }
    }
    *w.resource_mut::<ResetFight>() = ResetFight { requested: true, players: 1, progress: Some(progress), guests: Vec::new() };
    w.resource_mut::<NextState<AppState>>().set(AppState::Playing);
    close(w);
}

/// Rest at the checkpoint: opens the checkpoint menu. Extinguished torch of a defeated boss: asks
/// whether to rekindle it (to revive the boss).
fn open_on_rest(mut commands: Commands, fx: Res<FxState>, local: Query<(), With<LocalPlayer>>) {
    let rested = fx.last.iter().any(|e| matches!(e, SimEvent::Rested { entity } if local.contains(*entity)));
    if rested {
        commands.queue(|w: &mut World| open_page(w, Page::Checkpoint));
    }
    let torch = fx.last.iter().find_map(|e| match e {
        SimEvent::TorchTouched { entity, arena } if local.contains(*entity) => Some(*arena),
        _ => None,
    });
    if let Some(i) = torch {
        commands.queue(move |w: &mut World| {
            w.resource_mut::<MenuState>().reviving = i;
            open_page(w, Page::Revive);
        });
    }
}

#[derive(Clone, Copy, Debug)]
enum Intent {
    /// Open the pause menu.
    Pause,
    /// Give up looking for a co-op partner.
    CancelSearch,
    /// Previous page (or close).
    Back,
    /// Close the menu (Start).
    Close,
    Move(i32),
    Change(i32),
    Confirm,
    Hover(usize),
    Click(usize),
    Arrow(usize, i32),
}

#[allow(clippy::too_many_arguments)]
fn menu_input(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<State<AppState>>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    window: Single<&Window, With<PrimaryWindow>>,
    rows: Query<(&Row, &Interaction)>,
    pressed: Query<(&Row, &Interaction), Changed<Interaction>>,
    arrows: Query<(&Arrow, &Interaction), Changed<Interaction>>,
    mut menu: ResMut<MenuState>,
    mut typed: MessageReader<KeyboardInput>,
    net: Res<crate::net::Net>,
) {
    let cursor = window.cursor_position();
    let moved = menu.settle <= 0.0 && matches!((menu.cursor, cursor), (Some(a), Some(b)) if a.distance(b) > 0.5);
    menu.settle -= time.delta_secs();
    if cursor.is_some() {
        menu.cursor = cursor;
    }
    if menu.fresh {
        menu.fresh = false;
        return;
    }
    let mut out = Vec::new();
    let pad = |b: GamepadButton| gamepads.iter().any(|g| g.just_pressed(b));
    let esc = keys.just_pressed(KeyCode::Escape);
    let start = pad(GamepadButton::Start) || pad(GamepadButton::Select);
    // The password page takes the keyboard's letters: only the arrows and Enter navigate.
    let typing = menu.open && matches!(menu.page, Page::Online(_));
    let letters = |l: KeyCode| if typing { None } else { Some(l) };
    let k = |codes: &[Option<KeyCode>]| codes.iter().flatten().any(|c| keys.just_pressed(*c));
    if typing {
        for ev in typed.read().filter(|e| e.state == ButtonState::Pressed) {
            match &ev.logical_key {
                Key::Backspace => {
                    menu.password.pop();
                }
                Key::Character(c) => {
                    for ch in c.chars().filter(|ch| !ch.is_control()) {
                        if menu.password.chars().count() < PASSWORD_MAX {
                            menu.password.push(ch);
                        }
                    }
                }
                Key::Space if menu.password.chars().count() < PASSWORD_MAX => menu.password.push(' '),
                _ => {}
            }
        }
    } else {
        typed.clear();
    }
    if !menu.open {
        // Searching for a partner: Esc / Start / (B) give up instead of pausing.
        if net.searching() && (esc || start || pad(GamepadButton::East)) {
            out.push(Intent::CancelSearch);
        } else if (esc || start) && *state.get() == AppState::Playing {
            out.push(Intent::Pause);
        }
    } else {
        if esc || pad(GamepadButton::East) {
            out.push(Intent::Back);
        } else if start {
            out.push(Intent::Close);
        }
        if k(&[Some(KeyCode::ArrowUp), letters(KeyCode::KeyW)]) {
            out.push(Intent::Move(-1));
        }
        if k(&[Some(KeyCode::ArrowDown), letters(KeyCode::KeyS)]) {
            out.push(Intent::Move(1));
        }
        if k(&[Some(KeyCode::ArrowLeft), letters(KeyCode::KeyA)]) {
            out.push(Intent::Change(-1));
        }
        if k(&[Some(KeyCode::ArrowRight), letters(KeyCode::KeyD)]) {
            out.push(Intent::Change(1));
        }
        if k(&[Some(KeyCode::Enter), letters(KeyCode::Space)]) || pad(GamepadButton::South) {
            out.push(Intent::Confirm);
        }
        // Gamepad: D-pad or stick, with repeat when held.
        let mut held = Vec2::ZERO;
        for g in &gamepads {
            let mut v = g.left_stick();
            for (b, d) in [
                (GamepadButton::DPadUp, Vec2::Y),
                (GamepadButton::DPadDown, -Vec2::Y),
                (GamepadButton::DPadLeft, -Vec2::X),
                (GamepadButton::DPadRight, Vec2::X),
            ] {
                if g.pressed(b) {
                    v = d;
                }
            }
            if v.length() > 0.6 {
                held = v;
            }
        }
        if held != Vec2::ZERO {
            menu.repeat -= time.delta_secs();
            if menu.repeat <= 0.0 {
                out.push(if held.y.abs() > held.x.abs() {
                    Intent::Move(if held.y > 0.0 { -1 } else { 1 })
                } else {
                    Intent::Change(if held.x > 0.0 { 1 } else { -1 })
                });
                menu.repeat = if menu.repeat < -1.0 { 0.35 } else { 0.16 };
            }
        } else {
            menu.repeat = -2.0;
        }
        let mut arrow_clicked = false;
        for (a, i) in &arrows {
            if *i == Interaction::Pressed {
                out.push(Intent::Arrow(a.0, a.1));
                arrow_clicked = true;
            }
        }
        // Hover: only when the mouse moves (a page opening under the cursor, in the
        // centre of the screen, must not change the line selected by default).
        if moved {
            for (r, i) in &rows {
                if *i == Interaction::Hovered {
                    out.push(Intent::Hover(r.0));
                }
            }
        }
        for (r, i) in &pressed {
            if *i == Interaction::Pressed && !arrow_clicked {
                out.push(Intent::Click(r.0));
            }
        }
    }
    if !out.is_empty() {
        commands.queue(move |w: &mut World| {
            for i in out {
                handle(w, i);
            }
        });
    }
}

fn page_ctx(w: &mut World) -> PageCtx {
    let (found, here) = local_place(w);
    let owned = owned_bits(local_player(w).as_ref());
    PageCtx {
        owned,
        choosing: w.resource::<MenuState>().choosing,
        has_save: w.resource::<SaveSlot>().data.is_some(),
        reviving: w.resource::<MenuState>().reviving,
        weapons: w.resource::<Tuning>().weapons.len() as u8,
        device: *w.resource::<Device>(),
        found,
        here,
        coop: w.resource::<crate::net::Net>().coop(),
    }
}

/// Checkpoints discovered by the local player, and the one they're near.
fn local_place(w: &mut World) -> (u32, Option<u8>) {
    let t = w.resource::<Tuning>().clone();
    let local = w.resource::<crate::net::LocalId>().0;
    let mut q = w.query::<(&Player, &Body)>();
    q.iter(w).find(|(p, _)| p.id == local).map_or((0, None), |(p, b)| (p.found, near_checkpoint(&t, b.pos)))
}

fn local_player(w: &mut World) -> Option<Player> {
    let local = w.resource::<crate::net::LocalId>().0;
    let mut q = w.query::<&Player>();
    q.iter(w).find(|p| p.id == local).cloned()
}

fn handle(w: &mut World, intent: Intent) {
    let ctx = page_ctx(w);
    let (open, page, selected) = {
        let m = w.resource::<MenuState>();
        (m.open, m.page, m.selected)
    };
    if !open {
        match intent {
            Intent::Pause => open_page(w, Page::Pause),
            Intent::CancelSearch => crate::net::leave(w, None),
            _ => {}
        }
        return;
    }
    let list = entries(page, &ctx);
    match intent {
        Intent::Pause | Intent::CancelSearch => {}
        Intent::Back => {
            if !w.resource_mut::<MenuState>().back() && !w.resource::<MenuState>().on_title() {
                close(w);
            }
        }
        Intent::Close => {
            if !w.resource::<MenuState>().on_title() {
                close(w);
            }
        }
        // Grids: up/down change rows (staying in the column, or on the last
        // cell of a shorter row), left/right change columns (without wrapping).
        Intent::Move(d) if grid_rows(page, list.len(), ctx.weapons as usize).is_some() => {
            let rows = grid_rows(page, list.len(), ctx.weapons as usize).unwrap_or_default();
            let (r, c) = grid_pos(&rows, selected);
            let to = r as i32 + d;
            if (0..rows.len() as i32).contains(&to) {
                let to = to as usize;
                let start: usize = rows[..to].iter().sum();
                w.resource_mut::<MenuState>().selected = start + c.min(rows[to] - 1);
            }
        }
        Intent::Change(d) if grid_rows(page, list.len(), ctx.weapons as usize).is_some() => {
            let rows = grid_rows(page, list.len(), ctx.weapons as usize).unwrap_or_default();
            let (r, c) = grid_pos(&rows, selected);
            let to = c as i32 + d;
            if (0..rows[r] as i32).contains(&to) {
                w.resource_mut::<MenuState>().selected = (selected as i32 + d) as usize;
            }
        }
        Intent::Move(d) => {
            let n = list.len() as i32;
            let mut i = selected as i32;
            for _ in 0..n {
                i = (i + d).rem_euclid(n);
                if selectable(list[i as usize], &ctx) {
                    break;
                }
            }
            w.resource_mut::<MenuState>().selected = i as usize;
        }
        // Styles side by side: left/right switch from one to the other.
        Intent::Change(d) if page == Page::Style => {
            w.resource_mut::<MenuState>().selected = (selected as i32 + d).clamp(0, list.len() as i32 - 1) as usize;
        }
        Intent::Change(d) => {
            if let Some(e) = list.get(selected) {
                change_entry(w, *e, d);
            }
        }
        Intent::Confirm => {
            if let Some(e) = list.get(selected).filter(|e| selectable(**e, &ctx)) {
                confirm(w, *e);
            }
        }
        Intent::Hover(i) => {
            if list.get(i).is_some_and(|e| selectable(*e, &ctx)) {
                w.resource_mut::<MenuState>().selected = i;
            }
        }
        Intent::Click(i) => {
            if let Some(e) = list.get(i).filter(|e| selectable(**e, &ctx)) {
                w.resource_mut::<MenuState>().selected = i;
                if matches!(e, Entry::Act(_) | Entry::Boss(_) | Entry::Slot(_) | Entry::Talisman | Entry::Pick(_)) {
                    confirm(w, *e);
                }
            }
        }
        Intent::Arrow(i, d) => {
            if let Some(e) = list.get(i) {
                w.resource_mut::<MenuState>().selected = i;
                change_entry(w, *e, d);
            }
        }
    }
}

/// Left / right on an option: the previous or next value.
fn change_entry(w: &mut World, e: Entry, d: i32) {
    if let Entry::Opt(o) = e {
        let mon = monitor(w);
        let mut s = w.resource::<Settings>().clone();
        change(o, d, &mut s, mon.as_ref());
        if *w.resource::<Settings>() != s {
            *w.resource_mut::<Settings>() = s;
        }
    }
}

fn confirm(w: &mut World, e: Entry) {
    match e {
        Entry::Act(a) => act(w, a),
        // Enter after typing the password: off it goes.
        Entry::Password => act(w, Act::Search),
        // Weapons are only shown: they're switched in game.
        Entry::Weapon(_) => {}
        // Confirming an option advances it by one step (wrapping for binary choices).
        Entry::Opt(o) => {
            let mon = monitor(w);
            let mut s = w.resource::<Settings>().clone();
            let before = s.clone();
            change(o, 1, &mut s, mon.as_ref());
            if s == before
                && let Some(c) = choices(o, &s, mon.as_ref())
            {
                change(o, -(c.labels.len() as i32), &mut s, mon.as_ref());
            }
            if s != before {
                *w.resource_mut::<Settings>() = s;
            }
        }
        // Slot: the list of what's owned, the current item selected.
        Entry::Slot(_) | Entry::Talisman => {
            let slot = if let Entry::Slot(s) = e { s } else { QUICK_SLOTS as u8 };
            let current = local_player(w).and_then(|p| if let Entry::Slot(s) = e { p.inventory.slots[s as usize] } else { p.inventory.talisman });
            w.resource_mut::<MenuState>().choosing = slot;
            w.resource_mut::<MenuState>().push(Page::Choose);
            let ctx = page_ctx(w);
            let at = entries(Page::Choose, &ctx).iter().position(|x| *x == Entry::Pick(current)).unwrap_or(0);
            w.resource_mut::<MenuState>().selected = at;
        }
        Entry::Pick(item) => {
            let Some(p) = local_player(w) else { return };
            let slot = w.resource::<MenuState>().choosing;
            let cmd = if slot as usize == QUICK_SLOTS {
                SimCommand::EquipTalisman { player: p.id, item }
            } else {
                SimCommand::Equip { player: p.id, slot, item }
            };
            // Solo: the sim is paused during the menu, the command is applied right away.
            crate::net::send_command(w, cmd);
            w.resource_mut::<MenuState>().back();
        }
        // Travel: respawn rested in front of the other brazier (enemies back at their post).
        Entry::Place(i) => {
            let Some(p) = local_player(w) else { return };
            crate::net::send_command(w, SimCommand::Travel { player: p.id, checkpoint: i });
            crate::save::save_now(w);
            close(w);
        }
        // The torch lights up again: the boss awaits beyond its fog.
        Entry::Boss(i) => {
            crate::net::send_command(w, SimCommand::ReviveBoss(i));
            crate::save::save_now(w);
            close(w);
        }
        Entry::Line(..) | Entry::Stat(_) => {}
        Entry::Lang(l) => {
            w.resource_mut::<Settings>().language = Some(l);
            crate::lang::set(l);
            let page = first_launch_page(w.resource::<Settings>());
            w.resource_mut::<MenuState>().open(page);
        }
        Entry::Style(h) => {
            let mut s = w.resource_mut::<Settings>();
            s.internal_height = h;
            s.style_chosen = true;
            w.resource_mut::<MenuState>().open(Page::Title);
        }
    }
}

fn act(w: &mut World, a: Act) {
    match a {
        Act::Leave => close(w),
        Act::NewGame => {
            if w.resource::<SaveSlot>().data.is_some() {
                w.resource_mut::<MenuState>().push(Page::ConfirmNew);
            } else {
                launch(w, Launch::New);
            }
        }
        Act::ConfirmNew => launch(w, Launch::New),
        Act::Load => launch(w, Launch::Load),
        Act::Open(p) => w.resource_mut::<MenuState>().push(p),
        Act::Back => {
            w.resource_mut::<MenuState>().back();
        }
        Act::ToTitle => {
            // Co-op: each one back in their own world first (the guest's is saved then).
            if w.resource::<crate::net::Net>().coop() {
                crate::net::leave(w, None);
            } else {
                crate::save::save_now(w);
            }
            w.resource_mut::<NextState<AppState>>().set(AppState::Title);
        }
        Act::OnlineHost | Act::OnlineJoin => {
            w.resource_mut::<MenuState>().push(Page::Online(a == Act::OnlineHost));
        }
        Act::Search => {
            let (page, password) = {
                let m = w.resource::<MenuState>();
                (m.page, m.password.clone())
            };
            if let Page::Online(host) = page {
                crate::net::search(w, host, &password);
                close(w);
            }
        }
        Act::LeaveCoop => {
            crate::net::leave(w, None);
            close(w);
        }
        Act::Quit => {
            // The save is written at the end of the frame (see `save`).
            w.write_message(AppExit::Success);
        }
    }
}

/// Opens a web page (system browser, or new tab in the browser).
fn open_url(url: &str) {
    #[cfg(target_arch = "wasm32")]
    if let Some(w) = web_sys::window() {
        let _ = w.open_with_url_and_target(url, "_blank");
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let cmd = if cfg!(target_os = "windows") {
            std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn()
        } else if cfg!(target_os = "macos") {
            std::process::Command::new("open").arg(url).spawn()
        } else {
            std::process::Command::new("xdg-open").arg(url).spawn()
        };
        if let Err(e) = cmd {
            warn!("cannot open {url}: {e}");
        }
    }
}

fn monitor(w: &mut World) -> Option<Monitor> {
    let mut q = w.query::<(&Monitor, Has<PrimaryMonitor>)>();
    crate::settings::pick_monitor(q.iter(w)).cloned()
}

fn entry_value(e: Entry, c: &PageCtx, s: &Settings, m: Option<&Monitor>, player: Option<&Player>, save: Option<&SaveData>, t: &Tuning) -> (String, bool) {
    match e {
        Entry::Opt(o) => choices(o, s, m).map(|c| (c.labels[c.current].clone(), c.enabled)).unwrap_or_default(),
        Entry::Slot(i) => {
            let item = player.and_then(|p| p.inventory.slots[i as usize].map(|it| (it, p.inventory.count(it))));
            (item.map_or("—".into(), |(it, n)| format!("{} ×{n}", it.name())), true)
        }
        Entry::Talisman => (player.and_then(|p| p.inventory.talisman).map_or("—", Item::name).into(), true),
        Entry::Pick(Some(it)) if it.kind() == Kind::Consumable => (format!("×{}", player.map_or(0, |p| p.inventory.count(it))), true),
        Entry::Pick(_) => (String::new(), true),
        Entry::Act(Act::Load) if save.is_none() => (tr("No save", "Aucune sauvegarde").into(), false),
        Entry::Place(i) if c.here == Some(i) => (tr("You are here", "Vous êtes ici").into(), false),
        Entry::Place(_) => (String::new(), true),
        Entry::Boss(_) => (tr("Rekindle", "Raviver").into(), true),
        Entry::Weapon(i) if player.is_some_and(|p| p.weapon == i) => (tr("In hand", "En main").into(), true),
        Entry::Weapon(_) => (String::new(), true),
        Entry::Stat(st) => (player.map_or(String::new(), |p| st.value(p, t)), true),
        Entry::Line(..) | Entry::Lang(_) | Entry::Style(_) | Entry::Act(_) | Entry::Password => (String::new(), true),
    }
}

fn entry_label(e: Entry, t: &Tuning, device: Device) -> String {
    match e {
        Entry::Place(i) => t.level.checkpoints.get(i as usize).map_or("", |c| c.name.get()).into(),
        Entry::Boss(i) => t.encounters.get(i as usize).map_or("", |c| c.name.get()).into(),
        Entry::Talisman => tr("Talisman", "Talisman").into(),
        Entry::Weapon(i) => t.weapons.get(i as usize).map_or("", |w| w.name.get()).into(),
        Entry::Pick(Some(it)) => it.name().into(),
        Entry::Pick(None) => tr("(empty)", "(vide)").into(),
        Entry::Act(a) => act_label(a).into(),
        Entry::Opt(o) => opt_label(o).into(),
        Entry::Slot(i) => format!("{} {}", tr("Slot", "Emplacement"), i + 1),
        Entry::Line(l) => help_lines(device).get(l as usize).map_or("", |h| h.0).into(),
        Entry::Lang(l) => l.native_name().into(),
        Entry::Style(h) if h == PS1_HEIGHT => "PS1".into(),
        Entry::Style(_) => tr("Modern", "Moderne").into(),
        Entry::Stat(st) => st.label().into(),
        Entry::Password => tr("Password", "Mot de passe").into(),
    }
}

/// Card of a grid page: boss portrait, equipment cell, pause menu icon.
struct CardCtx {
    font: UiFont,
    frame: Color,
    portraits: Vec<Handle<Image>>,
    weapons: Vec<Handle<Image>>,
    equipment: Handle<Image>,
    status: Handle<Image>,
    system: Handle<Image>,
}

fn spawn_card(c: &mut ChildSpawnerCommands, i: usize, e: Entry, k: &CardCtx) {
    let width = match e {
        Entry::Boss(_) => 226,
        Entry::Act(_) => 180,
        _ => 170,
    };
    c.spawn((
        Row(i),
        Button,
        Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: px(4), padding: UiRect::all(px(6)), width: px(width), ..default() },
        BackgroundColor(Color::NONE),
    ))
    .with_children(|c| {
        // Framed cell around an icon of `n` dots.
        let boxed = |c: &mut ChildSpawnerCommands, n: u32| {
            c.spawn((
                Node { border: UiRect::all(px(2)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                PixelSize(UVec2::splat(n + 8)),
                BorderColor::all(k.frame),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
            ))
            .id()
        };
        match e {
            Entry::Boss(b) => {
                let portrait = k.portraits.get(b as usize).cloned().unwrap_or_default();
                c.spawn((Node { border: UiRect::all(px(2)), ..default() }, BorderColor::all(k.frame), BackgroundColor(Color::BLACK)))
                    .with_child(image_bundle(portrait, UVec2::splat(PORTRAIT)));
            }
            // Pause menu icons, at the size of the others.
            Entry::Act(a) => {
                let icon = match a {
                    Act::Open(Page::System) => k.system.clone(),
                    Act::Open(Page::Status) => k.status.clone(),
                    _ => k.equipment.clone(),
                };
                let b = boxed(c, ITEM_ICON as u32);
                c.commands().entity(b).with_child(image_bundle(icon, UVec2::splat(ITEM_ICON as u32)));
            }
            Entry::Weapon(wi) => {
                let b = boxed(c, ITEM_ICON as u32);
                c.commands().entity(b).with_child(image_bundle(k.weapons.get(wi as usize).cloned().unwrap_or_default(), UVec2::splat(ITEM_ICON as u32)));
            }
            // Item cell, at the size of the choice list icons (and its quantity).
            _ => {
                let b = boxed(c, ITEM_ICON as u32);
                let font = k.font.clone();
                c.commands().entity(b).with_children(|c| {
                    c.spawn((image_bundle(Handle::default(), UVec2::splat(ITEM_ICON as u32)), SlotIcon(i)));
                    c.spawn((
                        font.text("", 1, Color::srgb(0.95, 0.92, 0.85)),
                        Node { position_type: PositionType::Absolute, right: px(1), bottom: px(-2), ..default() },
                        SlotCount(i),
                    ));
                });
            }
        }
        let wrap = if matches!(e, Entry::Boss(_)) { LineBreak::WordBoundary } else { LineBreak::NoWrap };
        c.spawn((k.font.text("", 1, Color::srgb(0.85, 0.82, 0.75)), TextLayout::new(Justify::Center, wrap), RowLabel(i)));
        if matches!(e, Entry::Boss(_) | Entry::Weapon(_)) {
            // No line wrapping: otherwise the card is estimated taller than it is.
            c.spawn((k.font.text("", 1, Color::srgb(0.95, 0.92, 0.85)), TextLayout::new(Justify::Center, LineBreak::NoWrap), RowValue(i)));
        }
    });
}

/// Recreates the list lines (in rows of cards on grid pages).
fn build_rows(w: &mut World, list: Entity, entries: &[Entry], page: Page) {
    let ui_font = w.resource::<UiFont>().clone();
    let help = help_lines(*w.resource::<Device>());
    let (left, right) = w.resource_scope(|w, mut icons: Mut<Icons>| {
        let mut images = w.resource_mut::<Assets<Image>>();
        (icon_bundle(&mut icons, &mut images, Glyph::ValueLeft, 1), icon_bundle(&mut icons, &mut images, Glyph::ValueRight, 1))
    });
    let style = (w.resource::<StyleImages>().ps1.clone(), w.resource::<StyleImages>().modern.clone());
    let item_icon = |w: &World, it: Item| w.get_resource::<ItemIcons>().map(|ic| ic.get(it)).unwrap_or_default();
    let pick_icons: Vec<Option<Handle<Image>>> =
        entries.iter().map(|e| if let Entry::Pick(Some(it)) = e { Some(item_icon(w, *it)) } else { None }).collect();
    let side_by_side = entries.iter().any(|e| matches!(e, Entry::Style(_)));
    let n_weapons = w.resource::<Tuning>().weapons.len();
    let grid = grid_rows(page, entries.len(), n_weapons);
    let card = CardCtx {
        font: ui_font.clone(),
        frame: Color::srgb(0.62, 0.55, 0.42),
        portraits: w.resource::<BossPortraits>().0.clone(),
        weapons: (0..n_weapons).map(|i| w.get_resource::<WeaponIcons>().map(|ic| ic.get(i)).unwrap_or_default()).collect(),
        equipment: w.get_resource::<MenuIcons>().map(|m| m.equipment.clone()).unwrap_or_default(),
        status: w.get_resource::<MenuIcons>().map(|m| m.status.clone()).unwrap_or_default(),
        system: w.get_resource::<MenuIcons>().map(|m| m.system.clone()).unwrap_or_default(),
    };
    let mut commands = w.commands();
    commands.entity(list).entry::<Node>().and_modify(move |mut n| {
        n.flex_direction = if side_by_side { FlexDirection::Row } else { FlexDirection::Column };
        n.justify_content = if side_by_side { JustifyContent::SpaceEvenly } else { JustifyContent::Default };
        n.align_items = if page == Page::Pause { AlignItems::Center } else { AlignItems::Default };
        n.flex_wrap = FlexWrap::NoWrap;
        n.row_gap = px(2);
    });
    if let Some(rows) = grid {
        // Grid pages: one row of cards per grid row.
        commands.entity(list).despawn_children().with_children(|c| {
            let mut i = 0;
            for len in rows {
                c.spawn(Node { flex_direction: FlexDirection::Row, column_gap: px(8), margin: UiRect::bottom(px(6)), ..default() }).with_children(|c| {
                    for _ in 0..len {
                        if let Some(e) = entries.get(i) {
                            spawn_card(c, i, *e, &card);
                        }
                        i += 1;
                    }
                });
            }
        });
        return;
    }
    commands.entity(list).despawn_children().with_children(|c| {
        for (i, e) in entries.iter().enumerate() {
            if let Entry::Style(h) = *e {
                // Card: illustration, style name, description.
                let (image, desc) = if h == PS1_HEIGHT {
                    (style.0.clone(), tr("240p: big pixels, the 1997 look", "240p : gros pixels, l'image de 1997"))
                } else {
                    (style.1.clone(), tr("480p: a sharper image", "480p : une image plus nette"))
                };
                c.spawn((
                    Row(i),
                    Button,
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: px(8),
                        padding: UiRect::all(px(10)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|c| {
                    c.spawn((Node { border: UiRect::all(px(2)), ..default() }, BorderColor::all(Color::srgb(0.62, 0.55, 0.42))))
                        .with_child(image_bundle(image, STYLE_IMAGE_SIZE));
                    c.spawn((ui_font.text("", 2, Color::srgb(0.85, 0.82, 0.75)), RowLabel(i)));
                    c.spawn(ui_font.text(desc, 1, Color::srgb(0.78, 0.75, 0.68)));
                });
                continue;
            }
            let line = matches!(e, Entry::Line(..));
            c.spawn((
                Row(i),
                Button,
                Node {
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(px(12), px(if line { 0 } else { 4 })),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|c| {
                // Icon in front of the label: offered item.
                let icon = match e {
                    Entry::Pick(Some(_)) => pick_icons[i].clone().map(|h| image_bundle(h, UVec2::splat(ITEM_ICON as u32))),
                    // Empty cell: a transparent icon, to align the labels.
                    Entry::Pick(None) => {
                        let mut b = image_bundle(Handle::default(), UVec2::splat(ITEM_ICON as u32));
                        b.0.color = Color::NONE;
                        Some(b)
                    }
                    _ => None,
                };
                let label = Node { flex_shrink: 0.0, margin: UiRect::right(px(16)), ..default() };
                if let Some(icon) = icon {
                    c.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(8), ..default() }).with_children(|c| {
                        c.spawn(icon);
                        c.spawn((ui_font.text("", 1, Color::srgb(0.85, 0.82, 0.75)), TextLayout::linebreak(LineBreak::NoWrap), label, RowLabel(i)));
                    });
                } else {
                    c.spawn((ui_font.text("", 1, Color::srgb(0.85, 0.82, 0.75)), TextLayout::linebreak(LineBreak::NoWrap), label, RowLabel(i)));
                }
                if let Entry::Line(l) = e {
                    // Help: key icons, left-aligned.
                    let mut h = Hint::new(1, Color::srgb(0.85, 0.82, 0.75));
                    h.segs = help.get(*l as usize).map(|h| h.1.clone()).unwrap_or_default();
                    c.spawn(Node { width: px(340), flex_shrink: 0.0, ..default() }).with_children(|c| {
                        c.spawn((h, hint_node()));
                    });
                    return;
                }
                if matches!(e, Entry::Lang(_)) {
                    return;
                }
                c.spawn(Node { flex_direction: FlexDirection::Row, column_gap: px(10), align_items: AlignItems::Center, ..default() })
                    .with_children(|c| {
                        c.spawn((Button, left.clone(), Arrow(i, -1)));
                        c.spawn((
                            ui_font.text("", 1, Color::srgb(0.95, 0.92, 0.85)),
                            // No line wrapping: otherwise a value with spaces ("(none) — type it") is
                            // estimated taller, and the panel jumps when it changes (typing the password).
                            TextLayout::linebreak(LineBreak::NoWrap),
                            Node { min_width: px(240), justify_content: JustifyContent::Center, ..default() },
                            RowValue(i),
                        ));
                        c.spawn((Button, right.clone(), Arrow(i, 1)));
                    });
            });
        }
    });
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn refresh_menu(
    mut commands: Commands,
    mut menu: ResMut<MenuState>,
    (settings, save, device, tuning, net): (Res<Settings>, Res<SaveSlot>, Res<Device>, Res<Tuning>, Res<crate::net::Net>),
    mut preview: ResMut<CheckpointPreview>,
    mut preview_node: Query<&mut Node, With<MenuPreview>>,
    monitors: Query<(&Monitor, Has<PrimaryMonitor>)>,
    players: Query<(&Player, &Body), With<LocalPlayer>>,
    mut root: Single<(&mut Visibility, &mut BackgroundColor), With<MenuRoot>>,
    list: Single<Entity, With<MenuList>>,
    mut texts: ParamSet<(
        Query<&mut Text, With<MenuTitle>>,
        Query<(&mut Text, &mut Node), (With<MenuInfo>, Without<MenuPreview>)>,
        Query<&mut Hint, With<MenuFooter>>,
        Query<(&RowLabel, &mut Text, &mut TextColor)>,
        Query<(&RowValue, &mut Text, &mut TextColor)>,
        Query<&mut Text, With<MenuPreviewCaption>>,
        Query<(&mut Text, &mut Node), (With<MenuDetail>, Without<MenuPreview>)>,
        Query<(&SlotCount, &mut Text)>,
    )>,
    mut slot_icons: Query<(&SlotIcon, &mut ImageNode)>,
    item_icons: Option<Res<ItemIcons>>,
    mut rows: Query<(&Row, &mut BackgroundColor), Without<MenuRoot>>,
    mut arrows: Query<(&Arrow, &mut Visibility), Without<MenuRoot>>,
    mut built: Local<(Vec<Entry>, Option<Lang>)>,
) {
    let want = if menu.open { Visibility::Inherited } else { Visibility::Hidden };
    if *root.0 != want {
        *root.0 = want;
    }
    // The place preview (and its camera) only lives on the travel page.
    let travel = menu.open && menu.page == Page::Travel;
    if preview.shown != travel {
        preview.shown = travel;
    }
    for mut n in &mut preview_node {
        let want = if travel { Display::Flex } else { Display::None };
        if n.display != want {
            n.display = want;
        }
    }
    if !menu.open {
        return;
    }
    // The title screen completely hides the world.
    root.1.0 = if menu.on_title() { Color::srgb(0.03, 0.025, 0.035) } else { Color::srgba(0.0, 0.0, 0.0, 0.72) };
    let (found, here) = players
        .iter()
        .next()
        .map_or((0, None), |(p, b)| (p.found, near_checkpoint(&tuning, b.pos)));
    let owned = owned_bits(players.iter().next().map(|(p, _)| p));
    let ctx = PageCtx {
        has_save: save.data.is_some(),
        reviving: menu.reviving,
        weapons: tuning.weapons.len() as u8,
        device: *device,
        found,
        here,
        owned,
        choosing: menu.choosing,
        coop: net.coop(),
    };
    let list_entries = entries(menu.page, &ctx);
    // The help lines (icons) also depend on the language.
    let key = (list_entries.clone(), Some(crate::lang::current()));
    if *built != key {
        let (list, entries) = (*list, list_entries.clone());
        let page = menu.page;
        commands.queue(move |w: &mut World| build_rows(w, list, &entries, page));
        *built = key;
        // New lines only exist on the next frame.
        return;
    }
    if !list_entries.get(menu.selected).is_some_and(|e| selectable(*e, &ctx)) {
        menu.selected = list_entries.iter().position(|e| selectable(*e, &ctx)).unwrap_or(0);
    }
    let set = |t: &mut Text, s: &str| {
        if t.0 != s {
            t.0 = s.into();
        }
    };
    for mut t in &mut texts.p0() {
        set(&mut t, page_title(menu.page, *device));
    }
    for (mut t, mut n) in &mut texts.p1() {
        let info = match menu.page {
            Page::Pause if net.coop() => tr("Co-op: the game goes on.", "Coop : la partie continue.").into(),
            p => page_info(p).to_string(),
        };
        let info = info.as_str();
        set(&mut t, info);
        let want = if info.is_empty() { Display::None } else { Display::Flex };
        if n.display != want {
            n.display = want;
        }
    }
    let mut footer = if matches!(menu.page, Page::Language | Page::Style) {
        // Nothing to modify on these pages: choose and confirm. The language isn't chosen yet
        // on the first one.
        let keys = if menu.page == Page::Style { ["←", "→"] } else { ["↑", "↓"] };
        let (nav, ok) = if *device == Device::Gamepad { (vec![i(Glyph::Dpad)], Glyph::PadA) } else { (keys.map(|k| i(Glyph::Key(k))).to_vec(), Glyph::Key("↵")) };
        let select = if menu.page == Page::Style { tr("select", "choisir") } else { "select / choisir" };
        nav.into_iter().chain([t(select), i(ok), t("OK")]).collect()
    } else if grid_rows(menu.page, list_entries.len(), ctx.weapons as usize).is_some() || menu.page == Page::Choose {
        // Nothing to modify: choose, confirm.
        let nav = if *device == Device::Gamepad {
            vec![i(Glyph::Dpad)]
        } else if menu.page == Page::Choose {
            vec![i(Glyph::Key("↑")), i(Glyph::Key("↓"))]
        } else if menu.page == Page::Pause {
            // A single row of cards.
            vec![i(Glyph::Key("←")), i(Glyph::Key("→"))]
        } else {
            ["↑", "↓", "←", "→"].map(|k| i(Glyph::Key(k))).to_vec()
        };
        let ok = if *device == Device::Gamepad { Glyph::PadA } else { Glyph::Key("↵") };
        nav.into_iter().chain([t(tr("select", "choisir")), i(ok), t(tr("confirm", "valider"))]).collect()
    } else if *device == Device::Gamepad {
        vec![i(Glyph::Dpad), t(tr("select / change", "choisir / modifier")), i(Glyph::PadA), t(tr("confirm", "valider"))]
    } else {
        vec![
            i(Glyph::Key("↑")),
            i(Glyph::Key("↓")),
            t(tr("select", "choisir")),
            i(Glyph::Key("←")),
            i(Glyph::Key("→")),
            t(tr("change", "modifier")),
            i(Glyph::Key("↵")),
            t(tr("confirm", "valider")),
        ]
    };
    if !menu.on_title() {
        footer.extend([i(if *device == Device::Gamepad { Glyph::PadB } else { Glyph::Key("ESC") }), t(tr("back", "retour"))]);
    }
    for mut h in &mut texts.p2() {
        set_hint(&mut h, footer.clone());
    }
    let m = crate::settings::pick_monitor(monitors.iter());
    let player = players.iter().next().map(|(p, _)| p);
    for (r, mut bg) in &mut rows {
        bg.0 = if r.0 == menu.selected { Color::srgba(0.9, 0.82, 0.62, 0.16) } else { Color::NONE };
    }
    for (l, mut t, mut c) in &mut texts.p3() {
        let Some(e) = list_entries.get(l.0) else { continue };
        set(&mut t, &entry_label(*e, &tuning, *device));
        c.0 = if matches!(e, Entry::Line(..) | Entry::Stat(_)) {
            Color::srgb(0.78, 0.75, 0.68)
        } else if !selectable(*e, &ctx) {
            Color::srgba(0.6, 0.58, 0.55, 0.6)
        } else if l.0 == menu.selected {
            Color::srgb(1.0, 0.95, 0.85)
        } else {
            Color::srgb(0.78, 0.75, 0.68)
        };
    }
    for (v, mut t, mut c) in &mut texts.p4() {
        let Some(e) = list_entries.get(v.0) else { continue };
        let (s, enabled) = match e {
            Entry::Password if menu.password.is_empty() => (tr("(none) — type it", "(aucun) — tapez-le").into(), true),
            Entry::Password => (format!("{}_", menu.password), true),
            _ => entry_value(*e, &ctx, &settings, m, player, save.data.as_ref(), &tuning),
        };
        set(&mut t, &s);
        c.0 = if enabled { Color::srgb(0.95, 0.92, 0.85) } else { Color::srgba(0.6, 0.58, 0.55, 0.6) };
    }
    // Preview: the place of the selected line (or the one you're at).
    let shown = match list_entries.get(menu.selected) {
        Some(Entry::Place(i)) => *i as usize,
        _ => here.unwrap_or(0) as usize,
    };
    if preview.index != shown {
        preview.index = shown;
    }
    for mut t in &mut texts.p5() {
        set(&mut t, tuning.level.checkpoints.get(shown).map_or("", |c| c.name.get()));
    }
    // Equipment: the item of each cell, and what the one in the selected cell does.
    let slot_item = |e: Option<&Entry>| match e {
        Some(Entry::Slot(i)) => player.and_then(|p| p.inventory.slots[*i as usize]),
        Some(Entry::Talisman) => player.and_then(|p| p.inventory.talisman),
        Some(Entry::Pick(it)) => *it,
        _ => None,
    };
    for (s, mut img) in &mut slot_icons {
        let it = slot_item(list_entries.get(s.0));
        let handle = it.zip(item_icons.as_ref()).map(|(it, ic)| ic.get(it)).unwrap_or_default();
        if img.image != handle {
            img.image = handle;
        }
        let a = if it.is_some() { 1.0 } else { 0.0 };
        if img.color.alpha() != a {
            img.color.set_alpha(a);
        }
    }
    for (s, mut t) in &mut texts.p7() {
        let n = slot_item(list_entries.get(s.0)).filter(|it| it.kind() == Kind::Consumable).map_or(0, |it| player.map_or(0, |p| p.inventory.count(it)));
        set(&mut t, &if n > 0 { n.to_string() } else { String::new() });
    }
    let selected_entry = list_entries.get(menu.selected);
    let detail = match (slot_item(selected_entry), selected_entry) {
        (Some(it), _) => format!("{} — {}", it.name(), it.description()),
        (None, Some(Entry::Slot(_) | Entry::Talisman)) => tr("(empty)", "(vide)").into(),
        (None, Some(Entry::Pick(None))) => tr("(empty) — Clears this slot.", "(vide) — Libère cet emplacement.").into(),
        (None, Some(Entry::Weapon(i))) => {
            let w = tuning.weapons.get(*i as usize);
            format!("{} — {}", w.map_or("", |w| w.name.get()), w.map_or("", |w| w.description.get()))
        }
        _ => String::new(),
    };
    // On the equipment pages, the description line is always there (even empty): the
    // elements don't move from one selection to the next.
    let keep = matches!(menu.page, Page::Equipment | Page::Choose);
    for (mut t, mut n) in &mut texts.p6() {
        set(&mut t, if detail.is_empty() && keep { " " } else { &detail });
        let want = if detail.is_empty() && !keep { Display::None } else { Display::Flex };
        if n.display != want {
            n.display = want;
        }
    }
    for (a, mut vis) in &mut arrows {
        let show = match list_entries.get(a.0) {
            Some(Entry::Opt(o)) => choices(*o, &settings, m).is_some_and(|c| c.enabled && c.labels.len() > 1),
            _ => false,
        };
        let w = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != w {
            *vis = w;
        }
    }
}
