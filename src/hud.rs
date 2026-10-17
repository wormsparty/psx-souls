//! Interface: player and boss bars, quick item, lock-on reticle,
//! interaction prompt, banners and fade on death.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;

use crate::fx::FxState;
use crate::input::Device;
use crate::lang::{Localized, tr};
use crate::sim::input::MAX_PLAYERS;
use crate::menu::MenuState;
use crate::render::camera::{LockFoes, PartBones, lock_target};
use crate::render::ps1::{LowResTarget, WorldCamera};
use crate::render::{AppState, Interp, LocalPlayer};
use std::collections::{HashMap, VecDeque};

use crate::sim::SimEvent;
use crate::sim::boss::Boss;
use crate::sim::data::Tuning;
use crate::sim::encounter::{self, Encounter, RESPAWN_TICKS, near_checkpoint, near_loot, near_pickup};
use crate::sim::fighter::{Body, Foe, Health};
use crate::sim::items::{Item, Loot, QUICK_SLOTS};
use crate::sim::player::{PState, Player};
use crate::sim::world::Zone;
use crate::ui::{Glyph, Hint, PixelSize, UiFont, hint_node, i, image_bundle, set_hint, t};

const HP_PX: f32 = 0.51; // px per HP
/// The allies' bars, half as long.
const ALLY_HP_PX: f32 = HP_PX * 0.5;
const ST_PX: f32 = 2.0; // px per stamina point
const SP_PX: f32 = 0.6; // px per special gauge point
const SPECIAL: Color = Color::srgb(0.9, 0.7, 0.25);
/// Special ready: light yellow stripes scrolling over the gauge (period and speed in px).
const STRIPE_LIGHT: Color = Color::srgb(1.0, 0.9, 0.45);
const STRIPE_PERIOD: f32 = 12.0;
const STRIPE_SPEED: f32 = 24.0;
const TEXT: Color = Color::srgb(0.85, 0.82, 0.75);
const GOLD: Color = Color::srgb(0.9, 0.75, 0.4);

#[derive(Component)]
enum Bar {
    Hp,
    Regain,
    Stamina,
    HpFrame,
    StaminaFrame,
    Special,
    SpecialFrame,
    /// Bars of the boss shown in this slot (two for a duo).
    BossHp(u8),
    BossStagger(u8),
}

/// Stripe `i` of the special gauge, shown when the gauge is full.
#[derive(Component)]
struct SpecialStripe(usize);

/// Boss slot of the bottom panel (its name and bars).
#[derive(Component)]
struct BossSlot(u8);
#[derive(Component)]
struct BossName(u8);
/// Boss slots: a duo has two.
const BOSS_SLOTS: u8 = 2;

#[derive(Component)]
struct HudRoot;
#[derive(Component)]
struct ItemIcon;
#[derive(Component)]
struct ItemCount;
#[derive(Component)]
struct ItemName;
#[derive(Component)]
struct ItemHint;
#[derive(Component)]
struct SlotPip(u8);
#[derive(Component)]
struct Prompt;
#[derive(Component)]
struct BossPanel;
/// Equipped weapon (icon, name, key to switch) and its markers.
#[derive(Component)]
struct WeaponIcon;
#[derive(Component)]
struct WeaponName;
#[derive(Component)]
struct WeaponHint;
#[derive(Component)]
struct WeaponPip(u8);
#[derive(Component)]
struct Banner;
#[derive(Component)]
struct BannerText;
#[derive(Component)]
struct Overlay;
#[derive(Component)]
struct Fade;
#[derive(Component)]
struct Reticle;
#[derive(Component)]
struct LoadingText;
/// Frames-per-second counter ("Show FPS" option).
#[derive(Component)]
struct FpsText;
/// "Item obtained" popup.
#[derive(Component)]
struct Popup;
#[derive(Component)]
struct PopupIcon;
#[derive(Component)]
struct PopupName;
#[derive(Component)]
struct PopupInfo;
/// Active effects (resin, moss, ash, root).
#[derive(Component)]
struct Effects;
/// Co-op: line of an ally (the n-th other player, by number), their name and health.
#[derive(Component)]
struct AllyRow(u8);
#[derive(Component)]
struct AllyName(u8);
#[derive(Component)]
struct AllyHp(u8);
/// Co-op: connection quality.
#[derive(Component)]
struct NetLine;
/// Co-op: the partner search, or the last notice.
#[derive(Component)]
struct SearchLine;
/// Co-op: the number floating over an ally's head (the player it follows).
#[derive(Component)]
struct AllyTag(Entity);
/// Floating health bar of an enemy (and the enemy it follows).
#[derive(Component)]
struct FoeBar(Entity);
#[derive(Component)]
struct FoeBarFill;

/// Item icons (pixel art generated at startup), also used by the equipment menu.
#[derive(Resource)]
pub struct ItemIcons(HashMap<Item, Handle<Image>>);

impl ItemIcons {
    pub fn get(&self, item: Item) -> Handle<Image> {
        self.0.get(&item).cloned().unwrap_or_default()
    }
}

/// Weapon icons, in the order of `weapons.ron`.
#[derive(Resource)]
pub struct WeaponIcons(Vec<Handle<Image>>);

impl WeaponIcons {
    pub fn get(&self, i: usize) -> Handle<Image> {
        self.0.get(i).cloned().unwrap_or_default()
    }
}

/// Menu icons: equipment (helm), status (scroll) and system (cogwheel) of the pause menu, and
/// the final door's medallion (title screen, tinted in each boss's colour).
#[derive(Resource)]
pub struct MenuIcons {
    pub equipment: Handle<Image>,
    pub status: Handle<Image>,
    pub system: Handle<Image>,
    pub gem: Handle<Image>,
}

/// Picked-up items to announce, one after the other.
#[derive(Resource, Default)]
struct PickupPopup {
    queue: VecDeque<(Item, u8)>,
    current: Option<(Item, u8)>,
    timer: f32,
}

const POPUP_SHOW: f32 = 3.2;

/// Death / victory banner.
#[derive(Resource, Default)]
struct Outcome {
    /// Text (English, French) and colour.
    text: Option<((&'static str, &'static str), Color)>,
    timer: f32,
    /// Display duration (`None`: until the respawn).
    duration: Option<f32>,
    /// Banner shown after this one (the final door opening after the last victory).
    next: Option<((&'static str, &'static str), Color)>,
}

/// Animated values: fade to black.
#[derive(Resource, Default)]
struct HudAnim {
    fade: f32,
    dying: bool,
}

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Outcome>()
            .init_resource::<HudAnim>()
            .init_resource::<PickupPopup>()
            .add_systems(Startup, setup)
            .add_systems(OnExit(AppState::Loading), |mut q: Query<&mut Visibility, With<LoadingText>>| {
                for mut v in &mut q {
                    *v = Visibility::Hidden;
                }
            })
            .add_systems(
                OnEnter(AppState::Playing),
                |mut out: ResMut<Outcome>, mut anim: ResMut<HudAnim>, mut pop: ResMut<PickupPopup>| {
                    *out = Outcome::default();
                    *anim = HudAnim { fade: 1.0, ..default() };
                    *pop = PickupPopup::default();
                },
            )
            .add_systems(Update, (show_root, fps))
            .add_systems(
                Update,
                (
                    update_bars,
                    special_stripes,
                    update_boss,
                    update_item,
                    prompt,
                    outcome,
                    overlay,
                    reticle,
                    update_weapon,
                    popup,
                    effects,
                    foe_bars,
                    allies,
                    ally_tags,
                    search_line,
                )
                    .run_if(in_state(AppState::Playing))
                    .after(crate::fx::consume_events),
            );
    }
}

fn bar(width: f32, height: f32, color: Color, kind: Bar) -> impl Bundle {
    (Node { width: px(width), height: px(height), ..default() }, BackgroundColor(color), kind)
}

/// Size of item icons, in pixels (= dots).
pub const ITEM_ICON: usize = 24;

