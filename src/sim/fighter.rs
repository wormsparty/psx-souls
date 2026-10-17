//! Components shared by all fighters (players and bosses).

use bevy::prelude::*;

use super::data::{MoveDef, MoveRef, Tuning};

/// Simulation position and orientation. The render `Transform` is interpolated from it.
#[derive(Component, Clone, Copy, Debug)]
pub struct Body {
    pub pos: Vec3,
    pub yaw: f32,
    pub radius: f32,
    pub height: f32,
    /// Relative weight for body separation (the boss pushes more than it's pushed).
    pub mass: f32,
}

/// State of the previous tick, for render interpolation.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PrevBody {
    pub pos: Vec3,
    pub yaw: f32,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct Health {
    pub cur: f32,
    pub max: f32,
}

/// Co-op: opponents' health, per extra player — +30% for path enemies (and a boss's supporting
/// roles, the butcher's dogs), +100% for the bosses.
pub const PARTY_ENEMY_HP: f32 = 0.3;
pub const PARTY_BOSS_HP: f32 = 1.0;

/// Health multiplier of an opponent facing `players` players (`boss`: a lead boss).
pub fn party_hp(players: usize, boss: bool) -> f32 {
    let per = if boss { PARTY_BOSS_HP } else { PARTY_ENEMY_HP };
    1.0 + per * players.saturating_sub(1) as f32
}

impl Health {
    pub fn new(max: f32) -> Self {
        Self { cur: max, max }
    }
    pub fn dead(&self) -> bool {
        self.cur <= 0.0
    }
}

/// Freezes the entity for N ticks after an impact ("hitstop").
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Hitstop(pub u8);

/// The players' opponent (boss or path enemy): it can be locked on and hit.
#[derive(Component, Clone, Copy, Debug)]
pub struct Foe;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Team {
    Players,
    Enemies,
}

/// Current action (attack, dodge, reaction…), shared by players and the boss.
#[derive(Component, Clone, Debug, Default)]
pub struct Action {
    pub mv: Option<MoveRef>,
    /// Index of the tick being executed in the action.
    pub tick: u32,
    /// Counter incremented on each new action (identifies the attack instance).
    pub seq: u32,
    /// Hits already landed during this action: (window index, victim).
    pub hits: Vec<(u8, Entity)>,
    /// Distance to the target frozen at the start (for `to_target` movements).
    pub target_dist: f32,
    /// True if the entity ran a frame this tick (false during hitstop).
    pub executed: bool,
}

impl Action {
    pub fn start(&mut self, mv: MoveRef, target_dist: f32) {
        self.mv = Some(mv);
        self.tick = 0;
        self.seq = self.seq.wrapping_add(1);
        self.hits.clear();
        self.target_dist = target_dist;
    }
    pub fn stop(&mut self) {
        self.mv = None;
        self.tick = 0;
        self.hits.clear();
    }
    pub fn def<'a>(&self, t: &'a Tuning) -> Option<&'a MoveDef> {
        self.mv.map(|m| t.get(m))
    }
    pub fn finished(&self, t: &Tuning) -> bool {
        self.def(t).is_some_and(|d| self.tick >= d.total)
    }
    pub fn is(&self, mv: MoveRef) -> bool {
        self.mv == Some(mv)
    }
    pub fn iframes(&self, t: &Tuning) -> bool {
        self.def(t).is_some_and(|d| MoveDef::in_window(d.iframes, self.tick))
    }
    pub fn hyperarmor(&self, t: &Tuning) -> bool {
        self.def(t).is_some_and(|d| MoveDef::in_window(d.hyperarmor, self.tick))
    }
}
