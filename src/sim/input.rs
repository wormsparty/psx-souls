//! A player's abstract inputs for one tick. It's the only thing the simulation reads
//! on the player side: over the network, it's exactly what is exchanged between peers.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Co-op: two players.
pub const MAX_PLAYERS: usize = 2;

/// Logical buttons (bitfield).
pub mod btn {
    pub const LIGHT: u16 = 1 << 0;
    pub const HEAVY: u16 = 1 << 1;
    pub const GUARD: u16 = 1 << 2;
    pub const SPECIAL: u16 = 1 << 3;
    pub const DODGE: u16 = 1 << 4;
    pub const LOCK: u16 = 1 << 5;
    pub const SWITCH: u16 = 1 << 6;
    /// Use the item in the selected quick slot.
    pub const ITEM: u16 = 1 << 7;
    /// Next quick slot.
    pub const NEXT_ITEM: u16 = 1 << 8;
    /// Interact (rest at the checkpoint).
    pub const INTERACT: u16 = 1 << 9;
    /// Start sprinting (stick click): it lasts as long as the stick is pushed.
    pub const SPRINT: u16 = 1 << 10;
    /// Next target, to the left or right (locked on: right stick, mouse).
    pub const TARGET_LEFT: u16 = 1 << 11;
    pub const TARGET_RIGHT: u16 = 1 << 12;
    /// Lockable point higher or lower (a large boss's head, its legs).
    pub const TARGET_UP: u16 = 1 << 13;
    pub const TARGET_DOWN: u16 = 1 << 14;
    /// Jump. The gamepad sends it with INTERACT (same button): interacting takes precedence.
    pub const JUMP: u16 = 1 << 15;
    pub const COUNT: usize = 16;
}

/// A player's compact input for one tick (10 bytes, what travels over the network).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlayerInput {
    pub buttons: u16,
    /// Quantised movement stick (-127..=127). y = towards the camera's forward.
    pub move_x: i8,
    pub move_y: i8,
    /// Camera yaw quantised on 16 bits (movement is relative to the camera).
    pub cam_yaw: i16,
    /// A decision taken in a menu (equip, travel…), encoded by `SimCommand::encode`: its number
    /// (0: none), its kind and two arguments. The same number twice (an input predicted by
    /// repeating the last one) is only applied once.
    pub cmd: [u8; 4],
}

impl PlayerInput {
    pub fn held(&self, b: u16) -> bool {
        self.buttons & b != 0
    }
    pub fn stick(&self) -> Vec2 {
        Vec2::new(self.move_x as f32 / 127.0, self.move_y as f32 / 127.0)
    }
    pub fn cam_yaw_rad(&self) -> f32 {
        self.cam_yaw as f32 / 32768.0 * std::f32::consts::PI
    }
    pub fn quantize_yaw(yaw: f32) -> i16 {
        let w = crate::sim::math::wrap(yaw);
        (w / std::f32::consts::PI * 32767.0).round() as i16
    }
    pub fn quantize_axis(v: f32) -> i8 {
        (v.clamp(-1.0, 1.0) * 127.0).round() as i8
    }
}

/// Inputs of all players for the current tick (filled before each sim tick).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct PlayerInputs(pub [PlayerInput; MAX_PLAYERS]);

/// Remembers recent presses so that an action pressed a bit too early fires as soon as possible.
#[derive(Clone, Copy, Debug, Default, Hash)]
pub struct InputBuffer {
    /// Tick of the unconsumed press for each button.
    pressed_at: [Option<u32>; btn::COUNT],
    prev: u16,
}

impl InputBuffer {
    /// Call once per tick with the current buttons. Returns the rising edges.
    pub fn update(&mut self, buttons: u16, tick: u32) -> u16 {
        let pressed = buttons & !self.prev;
        for i in 0..btn::COUNT {
            if pressed & (1 << i) != 0 {
                self.pressed_at[i] = Some(tick);
            }
        }
        self.prev = buttons;
        pressed
    }

    /// True if `b` was pressed within the last `window` ticks and not yet consumed.
    pub fn buffered(&self, b: u16, tick: u32, window: u32) -> bool {
        let i = b.trailing_zeros() as usize;
        self.pressed_at[i].is_some_and(|t| tick.saturating_sub(t) <= window)
    }

    pub fn consume(&mut self, b: u16) {
        self.pressed_at[b.trailing_zeros() as usize] = None;
    }

    pub fn clear(&mut self) {
        self.pressed_at = [None; btn::COUNT];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_expires_and_consumes() {
        let mut b = InputBuffer::default();
        b.update(btn::LIGHT, 10);
        assert!(b.buffered(btn::LIGHT, 15, 8));
        assert!(!b.buffered(btn::LIGHT, 19, 8));
        b.consume(btn::LIGHT);
        assert!(!b.buffered(btn::LIGHT, 11, 8));
        // Holding the button doesn't retrigger.
        b.update(btn::LIGHT, 11);
        assert!(!b.buffered(btn::LIGHT, 11, 8));
    }

    #[test]
    fn yaw_roundtrip() {
        for y in [-3.0f32, -1.0, 0.0, 0.5, 3.1] {
            let q = PlayerInput { cam_yaw: PlayerInput::quantize_yaw(y), ..default() };
            assert!((q.cam_yaw_rad() - y).abs() < 1e-3);
        }
    }
}