/// PS1-style icon of `n`×`n` pixels: `shade(x, y)` gives the colour of each pixel (pixel
/// centre, in pixels from the top-left corner) or `None` if it's transparent. Colours
/// are reduced to 15 bits per channel with the same 4×4 dithering as the game; with `outline`,
/// the icon gets a dark one-pixel outline to stand out from the background.
fn ps1_icon(n: usize, outline: bool, shade: impl Fn(f32, f32) -> Option<Vec3>) -> Image {
    const BAYER: [f32; 16] = [0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0];
    let mut data = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            let px = match shade(x as f32 + 0.5, y as f32 + 0.5) {
                Some(c) => {
                    let d = BAYER[(y % 4) * 4 + x % 4] / 16.0 - 0.5;
                    let q = |v: f32| ((v.clamp(0.0, 1.0) * 31.0 + d).round().clamp(0.0, 31.0) / 31.0 * 255.0) as u8;
                    [q(c.x), q(c.y), q(c.z), 255]
                }
                None => [0, 0, 0, 0],
            };
            data.extend(px);
        }
    }
    if outline {
        let alpha = |x: i32, y: i32| (0..n as i32).contains(&x) && (0..n as i32).contains(&y) && data[(y as usize * n + x as usize) * 4 + 3] == 255;
        let edge: Vec<usize> = (0..n * n)
            .filter(|&i| {
                let (x, y) = ((i % n) as i32, (i / n) as i32);
                !alpha(x, y) && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| alpha(x + dx, y + dy))
            })
            .collect();
        for i in edge {
            data[i * 4..i * 4 + 4].copy_from_slice(&[16, 12, 12, 255]);
        }
    }
    Image::new(
        Extent3d { width: n as u32, height: n as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// Healing flask: small "3D" rendering (lit spherical body, highlight, glowing green liquid).
fn flask_icon() -> Image {
    let light = Vec3::new(-0.55, 0.6, 0.6).normalize();
    let (cx, cy, r) = (12.0, 15.0, 8.2);
    ps1_icon(ITEM_ICON, true, |fx, fy| {
        let (dx, dy) = ((fx - cx) / r, (fy - cy) / r);
        let d2 = dx * dx + dy * dy;
        let neck = (fx - cx).abs() <= 2.6 && (3.0..8.5).contains(&fy);
        let lip = (fx - cx).abs() <= 3.4 && (5.0..6.5).contains(&fy);
        let cork = (fx - cx).abs() <= 2.2 && (0.5..4.0).contains(&fy);
        if cork {
            let k = 0.75 + 0.35 * (1.0 - (fx - cx + 1.0).abs() / 3.0) - (fy - 0.5) * 0.06;
            Some(Vec3::new(0.55, 0.36, 0.2) * k)
        } else if lip || (neck && d2 > 1.0) {
            let k = 0.5 + 0.5 * (1.0 - ((fx - cx + 0.8) / 3.4).abs()).max(0.0);
            Some(Vec3::new(0.55, 0.62, 0.66) * k + Vec3::splat(0.08))
        } else if d2 <= 1.0 {
            let n = Vec3::new(dx, -dy, (1.0 - d2).sqrt());
            let diffuse = n.dot(light).max(0.0);
            let spec = n.dot((light + Vec3::Z).normalize()).max(0.0).powf(24.0);
            let rim = (1.0 - n.z).powf(2.0);
            // Liquid below the level line (slight ripple), empty glass above.
            let level = cy - 2.0 + 0.6 * ((fx - cx) * 0.9).sin();
            let base = if fy > level {
                let glow = 0.35 + 0.65 * (1.0 - d2);
                Vec3::new(0.12, 0.62, 0.3) * (0.35 + 0.75 * diffuse) + Vec3::new(0.1, 0.35, 0.12) * glow
            } else {
                Vec3::new(0.28, 0.33, 0.38) * (0.4 + 0.6 * diffuse)
            };
            Some(base + Vec3::new(0.55, 0.65, 0.7) * rim * 0.45 + Vec3::splat(spec * 1.1))
        } else {
            None
        }
    })
}

/// Small lit sphere (base shape of several icons): normal and diffuse factor.
fn lit(dx: f32, dy: f32) -> Option<(Vec3, f32)> {
    let d2 = dx * dx + dy * dy;
    (d2 <= 1.0).then(|| {
        let n = Vec3::new(dx, -dy, (1.0 - d2).sqrt());
        (n, n.dot(Vec3::new(-0.55, 0.6, 0.6).normalize()).max(0.0))
    })
}

/// Golden moss: tuft of green strands with golden tips.
fn moss_icon() -> Image {
    const BLADES: [(f32, f32, f32); 7] = [(5.0, 2.0, 7.0), (8.0, 6.5, 3.0), (11.0, 11.5, 1.0), (14.0, 16.0, 2.5), (17.0, 20.5, 5.0), (9.5, 4.0, 5.0), (15.0, 18.0, 4.0)];
    ps1_icon(ITEM_ICON, true, |fx, fy| {
        if fy > 15.0 {
            let (dx, dy) = ((fx - 12.0) / 9.0, (fy - 19.0) / 4.5);
            let (_, diffuse) = lit(dx, dy)?;
            return Some(Vec3::new(0.18, 0.32, 0.1) * (0.5 + 0.8 * diffuse));
        }
        for (bx, tx, ty) in BLADES {
            let k = ((fy - ty) / (17.0 - ty)).clamp(0.0, 1.0);
            if fy < ty {
                continue;
            }
            let x = tx + (bx - tx) * k;
            if (fx - x).abs() <= 0.6 + 0.9 * k {
                let tip = 1.0 - k;
                return Some(Vec3::new(0.25, 0.55, 0.15).lerp(Vec3::new(0.95, 0.8, 0.3), tip * tip));
            }
        }
        None
    })
}

/// Ember resin: small clay pot, amber resin overflowing.
fn resin_icon() -> Image {
    ps1_icon(ITEM_ICON, true, |fx, fy| {
        let (cx, w) = (12.0, 7.5 - (fy - 14.0).abs() * 0.12);
        if (4.5..8.0).contains(&fy) && (fx - cx).abs() <= 6.0 - (fy - 4.5) * 0.3 {
            let k = 0.7 + 0.3 * (1.0 - (fx - cx + 2.0).abs() / 6.0);
            return Some(Vec3::new(1.0, 0.62, 0.15) * k);
        }
        if (8.0..21.0).contains(&fy) && (fx - cx).abs() <= w {
            let shade = 0.55 + 0.45 * (1.0 - ((fx - cx + 2.5) / w).abs()).max(0.0);
            let drip = (fx - 9.0).abs() < 1.0 && fy < 12.0 + (fx * 3.0).sin().abs() * 3.0;
            let base = if drip { Vec3::new(0.95, 0.55, 0.12) } else { Vec3::new(0.48, 0.3, 0.2) };
            return Some(base * shade);
        }
        None
    })
}

/// Warding ash: a small heap of pale ash, a few silvery glints of protection rising from it.
fn ash_icon() -> Image {
    ps1_icon(ITEM_ICON, true, |fx, fy| {
        match (fx as i32, fy as i32) {
            (7, 5) | (16, 3) | (12, 7) => return Some(Vec3::new(0.75, 0.88, 1.0)),
            (9, 2) | (18, 8) => return Some(Vec3::new(0.5, 0.65, 0.85)),
            _ => {}
        }
        let (dx, dy) = ((fx - 12.0) / 9.5, (fy - 20.0) / 9.0);
        if dy > 0.12 {
            return None;
        }
        let (_, diffuse) = lit(dx, dy)?;
        let speck = ((fx * 2.7).sin() * (fy * 3.1).cos()).abs() > 0.82;
        let ash = if speck { Vec3::new(0.3, 0.29, 0.3) } else { Vec3::new(0.68, 0.67, 0.66) };
        Some(ash * (0.5 + 0.7 * diffuse))
    })
}

/// Vigor root: a gnarled brown root, two green sprouts at its top.
fn root_icon() -> Image {
    ps1_icon(ITEM_ICON, true, |fx, fy| {
        // Sprouts: two small leaves at the top of the root.
        for (cx, cy, sx) in [(10.0, 4.5, -1.0), (14.0, 4.0, 1.0)] {
            let (u, v) = ((fx - cx) * sx, fy - cy);
            if (u - v * 0.4).abs() * 1.8 + v.abs() < 2.6 {
                return Some(Vec3::new(0.35, 0.72, 0.2) * (0.8 + 0.2 * sx));
            }
        }
        // Main stem: a wavy line from the top down to the bottom left, thinning out, and a
        // side rootlet to the right.
        let axis = 12.0 + (fy * 0.55).sin() * 2.2 - (fy - 7.0).max(0.0) * 0.25;
        let half = (3.2 - (fy - 7.0).max(0.0) * 0.17).max(0.6);
        let side = (fy - 13.0 - (fx - 13.0) * 0.6).abs() < 1.0 && (13.0..20.0).contains(&fx);
        if ((6.0..22.0).contains(&fy) && (fx - axis).abs() <= half) || side {
            let light = if side { 0.6 } else { (0.75 - 0.35 * (fx - axis) / half).clamp(0.3, 1.0) };
            let ring = ((fy * 1.3).sin() > 0.85) as i32 as f32;
            return Some(Vec3::new(0.48, 0.3, 0.16) * light * (1.0 - 0.3 * ring));
        }
        None
    })
}

/// Flask shard: sharp piece of green glass.
fn shard_icon() -> Image {
    ps1_icon(ITEM_ICON, true, |fx, fy| {
        let (u, v) = (fx - 12.0, fy - 12.0);
        let inside = v > -10.0 && u.abs() * 1.6 + (v + 2.0).abs() * 0.55 < 7.0 && v < 9.0 - u * 0.5;
        inside.then(|| {
            let facet = if u + v * 0.3 > 0.0 { 0.65 } else { 1.0 };
            let edge = ((u * 1.6).abs() + (v + 2.0).abs() * 0.55 > 5.8) as i32 as f32;
            Vec3::new(0.35, 0.8, 0.5) * facet + Vec3::splat(0.35 * edge)
        })
    })
}

/// Iron brooch: disc of hammered metal, dark cabochon and pin.
fn brooch_icon() -> Image {
    ps1_icon(ITEM_ICON, true, |fx, fy| {
        if (fx - fy - 1.0).abs() < 0.8 && (3.0..21.0).contains(&fx) && !(6.0..18.0).contains(&fx) {
            return Some(Vec3::splat(0.75));
        }
        let (dx, dy) = ((fx - 12.0) / 8.0, (fy - 12.0) / 8.0);
        let (n, diffuse) = lit(dx, dy)?;
        let spec = n.dot((Vec3::new(-0.55, 0.6, 0.6).normalize() + Vec3::Z).normalize()).max(0.0).powf(16.0);
        if dx * dx + dy * dy < 0.18 {
            return Some(Vec3::new(0.45, 0.08, 0.08) * (0.5 + diffuse) + Vec3::splat(spec));
        }
        let ring = (dx * dx + dy * dy - 0.55).abs() < 0.06;
        Some(Vec3::splat(if ring { 0.25 } else { 0.42 }) * (0.5 + 0.8 * diffuse) + Vec3::splat(spec * 0.8))
    })
}

/// Crest plume: diagonal feather, striped red and white.
fn feather_icon() -> Image {
    ps1_icon(ITEM_ICON, true, |fx, fy| {
        // Feather axis: from the bottom-left corner towards the top right.
        let k = std::f32::consts::FRAC_1_SQRT_2;
        let (u, v) = ((fx - fy) * k, (fx + fy - 24.0) * k);
        let half = (1.0 - (v / 10.0) * (v / 10.0)).max(0.0) * 4.2 - if v < -6.0 { 4.0 } else { 0.0 };
        if u.abs() < 0.6 && (-11.0..10.0).contains(&v) {
            return Some(Vec3::new(0.9, 0.82, 0.6));
        }
        (u.abs() < half && v > -6.0).then(|| {
            let stripe = ((v + u * 0.6) / 2.6).floor() as i32 % 2 == 0;
            let c = if stripe { Vec3::new(0.75, 0.14, 0.12) } else { Vec3::new(0.92, 0.86, 0.72) };
            c * (0.75 + 0.25 * (u / half.max(0.1)))
        })
    })
}

/// Diagonal weapon, guard at the bottom left, point at the top right. `t`: distance along
/// the weapon from the pommel, `v`: offset from the axis (positive: lit side). The rapier has a
/// thin blade and a golden shell guard, the greatsword a wide fullered blade and a long
/// straight iron guard.
fn weapon_icon(index: usize) -> Image {
    let k = std::f32::consts::FRAC_1_SQRT_2;
    let rapier = index == 0;
    let steel = |v: f32, w: f32| {
        let lit = (0.5 + 0.5 * v / w.max(0.1)).clamp(0.0, 1.0);
        Vec3::new(0.38, 0.4, 0.46).lerp(Vec3::new(0.9, 0.92, 0.95), lit)
    };
    ps1_icon(ITEM_ICON, true, move |fx, fy| {
        let (t, v) = ((fx - 2.0 + (22.0 - fy)) * k, (fx - 2.0 - (22.0 - fy)) * -k);
        let leather = |t: f32| {
            let wrap = (t * 2.2).fract() < 0.5;
            Vec3::new(0.36, 0.2, 0.12) * if wrap { 1.0 } else { 0.7 }
        };
        let pommel = (t - 1.3).powi(2) + v * v;
        if rapier {
            let gold = Vec3::new(0.82, 0.62, 0.26);
            if pommel < 1.6 {
                return Some(gold * 1.1);
            }
            if (2.2..5.0).contains(&t) && v.abs() < 0.8 {
                return Some(leather(t));
            }
            // Shell and quillons.
            if (5.0..6.2).contains(&t) && v.abs() < 3.4 {
                return Some(gold * (0.8 + 0.1 * v));
            }
            // Knuckle bow: an arc curving back towards the pommel.
            let bow = ((t - 4.2).powi(2) + (v - 2.2).powi(2)).sqrt();
            if (bow - 2.0).abs() < 0.45 && v > 1.0 {
                return Some(gold * 0.85);
            }
            let w = 0.85 * (1.0 - ((t - 6.2) / 21.0).max(0.0)).max(0.25);
            if (6.2..27.5).contains(&t) && v.abs() < w {
                return Some(steel(v, w));
            }
        } else {
            let iron = Vec3::new(0.34, 0.32, 0.32);
            if pommel < 2.0 {
                return Some(iron * 1.3);
            }
            if (2.3..6.2).contains(&t) && v.abs() < 0.95 {
                return Some(leather(t));
            }
            if (6.2..7.6).contains(&t) && v.abs() < 5.0 {
                return Some(iron * (1.0 + 0.15 * v.signum()));
            }
            let w = if t < 24.0 { 2.1 } else { 2.1 * (27.5 - t) / 3.5 };
            if (7.6..27.5).contains(&t) && v.abs() < w {
                // Dark fuller in the middle of the blade.
                let fuller = v.abs() < 0.5 && t < 22.0;
                return Some(steel(v, w) * if fuller { 0.6 } else { 1.0 });
            }
        }
        None
    })
}

/// Cogwheel (system): eight teeth, pierced hub, steel lit from the top left.
fn gear_icon() -> Image {
    ps1_icon(ITEM_ICON, true, |fx, fy| {
        let (dx, dy) = (fx - 12.0, fy - 12.0);
        let r = (dx * dx + dy * dy).sqrt();
        let a = dy.atan2(dx);
        // Teeth: crenels around the rim (a bit narrower at the top).
        let tooth = ((a * 8.0 / std::f32::consts::TAU + 0.25).rem_euclid(1.0) - 0.5).abs() < 0.22;
        let outer = if tooth { 10.5 } else { 8.0 };
        if r > outer || r < 3.2 {
            return None;
        }
        let light = (0.55 - 0.45 * (dx - dy) / 17.0).clamp(0.0, 1.0);
        let base = Vec3::new(0.42, 0.42, 0.46).lerp(Vec3::new(0.86, 0.86, 0.9), light);
        // Groove between the rim and the hub.
        Some(if (r - 5.6).abs() < 0.7 { base * 0.55 } else { base })
    })
}

/// Scroll (status): a parchment unrolled between two wooden rods, with lines of writing.
fn scroll_icon() -> Image {
    ps1_icon(ITEM_ICON, true, |fx, fy| {
        // The rods, top and bottom, a little wider than the sheet.
        for rod in [4.0, 20.0] {
            if (fy - rod).abs() < 1.6 && (3.0..21.0).contains(&fx) {
                let light = (0.75 - 0.35 * (fy - rod)).clamp(0.0, 1.0);
                return Some(Vec3::new(0.3, 0.16, 0.07).lerp(Vec3::new(0.62, 0.38, 0.18), light));
            }
        }
        if !(5.0..19.0).contains(&fx) || !(5.5..18.5).contains(&fy) {
            return None;
        }
        // Lines of writing (the last one shorter), darker at the edges of the sheet.
        let line = ((fy - 7.5) / 2.5).fract() < 0.35 && fy < 17.0 && (7.0..17.0).contains(&fx) && !(fy > 14.5 && fx > 13.0);
        if line {
            return Some(Vec3::new(0.3, 0.2, 0.12));
        }
        let edge = ((fx - 12.0).abs() / 7.0).powi(4);
        Some(Vec3::new(0.9, 0.82, 0.62).lerp(Vec3::new(0.62, 0.5, 0.34), edge))
    })
}

/// Size of the medallion icon, in pixels.
pub const GEM_ICON: usize = 15;

/// Medallion of the final door (a square standing on a corner), in grey: tinted by the
/// boss's colour. Facets lit from the top left, like the door's in the light.
fn gem_icon() -> Image {
    ps1_icon(GEM_ICON, true, |fx, fy| {
        let c = GEM_ICON as f32 / 2.0;
        let (dx, dy) = (fx - c, fy - c);
        if dx.abs() + dy.abs() > c - 1.0 {
            return None;
        }
        // Flat central table, bevels around it.
        let shade = if dx.abs() + dy.abs() < c * 0.45 {
            0.85
        } else {
            match (dx < 0.0, dy < 0.0) {
                (true, true) => 1.0,
                (false, true) => 0.78,
                (true, false) => 0.62,
                (false, false) => 0.45,
            }
        };
        Some(Vec3::splat(shade))
    })
}

/// Helm (equipment): iron helmet with a visor slit and a red crest.
fn helm_icon() -> Image {
    ps1_icon(ITEM_ICON, true, |fx, fy| {
        let (dx, dy) = ((fx - 12.0) / 8.0, (fy - 13.0) / 9.0);
        // Crest on top.
        if (fx - 12.0).abs() < 1.3 && (1.0..5.0).contains(&fy) {
            return Some(Vec3::new(0.72, 0.14, 0.1) * (1.1 - (fy - 1.0) * 0.08));
        }
        // Dome at the top, straight cheeks at the bottom.
        let inside = if dy < 0.0 { dx * dx + dy * dy <= 1.0 } else { dx.abs() <= 1.0 - dy * 0.15 && dy <= 1.0 };
        if !inside {
            return None;
        }
        let light = (0.6 - 0.5 * dx - 0.3 * dy).clamp(0.0, 1.0);
        let iron = Vec3::new(0.32, 0.32, 0.36).lerp(Vec3::new(0.82, 0.82, 0.86), light);
        // Visor slit, and rivets on either side.
        if (12.5..14.0).contains(&fy) && (6.0..18.0).contains(&fx) {
            return Some(Vec3::splat(0.05));
        }
        if (fy - 18.0).abs() < 0.8 && ((fx - 8.0).abs() < 0.8 || (fx - 16.0).abs() < 0.8) {
            return Some(iron * 0.5);
        }
        Some(iron)
    })
}

fn setup(mut commands: Commands, tuning: Res<Tuning>, font: Res<UiFont>, mut images: ResMut<Assets<Image>>) {
    let icons = ItemIcons(
        Item::ALL
            .into_iter()
            .map(|it| {
                let img = match it {
                    Item::HealFlask => flask_icon(),
                    Item::GoldenMoss => moss_icon(),
                    Item::EmberResin => resin_icon(),
                    Item::WardingAsh => ash_icon(),
                    Item::VigorRoot => root_icon(),
                    Item::FlaskShard => shard_icon(),
                    Item::IronBrooch => brooch_icon(),
                    Item::CrestPlume => feather_icon(),
                };
                (it, images.add(img))
            })
            .collect(),
    );
    let weapon_icons = WeaponIcons((0..tuning.weapons.len()).map(|i| images.add(weapon_icon(i))).collect());
    let p = &tuning.player;
    commands.spawn((
        font.text("", 1, TEXT),
        Localized::tr("Loading…", "Chargement…"),
        Node { position_type: PositionType::Absolute, left: percent(45), top: percent(45), ..default() },
        LoadingText,
    ));
    // FPS, top right, visible everywhere (menus included) when the option is on.
    commands.spawn((
        font.text("", 1, TEXT),
        Node { position_type: PositionType::Absolute, right: px(28), top: px(24), ..default() },
        GlobalZIndex(100),
        Visibility::Hidden,
        FpsText,
    ));
    let root = commands
        .spawn((
            HudRoot,
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
            Visibility::Hidden,
        ))
        .id();

    // Player bars, top left.
    commands
        .spawn((
            ChildOf(root),
            Node {
                position_type: PositionType::Absolute,
                left: px(28),
                top: px(24),
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                ..default()
            },
        ))
        .with_children(|c| {
            c.spawn((
                Node { width: px(p.max_hp * HP_PX + 4.0), height: px(14), padding: UiRect::all(px(2)), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
                Bar::HpFrame,
            ))
            .with_children(|c| {
                c.spawn(bar(0.0, 10.0, Color::srgb(0.72, 0.1, 0.08), Bar::Hp));
                c.spawn(bar(0.0, 10.0, Color::srgb(0.55, 0.52, 0.5), Bar::Regain));
            });
            c.spawn((
                Node { width: px(p.max_stamina * ST_PX + 4.0), height: px(9), padding: UiRect::all(px(2)), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
                Bar::StaminaFrame,
            ))
            .with_children(|c| {
                c.spawn(bar(0.0, 5.0, Color::srgb(0.3, 0.62, 0.25), Bar::Stamina));
            });
            c.spawn((
                Node { width: px(p.special_max * SP_PX + 4.0), height: px(9), padding: UiRect::all(px(2)), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
                Bar::SpecialFrame,
            ))
            .with_children(|c| {
                c.spawn((Node { width: px(0), height: px(5), overflow: Overflow::clip(), ..default() }, BackgroundColor(SPECIAL), Bar::Special))
                    .with_children(|c| {
                        let n = (p.special_max * SP_PX / STRIPE_PERIOD).ceil() as usize + 1;
                        for i in 0..n {
                            c.spawn((
                                Node { position_type: PositionType::Absolute, width: px(STRIPE_PERIOD / 2.0), height: px(5), ..default() },
                                BackgroundColor(STRIPE_LIGHT),
                                Visibility::Hidden,
                                SpecialStripe(i),
                            ));
                        }
                    });
            });
            c.spawn((font.text("", 1, GOLD), Effects));
            // Co-op: the allies' health, then the connection.
            for slot in 0..MAX_PLAYERS as u8 - 1 {
                c.spawn((Node { column_gap: px(6), align_items: AlignItems::Center, display: Display::None, ..default() }, AllyRow(slot)))
                    .with_children(|c| {
                        c.spawn((font.text("", 1, GOLD), AllyName(slot)));
                        c.spawn((
                            Node { width: px(p.max_hp * ALLY_HP_PX + 4.0), height: px(8), padding: UiRect::all(px(2)), ..default() },
                            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
                        ))
                        .with_children(|c| {
                            c.spawn((Node { width: px(0), height: px(4), ..default() }, BackgroundColor(Color::srgb(0.72, 0.1, 0.08)), AllyHp(slot)));
                        });
                    });
            }
            c.spawn((font.text("", 1, Color::srgb(0.7, 0.75, 0.8)), NetLine));
        });

    // Co-op: the search for a partner (time left, how to give up), or why it stopped.
    commands
        .spawn((
            ChildOf(root),
            Node { position_type: PositionType::Absolute, width: percent(100), bottom: px(100), justify_content: JustifyContent::Center, ..default() },
        ))
        .with_children(|c| {
            c.spawn((font.text("", 1, GOLD), SearchLine));
        });

    // Equipped weapon and quick item, bottom left: a box with the icon, and next to it the name,
    // the slot markers and the key to switch.
    let slot_box = |c: &mut ChildSpawnerCommands, icon: Handle<Image>, marker: Option<ItemIcon>| {
        c.spawn((
            Node {
                border: UiRect::all(px(2)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            PixelSize(UVec2::splat(32)),
            BorderColor::all(Color::srgb(0.62, 0.55, 0.42)),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        ))
        .with_children(|c| match marker {
            Some(m) => {
                c.spawn((image_bundle(icon, UVec2::splat(ITEM_ICON as u32)), m));
                c.spawn((
                    font.text("", 1, Color::srgb(0.95, 0.92, 0.85)),
                    Node { position_type: PositionType::Absolute, right: px(3), bottom: px(0), ..default() },
                    ItemCount,
                ));
            }
            None => {
                c.spawn((image_bundle(icon, UVec2::splat(ITEM_ICON as u32)), WeaponIcon));
            }
        });
    };
    let row = || Node { flex_direction: FlexDirection::Row, align_items: AlignItems::FlexEnd, column_gap: px(10), ..default() };
    let info = || Node { flex_direction: FlexDirection::Column, row_gap: px(3), ..default() };
    let pips = || Node { flex_direction: FlexDirection::Row, column_gap: px(3), ..default() };
    commands
        .spawn((
            ChildOf(root),
            Node {
                position_type: PositionType::Absolute,
                left: px(28),
                bottom: px(26),
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                ..default()
            },
        ))
        .with_children(|c| {
            c.spawn(row()).with_children(|c| {
                slot_box(c, weapon_icons.0.first().cloned().unwrap_or_default(), None);
                c.spawn(info()).with_children(|c| {
                    c.spawn(pips()).with_children(|c| {
                        for i in 0..tuning.weapons.len() as u8 {
                            c.spawn((Node { width: px(10), height: px(4), ..default() }, BackgroundColor(Color::NONE), WeaponPip(i)));
                        }
                    });
                    c.spawn((font.text("", 1, TEXT), WeaponName));
                    c.spawn((Hint::new(1, Color::srgba(0.85, 0.82, 0.75, 0.8)), hint_node(), WeaponHint));
                });
            });
            c.spawn(row()).with_children(|c| {
                slot_box(c, icons.get(Item::HealFlask), Some(ItemIcon));
                c.spawn(info()).with_children(|c| {
                    c.spawn(pips()).with_children(|c| {
                        for i in 0..QUICK_SLOTS as u8 {
                            c.spawn((Node { width: px(10), height: px(4), ..default() }, BackgroundColor(Color::NONE), SlotPip(i)));
                        }
                    });
                    c.spawn((font.text("", 1, TEXT), ItemName));
                    c.spawn((Hint::new(1, Color::srgba(0.85, 0.82, 0.75, 0.8)), hint_node(), ItemHint));
                });
            });
        });
    // Item obtained: icon, name and description, above the interaction prompt.
    commands
        .spawn((
            ChildOf(root),
            Node {
                position_type: PositionType::Absolute,
                bottom: px(190),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
            Popup,
        ))
        .with_children(|c| {
            c.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    padding: UiRect::axes(px(16), px(8)),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BorderColor::all(Color::srgb(0.42, 0.37, 0.3)),
                BackgroundColor(Color::srgba(0.03, 0.025, 0.02, 0.82)),
            ))
            .with_children(|c| {
                c.spawn((image_bundle(icons.get(Item::GoldenMoss), UVec2::splat(ITEM_ICON as u32)), PopupIcon));
                c.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(3), ..default() }).with_children(|c| {
                    c.spawn((font.text("", 1, Color::srgb(0.95, 0.92, 0.85)), PopupName));
                    c.spawn((font.text("", 1, Color::srgb(0.7, 0.67, 0.6)), PopupInfo));
                });
            });
        });
    commands.insert_resource(icons);
    commands.insert_resource(weapon_icons);
    commands.insert_resource(MenuIcons {
        equipment: images.add(helm_icon()),
        status: images.add(scroll_icon()),
        system: images.add(gear_icon()),
        gem: images.add(gem_icon()),
    });

    // Interaction prompt, above the boss panel.
    commands
        .spawn((
            ChildOf(root),
            Node {
                position_type: PositionType::Absolute,
                bottom: px(150),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
            Prompt,
        ))
        .with_children(|c| {
            c.spawn((
                Hint::new(1, Color::srgb(0.95, 0.92, 0.85)),
                Node { padding: UiRect::axes(px(18), px(6)), ..hint_node() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            ));
        });

    // Boss, bottom centre.
    commands
        .spawn((
            ChildOf(root),
            Node {
                position_type: PositionType::Absolute,
                bottom: px(48),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BossPanel,
            Visibility::Hidden,
        ))
        .with_children(|c| {
            c.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(10), width: px(620), ..default() }).with_children(|c| {
                for slot in 0..BOSS_SLOTS {
                    c.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(4), width: px(620), ..default() }, BossSlot(slot)))
                        .with_children(|c| {
                            c.spawn((font.text("", 1, Color::srgb(0.92, 0.88, 0.8)), BossName(slot)));
                            c.spawn((Node { width: px(620), height: px(12), padding: UiRect::all(px(2)), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7))))
                                .with_children(|c| {
                                    c.spawn((Node { width: percent(100), height: px(8), ..default() }, BackgroundColor(Color::srgb(0.68, 0.08, 0.06)), Bar::BossHp(slot)));
                                });
                            // Stagger gauge: tracked but hidden (you guess it from the boss's reaction).
                            c.spawn((Node { width: px(620), height: px(6), padding: UiRect::all(px(1)), display: Display::None, ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6))))
                                .with_children(|c| {
                                    c.spawn((Node { width: percent(0), height: px(4), ..default() }, BackgroundColor(Color::srgb(0.95, 0.9, 0.75)), Bar::BossStagger(slot)));
                                });
                        });
                }
            });
        });

    // Full-screen flash (perfect guard / rage), then fade to black.
    commands.spawn((
        ChildOf(root),
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
        BackgroundColor(Color::NONE),
        Overlay,
    ));
    commands.spawn((
        ChildOf(root),
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
        BackgroundColor(Color::NONE),
        Fade,
    ));

    // Death / victory banner.
    commands
        .spawn((
            ChildOf(root),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                top: percent(40),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(6),
                padding: UiRect::axes(px(0), px(22)),
                ..default()
            },
            // Dark strip across the whole width, Dark Souls style.
            BackgroundColor(Color::NONE),
            Visibility::Hidden,
            Banner,
        ))
        .with_children(|c| {
            c.spawn((font.text("", 3, Color::srgb(0.75, 0.12, 0.08)), BannerText));
        });

    commands.spawn((
        ChildOf(root),
        Node { position_type: PositionType::Absolute, width: px(6), height: px(6), ..default() },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
        Visibility::Hidden,
        Reticle,
    ));
}

/// The HUD is only visible in game (not on the title screen).
fn show_root(state: Res<State<AppState>>, mut q: Query<&mut Visibility, With<HudRoot>>) {
    let want = if *state.get() == AppState::Playing { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut q {
        if *v != want {
            *v = want;
        }
    }
}

/// Average over half a second, so the number stays readable.
fn fps(
    time: Res<Time<Real>>,
    settings: Res<crate::settings::Settings>,
    mut q: Query<(&mut Text, &mut Visibility), With<FpsText>>,
    mut acc: Local<(f32, u32)>,
) {
    let Ok((mut text, mut vis)) = q.single_mut() else { return };
    let want = if settings.show_fps { Visibility::Inherited } else { Visibility::Hidden };
    if *vis != want {
        *vis = want;
    }
    acc.0 += time.delta_secs();
    acc.1 += 1;
    if acc.0 >= 0.5 {
        set_text(&mut text, format!("{:.0} FPS", acc.1 as f32 / acc.0));
        *acc = (0.0, 0);
    }
}

fn set_text(t: &mut Text, s: impl AsRef<str>) {
    if t.0 != s.as_ref() {
        t.0 = s.as_ref().into();
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_item(
    players: Query<&Player, With<LocalPlayer>>,
    device: Res<Device>,
    icons: Res<ItemIcons>,
    mut icon: Query<(&mut ImageNode, &mut Visibility), With<ItemIcon>>,
    mut count: Query<&mut Text, (With<ItemCount>, Without<ItemName>, Without<ItemHint>)>,
    mut name: Query<&mut Text, (With<ItemName>, Without<ItemCount>, Without<ItemHint>)>,
    mut hint: Query<&mut Hint, With<ItemHint>>,
    mut pips: Query<(&SlotPip, &mut BackgroundColor)>,
) {
    let Ok(p) = players.single() else { return };
    let inv = &p.inventory;
    let item = inv.active_item();
    let n = item.map_or(0, |i| inv.count(i));
    for (mut img, mut vis) in &mut icon {
        *vis = if item.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if let Some(i) = item {
            let h = icons.get(i);
            if img.image != h {
                img.image = h;
            }
            img.color = if n > 0 { Color::WHITE } else { Color::srgb(0.35, 0.35, 0.35) };
        }
    }
    for mut t in &mut count {
        set_text(&mut t, if item.is_some() { n.to_string() } else { String::new() });
    }
    for mut t in &mut name {
        set_text(&mut t, item.map_or("—", Item::name));
    }
    let (use_key, next_key) =
        if *device == Device::Gamepad { (Glyph::PadX, Glyph::DpadDown) } else { (Glyph::Key("R"), Glyph::Key("↓")) };
    let equipped = inv.slots.iter().filter(|s| s.is_some()).count();
    for mut h in &mut hint {
        let mut segs = vec![i(use_key), t(tr("use", "utiliser"))];
        if equipped > 1 {
            segs.extend([i(next_key), t(tr("next", "suivant"))]);
        }
        set_hint(&mut h, segs);
    }
    for (pip, mut bg) in &mut pips {
        let i = pip.0 as usize;
        bg.0 = if i == inv.active as usize {
            Color::srgb(0.95, 0.85, 0.55)
        } else if inv.slots[i].is_some() {
            Color::srgba(0.85, 0.82, 0.75, 0.6)
        } else {
            Color::srgba(0.85, 0.82, 0.75, 0.15)
        };
    }
}

#[allow(clippy::too_many_arguments)]
fn prompt(
    tuning: Res<Tuning>,
    device: Res<Device>,
    enc: Res<Encounter>,
    menu: Res<MenuState>,
    net: Res<crate::net::Net>,
    players: Query<(&Player, &Body, &Health), With<LocalPlayer>>,
    loot: Query<(Entity, &Loot)>,
    mut root: Query<(&mut Visibility, &Children), With<Prompt>>,
    mut hints: Query<&mut Hint>,
) {
    // Picking up takes priority over resting (same button). (text, with the key?)
    // Looking for a co-op partner: nothing to do but wait (and a notice takes the spot).
    let label = players.single().ok().filter(|(p, _, h)| !h.dead() && !net.searching() && net.notice_for <= 0.0 && matches!(p.state, PState::Free | PState::Guard)).and_then(
        |(p, b, _)| {
            if near_loot(loot.iter(), b.pos).is_some() || near_pickup(&tuning, p.picked, b.pos).is_some() {
                return Some((tr("Pick up", "Ramasser"), true));
            }
            if let Some(cp) = near_checkpoint(&tuning, b.pos).filter(|_| !enc.active) {
                return Some(if enc.hunted {
                    (tr("Enemies nearby: cannot rest", "Des ennemis rôdent : impossible de se reposer"), false)
                } else if p.found & (1 << cp) == 0 {
                    (tr("Kindle the brazier", "Ranimer le brasier"), true)
                } else {
                    (tr("Rest", "Se reposer"), true)
                });
            }
            match p.zone {
                Zone::Level => {
                    if let Some(i) = encounter::near_gate(&tuning, b.pos) {
                        return Some(if enc.is_defeated(i) {
                            (tr("The way is barred", "Le passage est barré"), false)
                        } else {
                            (tr("Go through the fog", "Traverser la brume"), true)
                        });
                    }
                    encounter::near_torch(&tuning, b.pos)
                        .is_some_and(|i| enc.is_defeated(i))
                        .then_some((tr("Rekindle the torch", "Raviver la torche"), true))
                }
                Zone::Arena(i) => {
                    (!enc.active && encounter::near_door(&tuning, i as usize, b.pos)).then_some((tr("Leave through the fog", "Repartir par la brume"), true))
                }
            }
        },
    );
    let show = !menu.open && label.is_some();
    for (mut vis, children) in &mut root {
        *vis = if show { Visibility::Inherited } else { Visibility::Hidden };
        for c in children {
            if let (Ok(mut h), Some((l, key))) = (hints.get_mut(*c), label) {
                let glyph = if *device == Device::Gamepad { Glyph::PadA } else { Glyph::Key("E") };
                set_hint(&mut h, if key { vec![i(glyph), t(l)] } else { vec![t(l)] });
            }
        }
    }
}

fn update_bars(
    tuning: Res<Tuning>,
    fx: Res<FxState>,
    players: Query<(&Player, &Health), With<LocalPlayer>>,
    mut bars: Query<(&Bar, &mut Node, &mut BackgroundColor)>,
) {
    let Ok((p, hp)) = players.single() else { return };
    let hp_scale = HP_PX;
    for (b, mut n, mut bg) in &mut bars {
        match b {
            // Stamina: orange while you can't act, red frame if an action is refused.
            Bar::Stamina => {
                bg.0 = if p.can_act() { Color::srgb(0.3, 0.62, 0.25) } else { Color::srgb(0.75, 0.42, 0.12) };
            }
            Bar::StaminaFrame => {
                bg.0 = Color::srgba(0.1 + 0.7 * fx.no_stamina, 0.0, 0.0, 0.7);
            }
            _ => {}
        }
        n.width = match b {
            Bar::Hp => px(hp.cur.max(0.0) * hp_scale),
            Bar::Regain => px(p.regain.min(hp.max - hp.cur).max(0.0) * hp_scale),
            Bar::HpFrame => px(hp.max * hp_scale + 4.0),
            Bar::Stamina => px(p.stamina.max(0.0) * ST_PX),
            Bar::StaminaFrame => px(tuning.player.max_stamina * ST_PX + 4.0),
            Bar::Special => px(p.special.max(0.0) * SP_PX),
            Bar::SpecialFrame => px(p.special_max(&tuning) * SP_PX + 4.0),
            _ => continue,
        };
    }
}

/// Special ready (full gauge): the stripes scroll over it.
fn special_stripes(
    tuning: Res<Tuning>,
    time: Res<Time>,
    players: Query<&Player, With<LocalPlayer>>,
    mut stripes: Query<(&SpecialStripe, &mut Node, &mut Visibility), Without<Bar>>,
) {
    let Ok(p) = players.single() else { return };
    let ready = p.special_ready(&tuning);
    let offset = (time.elapsed_secs() * STRIPE_SPEED) % STRIPE_PERIOD;
    for (s, mut n, mut vis) in &mut stripes {
        *vis = if ready { Visibility::Inherited } else { Visibility::Hidden };
        n.left = px(s.0 as f32 * STRIPE_PERIOD + offset - STRIPE_PERIOD);
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_weapon(
    tuning: Res<Tuning>,
    device: Res<Device>,
    icons: Res<WeaponIcons>,
    players: Query<&Player, With<LocalPlayer>>,
    mut icon: Query<&mut ImageNode, With<WeaponIcon>>,
    mut name: Query<&mut Text, With<WeaponName>>,
    mut hint: Query<&mut Hint, With<WeaponHint>>,
    mut pips: Query<(&WeaponPip, &mut BackgroundColor)>,
) {
    let Ok(p) = players.single() else { return };
    let w = p.weapon as usize;
    for mut img in &mut icon {
        let h = icons.0.get(w).cloned().unwrap_or_default();
        if img.image != h {
            img.image = h;
        }
    }
    for mut t in &mut name {
        set_text(&mut t, tuning.weapons[w].name.get());
    }
    let key = if *device == Device::Gamepad { Glyph::DpadRight } else { Glyph::Key("1/2") };
    for mut h in &mut hint {
        set_hint(&mut h, if tuning.weapons.len() > 1 { vec![i(key), t(tr("switch", "changer"))] } else { vec![] });
    }
    for (pip, mut bg) in &mut pips {
        let c = if pip.0 as usize == w { Color::srgb(0.95, 0.85, 0.55) } else { Color::srgba(0.85, 0.82, 0.75, 0.6) };
        if bg.0 != c {
            bg.0 = c;
        }
    }
}

/// A boss's health bar: in its colour (that of its torch and its spells), a little darker.
fn boss_bar_color([r, g, b]: [f32; 3]) -> Color {
    Color::srgb(r * 0.82, g * 0.82, b * 0.82)
}

#[allow(clippy::type_complexity)]
fn update_boss(
    enc: Res<Encounter>,
    bosses: Query<(Entity, &Boss, &Health)>,
    tuning: Res<Tuning>,
    mut panel: Query<&mut Visibility, (With<BossPanel>, Without<BossSlot>)>,
    mut slots: Query<(&BossSlot, &mut Visibility, &mut Node), Without<Bar>>,
    mut names: Query<(&BossName, &mut Text)>,
    mut bars: Query<(&Bar, &mut Node, &mut BackgroundColor), Without<BossSlot>>,
) {
    // The panel only appears during the fight.
    for mut v in &mut panel {
        *v = if enc.active { Visibility::Inherited } else { Visibility::Hidden };
    }
    // The leads, in creation order (that of the encounter).
    let mut main: Vec<_> = bosses.iter().filter(|(_, b, _)| !b.def(&tuning).minor).collect();
    main.sort_by_key(|(_, b, _)| b.def);
    for (slot, mut v, mut n) in &mut slots {
        let on = (slot.0 as usize) < main.len();
        let want = if on { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
        let d = if on { Display::Flex } else { Display::None };
        if n.display != d {
            n.display = d;
        }
    }
    for (name, mut t) in &mut names {
        if let Some((_, b, _)) = main.get(name.0 as usize) {
            set_text(&mut t, b.def(&tuning).name.get());
        }
    }
    for (b, mut n, mut bg) in &mut bars {
        match *b {
            Bar::BossHp(i) => {
                if let Some((_, boss, hp)) = main.get(i as usize) {
                    n.width = percent(hp.cur / hp.max * 100.0);
                    let c = boss_bar_color(boss.def(&tuning).color);
                    if bg.0 != c {
                        bg.0 = c;
                    }
                }
            }
            Bar::BossStagger(i) => {
                if let Some((_, boss, _)) = main.get(i as usize) {
                    n.width = percent(boss.stagger / boss.def(&tuning).stagger_max * 100.0);
                }
            }
            _ => {}
        }
    }
}

fn outcome(
    time: Res<Time>,
    fx: Res<FxState>,
    enc: Res<Encounter>,
    local: Query<(), With<LocalPlayer>>,
    mut out: ResMut<Outcome>,
    mut banner: Query<(&mut Visibility, &mut BackgroundColor), With<Banner>>,
    mut texts: Query<(&mut Text, &mut TextColor), With<BannerText>>,
) {
    for e in &fx.last {
        match e {
            SimEvent::PlayerDied { entity } if local.contains(*entity) => {
                *out = Outcome { text: Some((("YOU DIED", "VOUS ÊTES MORT"), Color::srgb(0.75, 0.12, 0.08))), ..default() };
            }
            SimEvent::BossDefeated => {
                let text = if enc.arena == Some(0) { ("AUTOMATON DESTROYED", "AUTOMATE DÉTRUIT") } else { ("GREAT FOE FELLED", "GRAND ENNEMI TERRASSÉ") };
                *out = Outcome { text: Some((text, Color::srgb(0.9, 0.75, 0.35))), duration: Some(5.0), ..default() };
            }
            SimEvent::DoorOpened => {
                out.next = Some((("A GREAT DOOR HAS OPENED", "UNE GRANDE PORTE S'EST OUVERTE"), Color::srgb(0.8, 0.85, 0.95)));
            }
            SimEvent::Kindled { entity, .. } if local.contains(*entity) => {
                *out = Outcome { text: Some((("BRAZIER KINDLED", "BRASIER RANIMÉ"), Color::srgb(0.95, 0.78, 0.4))), duration: Some(3.5), ..default() };
            }
            SimEvent::Respawned => *out = Outcome::default(),
            SimEvent::Revived { entity } if local.contains(*entity) => *out = Outcome::default(),
            _ => {}
        }
    }
    let Some(((en, fr), color)) = out.text else {
        for (mut v, _) in &mut banner {
            *v = Visibility::Hidden;
        }
        return;
    };
    out.timer += time.delta_secs();
    if out.duration.is_some_and(|d| out.timer > d) {
        *out = match out.next {
            Some(text) => Outcome { text: Some(text), duration: Some(4.0), ..default() },
            None => Outcome::default(),
        };
        return;
    }
    let fade_out = out.duration.map_or(1.0, |d| (d - out.timer).min(1.0));
    let a = (out.timer / 0.9).min(1.0) * fade_out;
    for (mut v, mut bg) in &mut banner {
        *v = Visibility::Inherited;
        bg.0 = Color::srgba(0.0, 0.0, 0.0, 0.7 * a);
    }
    for (mut tx, mut c) in &mut texts {
        set_text(&mut tx, tr(en, fr));
        c.0 = color.with_alpha(a);
    }
}

#[allow(clippy::type_complexity)]
fn overlay(
    time: Res<Time>,
    fx: Res<FxState>,
    mut anim: ResMut<HudAnim>,
    players: Query<(Entity, &Player), With<LocalPlayer>>,
    mut flash: Query<&mut BackgroundColor, (With<Overlay>, Without<Fade>)>,
    mut fade: Query<&mut BackgroundColor, (With<Fade>, Without<Overlay>)>,
) {
    for mut bg in &mut flash {
        bg.0 = if fx.perfect_flash > 0.0 {
            Color::srgba(1.0, 0.95, 0.8, fx.perfect_flash * 0.18)
        } else if fx.fury_flash > 0.0 {
            Color::srgba(0.8, 0.0, 0.0, fx.fury_flash * 0.22)
        } else {
            Color::NONE
        };
    }
    // Death: the scene darkens during "YOU DIED", fully black just before the
    // respawn at the checkpoint, then gradually back.
    // Through a fog: from black, into the arena (or back out).
    let me = players.single().ok();
    let mine = |e: &Entity| me.is_some_and(|(m, _)| m == *e);
    if fx.last.iter().any(|e| matches!(e, SimEvent::Respawned) || matches!(e, SimEvent::Passage { entity, .. } | SimEvent::Revived { entity } if mine(entity))) {
        anim.fade = 1.0;
    }
    let dying = fx_dying(&fx, &mut anim, me.map(|(e, _)| e));
    // Co-op, fallen during a boss fight: past the respawn time, they watch the others.
    let target = match me.map(|(_, p)| p) {
        Some(p) if p.state == PState::Dead && (RESPAWN_TICKS.saturating_sub(45)..=RESPAWN_TICKS).contains(&p.dead_ticks) => 1.0,
        Some(p) if p.state == PState::Dead || dying => 0.4,
        _ => 0.0,
    };
    let dt = time.delta_secs();
    anim.fade = if target > anim.fade {
        (anim.fade + dt * 1.6).min(target)
    } else {
        (anim.fade - dt * 1.2).max(target)
    };
    for mut bg in &mut fade {
        bg.0 = Color::srgba(0.0, 0.0, 0.0, anim.fade);
    }
}

#[allow(clippy::too_many_arguments)]
fn reticle(
    players: Query<&Player, With<LocalPlayer>>,
    tuning: Res<Tuning>,
    foes: LockFoes,
    bones: PartBones,
    camera: Single<(&Camera, &GlobalTransform), With<WorldCamera>>,
    target: Res<LowResTarget>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    mut q: Query<(&mut Node, &mut Visibility), With<Reticle>>,
) {
    let Ok((mut node, mut vis)) = q.single_mut() else { return };
    let point = players
        .single()
        .ok()
        .and_then(|p| Some((p.lock?, p.lock_part)))
        .and_then(|(e, part)| lock_target(&tuning, e, part, &foes, &bones));
    let Some(world) = point else {
        *vis = Visibility::Hidden;
        return;
    };
    let (cam, gt) = *camera;
    let Ok(vp) = cam.world_to_viewport(gt, world) else {
        *vis = Visibility::Hidden;
        return;
    };
    let sx = window.width() / target.size.x as f32;
    let sy = window.height() / target.size.y as f32;
    // UI lengths are multiplied by UiScale: convert to UI units.
    node.left = px(vp.x * sx / ui_scale.0 - 3.0);
    node.top = px(vp.y * sy / ui_scale.0 - 3.0);
    *vis = Visibility::Inherited;
}

/// True between the fatal blow and the end of the death animation.
fn fx_dying(fx: &FxState, anim: &mut HudAnim, local: Option<Entity>) -> bool {
    let mine = |e: &Entity| local == Some(*e);
    if fx.last.iter().any(|e| matches!(e, SimEvent::PlayerDied { entity } if mine(entity))) {
        anim.dying = true;
    }
    if fx.last.iter().any(|e| matches!(e, SimEvent::Respawned) || matches!(e, SimEvent::Revived { entity } if mine(entity))) {
        anim.dying = false;
    }
    anim.dying
}

/// "Item obtained": each picked-up item is shown for a few seconds, one after the other.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn popup(
    time: Res<Time>,
    fx: Res<FxState>,
    tuning: Res<Tuning>,
    icons: Res<ItemIcons>,
    mut pop: ResMut<PickupPopup>,
    mut root: Query<&mut Visibility, With<Popup>>,
    mut icon: Query<&mut ImageNode, With<PopupIcon>>,
    mut name: Query<&mut Text, (With<PopupName>, Without<PopupInfo>)>,
    mut info: Query<&mut Text, (With<PopupInfo>, Without<PopupName>)>,
    local: Query<(), With<LocalPlayer>>,
) {
    for e in &fx.last {
        if let SimEvent::PickedUp { pickup, entity } = e
            && local.contains(*entity)
            && let Some(p) = tuning.level.pickups.get(*pickup as usize)
        {
            pop.queue.extend(p.items.iter().copied());
        }
        if let SimEvent::LootPicked { item, entity } = e
            && local.contains(*entity)
        {
            pop.queue.push_back((*item, 1));
        }
    }
    pop.timer -= time.delta_secs();
    if pop.timer <= 0.0 {
        pop.current = pop.queue.pop_front();
        pop.timer = if pop.current.is_some() { POPUP_SHOW } else { 0.0 };
    }
    let Some((item, n)) = pop.current else {
        for mut v in &mut root {
            *v = Visibility::Hidden;
        }
        return;
    };
    for mut v in &mut root {
        *v = Visibility::Inherited;
    }
    for mut img in &mut icon {
        let h = icons.get(item);
        if img.image != h {
            img.image = h;
        }
    }
    for mut t in &mut name {
        set_text(&mut t, if n > 1 { format!("{} ×{n}", item.name()) } else { item.name().to_string() });
    }
    for mut t in &mut info {
        set_text(&mut t, item.description());
    }
}

/// Co-op: the partner's health ("Host" / "Guest", greyed once fallen), and the connection: round
/// trip, a peer gone silent, a desync (the games no longer match: a bug to report).
#[allow(clippy::type_complexity)]
fn allies(
    net: Res<crate::net::Net>,
    local: Res<crate::net::LocalId>,
    players: Query<(&Player, &Health)>,
    mut rows: Query<(&AllyRow, &mut Node), Without<AllyHp>>,
    mut names: Query<(&AllyName, &mut Text, &mut TextColor), Without<NetLine>>,
    mut hps: Query<(&AllyHp, &mut Node), Without<AllyRow>>,
    mut line: Query<&mut Text, (With<NetLine>, Without<AllyName>)>,
) {
    let mut others: Vec<(&Player, &Health)> = players.iter().filter(|(p, _)| p.id != local.0).collect();
    others.sort_by_key(|(p, _)| p.id);
    let at = |slot: u8| others.get(slot as usize).copied();
    for (r, mut n) in &mut rows {
        let want = if at(r.0).is_some() { Display::Flex } else { Display::None };
        if n.display != want {
            n.display = want;
        }
    }
    for (a, mut t, mut c) in &mut names {
        let Some((p, h)) = at(a.0) else { continue };
        set_text(&mut t, role_name(p.id));
        c.0 = if h.dead() { Color::srgba(0.6, 0.58, 0.55, 0.7) } else { GOLD };
    }
    for (a, mut n) in &mut hps {
        if let Some((_, h)) = at(a.0) {
            n.width = px(h.cur.max(0.0) * ALLY_HP_PX);
        }
    }
    let s = &net.status;
    let text = if !net.coop() {
        String::new()
    } else if let Some(f) = s.desync {
        format!("{} ({f})", tr("DESYNC", "DÉSYNCHRONISATION"))
    } else if s.interrupted {
        tr("Connection unstable…", "Connexion instable…").into()
    } else {
        s.ping_ms.map_or(String::new(), |ms| format!("{ms} ms"))
    };
    for mut t in &mut line {
        set_text(&mut t, text.clone());
    }
}

/// Active effects, under the bars: resin (flaming weapon), moss (regeneration), ash
/// (protection), root (stamina).
fn effects(players: Query<&Player, With<LocalPlayer>>, mut q: Query<&mut Text, With<Effects>>) {
    let Ok(p) = players.single() else { return };
    let parts: Vec<String> = [
        (Item::EmberResin, p.resin_ticks),
        (Item::GoldenMoss, p.regen_ticks),
        (Item::WardingAsh, p.ward_ticks),
        (Item::VigorRoot, p.vigor_ticks),
    ]
    .into_iter()
    .filter(|(_, ticks)| *ticks > 0)
    .map(|(item, ticks)| format!("{} {} s", item.name(), ticks.div_ceil(60)))
    .collect();
    for mut t in &mut q {
        set_text(&mut t, parts.join("   "));
    }
}

/// Co-op: the partner search (and how to give up), or for a few seconds why the co-op stopped.
fn search_line(net: Res<crate::net::Net>, device: Res<crate::input::Device>, mut q: Query<&mut Text, With<SearchLine>>) {
    let text = if net.searching() {
        let cancel = if *device == crate::input::Device::Gamepad { tr("(B): cancel", "(B) : annuler") } else { tr("Esc: cancel", "Échap : annuler") };
        format!("{}   {cancel}", crate::net::search_text(&net))
    } else if net.notice_for > 0.0 {
        crate::net::notice_text(net.notice.as_ref())
    } else {
        String::new()
    };
    for mut t in &mut q {
        set_text(&mut t, &text);
    }
}

/// Co-op: what a player is called — the host (player 0) or the guest.
fn role_name(id: u8) -> &'static str {
    if id == 0 { tr("Host", "Hôte") } else { tr("Guest", "Invité") }
}

/// Co-op: "Host" / "Guest" over the partner's head (they all look alike), within sight.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn ally_tags(
    mut commands: Commands,
    root: Single<Entity, With<HudRoot>>,
    font: Res<UiFont>,
    me: Query<&Interp, With<LocalPlayer>>,
    allies: Query<(Entity, &Player, &Interp, &Body, &Health), Without<LocalPlayer>>,
    camera: Single<(&Camera, &GlobalTransform), With<WorldCamera>>,
    target: Res<LowResTarget>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    mut tags: Query<(Entity, &AllyTag, &mut Node, &mut Visibility)>,
) {
    let (cam, gt) = *camera;
    let sx = window.width() / target.size.x as f32;
    let sy = window.height() / target.size.y as f32;
    let mut tagged: Vec<Entity> = Vec::new();
    for (e, tag, mut node, mut vis) in &mut tags {
        let Ok((_, _, i, b, h)) = allies.get(tag.0) else {
            commands.entity(e).despawn();
            continue;
        };
        tagged.push(tag.0);
        let near = me.single().is_ok_and(|m| m.pos.distance(i.pos) < 40.0);
        let vp = cam.world_to_viewport(gt, i.pos + Vec3::Y * (b.height + 0.45)).ok().filter(|_| near && !h.dead());
        let Some(vp) = vp else {
            *vis = Visibility::Hidden;
            continue;
        };
        *vis = Visibility::Inherited;
        node.left = px(vp.x * sx / ui_scale.0 - 6.0);
        node.top = px(vp.y * sy / ui_scale.0 - 6.0);
    }
    for (e, p, ..) in &allies {
        if !tagged.contains(&e) {
            commands.spawn((
                ChildOf(*root),
                AllyTag(e),
                font.text(role_name(p.id), 1, GOLD),
                Node { position_type: PositionType::Absolute, ..default() },
                Visibility::Hidden,
            ));
        }
    }
}

/// Floating health bars above wounded (or locked-on) enemies, Dark Souls style.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn foe_bars(
    mut commands: Commands,
    root: Single<Entity, With<HudRoot>>,
    players: Query<(&Player, &Interp), With<LocalPlayer>>,
    enemies: Query<(Entity, &Interp, &Body, &Health, Option<&Boss>), With<Foe>>,
    tuning: Res<Tuning>,
    camera: Single<(&Camera, &GlobalTransform), With<WorldCamera>>,
    target: Res<LowResTarget>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    mut bars: Query<(Entity, &FoeBar, &mut Node, &mut Visibility, &Children)>,
    mut fills: Query<&mut Node, (With<FoeBarFill>, Without<FoeBar>)>,
) {
    const W: f32 = 44.0;
    let (cam, gt) = *camera;
    let player = players.single().ok();
    let lock = player.and_then(|(p, _)| p.lock);
    let sx = window.width() / target.size.x as f32;
    let sy = window.height() / target.size.y as f32;
    let mut has_bar: Vec<Entity> = Vec::new();
    for (e, bar, mut node, mut vis, children) in &mut bars {
        let Ok((_, i, b, h, _)) = enemies.get(bar.0) else {
            commands.entity(e).despawn();
            continue;
        };
        has_bar.push(bar.0);
        let near = player.is_some_and(|(_, pi)| pi.pos.distance(i.pos) < 30.0);
        let show = !h.dead() && near && (h.cur < h.max || lock == Some(bar.0));
        let vp = cam.world_to_viewport(gt, i.pos + Vec3::Y * (b.height + 0.35)).ok();
        let Some(vp) = vp.filter(|_| show) else {
            *vis = Visibility::Hidden;
            continue;
        };
        *vis = Visibility::Inherited;
        node.left = px(vp.x * sx / ui_scale.0 - W / 2.0);
        node.top = px(vp.y * sy / ui_scale.0);
        for c in children {
            if let Ok(mut f) = fills.get_mut(*c) {
                f.width = px((W - 2.0) * (h.cur / h.max).clamp(0.0, 1.0));
            }
        }
    }
    // Path enemies and a boss's supporting roles (the leads have their bar at the bottom).
    for (e, .., boss) in &enemies {
        if !has_bar.contains(&e) && boss.is_none_or(|b| b.def(&tuning).minor) {
            let fill = boss.map_or(Color::srgb(0.68, 0.08, 0.06), |b| boss_bar_color(b.def(&tuning).color));
            commands
                .spawn((
                    ChildOf(*root),
                    FoeBar(e),
                    Node { position_type: PositionType::Absolute, width: px(W), height: px(4), padding: UiRect::all(px(1)), ..default() },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
                    Visibility::Hidden,
                ))
                .with_children(|c| {
                    c.spawn((Node { width: px(W - 2.0), height: px(2), ..default() }, BackgroundColor(fill), FoeBarFill));
                });
        }
    }
}
