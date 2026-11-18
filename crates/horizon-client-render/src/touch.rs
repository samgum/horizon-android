// Adapted from Starling (https://github.com/starling-client/starling, `android` branch at
// 150a414b09a1, `crates/horizon-client-render/src/touch.rs`), written by Vasysik and the
// Starling contributors. Copyright (c) 2026 Ludovic Marie and the Horizon contributors, under the
// same MIT license as Horizon (LICENSE at the repository root).

//! On-screen touch controls for touch-screen devices (Android first). Nothing
//! here runs unless a touch surface is in use.
//!
//! [`TouchControls`] turns fingers into the controller state the game already
//! reads ([`PadState`]). Each finger gets a role when it lands (the joystick or
//! D-pad, a button, or a look gesture on the world) and keeps it until it
//! lifts, so moving, looking and pressing never take each other's fingers.
pub mod aim;
pub mod container;
mod draw;
pub mod layout;
pub use aim::{
    attack_indicator, camera_delta, hotbar_slot, item_places_entity_on_tap, item_uses_on_tap,
    projectile_crosshair, screen_direction,
};
pub use layout::{ChatBarPosition, ContainerTouchStyle, Control, Mode, Placement, Settings};

use crate::gamepad::{Button, PadState};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// A world touch held this long without dragging becomes a hold (mining, eating).
const HOLD_DELAY: Duration = Duration::from_millis(320);
/// Shorter hold delay when drawing a projectile weapon (bow, trident).
const PROJECTILE_HOLD_DELAY: Duration = Duration::from_millis(180);
/// How long the marker of a short world tap stays.
const MARKER_TIME: Duration = Duration::from_millis(180);
/// Two taps on the D-pad's sneak button closer than this toggle crouching.
const DOUBLE_TAP: Duration = Duration::from_millis(320);
/// Two taps on Jump closer than this toggle creative flight.
const DOUBLE_JUMP: Duration = Duration::from_millis(380);
/// A look finger that moves farther than this (a fraction of the surface
/// height) is a camera drag, not a tap.
const DRAG_SLOP: f32 = 0.011;
/// The swap-hands action a [`Control::Offhand`] tap sends.
const SWAP_HANDS: &str = "action:key.swapOffhand";

/// What a finger does, decided when it lands.
#[derive(Clone, Copy, Debug)]
enum Role {
    /// Drives the joystick or D-pad, measured from this anchor.
    Move([f32; 2]),
    /// Turns the camera; without a drag, a short tap or a long hold acts on
    /// the world where it landed.
    Look {
        start: [f32; 2],
        since: Instant,
        /// Moved past [`DRAG_SLOP`]: a camera turn, not a tap.
        dragged: bool,
        /// Whether a hold uses the item (eat, draw a bow) instead of mining,
        /// as [`TouchControls::hold_uses_item`] said when the finger landed.
        held_use: bool,
        /// Became a hold before it dragged; it keeps acting while it turns.
        committed: bool,
    },
    /// Landed on a control that acted at once; nothing more until it lifts.
    Ignore,
    /// Holds the shield up.
    Shield,
    /// Holds a controller button.
    Button(Button),
}

/// The on-screen controls' input state. The game feeds it touches with
/// [`Self::update`] and reads [`Self::poll_state`] once a tick.
#[derive(Default)]
pub struct TouchControls {
    pub settings: Settings,
    /// The game needs the crosshair whatever the scheme.
    pub force_crosshair: bool,
    /// The held item is aimed along the crosshair ([`projectile_crosshair`]).
    pub projectile_crosshair: bool,
    /// The player has a shield, so the shield button may show.
    pub shield_available: bool,
    /// Only the shortcut buttons (chat, pause, settings, F3, F5): a keyboard
    /// or controller moves the player.
    pub auxiliary_only: bool,
    /// The use button would act on what the crosshair points at (shown with
    /// `contextual_interact` in the tap schemes).
    pub context_use: bool,
    /// Flying: jump and sneak move up and down.
    pub vertical_movement: bool,
    pub swimming: bool,
    pub mounted: bool,
    /// The player is sneaking: the sneak button shows held.
    pub sneaking: bool,
    /// How far the block under a world hold is broken (0..1), drawn in the
    /// touch marker.
    pub break_progress: f32,
    /// A long hold on the world uses the held item instead of mining. Read when
    /// a finger lands, so a change mid-hold (the last apple eaten) can't turn
    /// eating into mining.
    pub hold_uses_item: bool,
    /// Where this tick's world action aims (0..1 of the surface), set by
    /// [`Self::poll_state`].
    pub aim: Option<[f32; 2]>,
    /// A world hold is active this tick, and whether it uses the item.
    pub holding: bool,
    pub holding_use: bool,
    /// A button was pressed; the game plays the click and clears it.
    pub clicked: bool,
    pub chat_requested: bool,
    pub settings_requested: bool,
    pub debug_requested: bool,
    pub perspective_requested: bool,
    /// Creative `mayfly` ability is active; double-tapping Jump sets `fly_toggle_requested`.
    pub can_fly: bool,
    pub fly_toggle_requested: bool,
    /// When true, contextual `Use` targets a mountable entity (show Ride icon).
    pub mount_hint: bool,
    /// Normalized `[left, top, right, bottom]` safe-area insets (cutouts & system bars).
    pub safe_insets: [f32; 4],
    /// Normalized `([center_x, center_y], size)` for snapping `Inventory` next to the hotbar.
    pub hotbar_edge: Option<([f32; 2], f32)>,
    fingers: BTreeMap<u64, (Role, [f32; 2])>,
    /// Presses that must reach the game even if released before the next poll.
    pending: PadState,
    /// Key or action presses for the game's key bindings: (token, down).
    key_events: Vec<(String, bool)>,
    tap_aim: Option<[f32; 2]>,
    aim_until: Option<Instant>,
    /// A short world tap waiting for the game, with the finger's `held_use`.
    pending_tap: Option<bool>,
    last_marker: Option<([f32; 2], Instant)>,
    last_sneak_tap: Option<Instant>,
    last_jump_tap: Option<Instant>,
    sprint_latched: bool,
    aspect: f32,
}

impl TouchControls {
    /// Clears all active fingers (e.g. when the app goes to the background).
    pub fn clear_fingers(&mut self) {
        self.fingers.clear();
        self.pending = PadState::default();
        self.holding = false;
        self.holding_use = false;
        self.aim = None;
        self.tap_aim = None;
        self.aim_until = None;
    }

    /// Whether attack and use act on the crosshair rather than on a tap.
    pub fn crosshair(&self) -> bool {
        self.force_crosshair || self.settings.crosshair()
    }

    /// The surface size in pixels; controls stay square on any shape.
    pub fn viewport(&mut self, width: u32, height: u32) {
        self.aspect = width.max(1) as f32 / height.max(1) as f32;
    }

    fn aspect(&self) -> f32 {
        if self.aspect > 0.0 {
            self.aspect
        } else {
            16.0 / 9.0
        }
    }

    /// Where a dynamic joystick may appear, as `[x, y, w, h]`: a strip
    /// `dynamic_area_x` of the width wide on the joystick's side, and
    /// `dynamic_area_y` of the height tall centred on it. Touches outside it
    /// stay camera gestures.
    pub fn dynamic_area(&self) -> [f32; 4] {
        let home = self.placement(Control::Move);
        let width = self.settings.dynamic_area_x.clamp(0.0, 1.0);
        let height = self.settings.dynamic_area_y.clamp(0.0, 1.0);
        let y = (home.center[1] - height / 2.0).clamp(0.0, 1.0 - height);
        let x = if home.center[0] < 0.5 {
            0.0
        } else {
            1.0 - width
        };
        [x, y, width, height]
    }

    fn dynamic_joystick(&self) -> bool {
        self.settings.dynamic_joystick && self.settings.mode != Mode::Dpad
    }

    /// The shortcuts that stay with [`Self::auxiliary_only`].
    fn auxiliary_control(c: Control) -> bool {
        matches!(
            c,
            Control::Debug
                | Control::Chat
                | Control::Pause
                | Control::Settings
                | Control::Perspective
        )
    }

    /// Whether `point` is on a visible shortcut button.
    pub fn auxiliary_at(&self, point: [f32; 2]) -> bool {
        Control::ALL.into_iter().any(|c| {
            Self::auxiliary_control(c)
                && self.visible(c)
                && layout::contains(self.placement(c).rect(self.aspect()), point)
        })
    }

    /// The visible control under `point`, the last-drawn one first.
    pub fn control_at(&self, point: [f32; 2]) -> Option<Control> {
        Control::ALL.into_iter().rev().find(|&c| {
            if !self.visible(c) {
                return false;
            }
            if c == Control::Move && self.dynamic_joystick() {
                let area = self.dynamic_area();
                return area[2] > 0.0
                    && area[3] > 0.0
                    && !self.moving()
                    && layout::contains(area, point);
            }
            layout::contains(self.placement(c).rect(self.aspect()), point)
        })
    }

    fn moving(&self) -> bool {
        self.fingers
            .values()
            .any(|(r, _)| matches!(r, Role::Move(_)))
    }

    /// Whether this finger belongs to the touch controls.
    pub fn owns_pointer(&self, id: u64) -> bool {
        self.fingers.contains_key(&id)
    }

    /// Hands a finger to something else (a menu) until it lifts.
    pub fn ignore(&mut self, id: u64) {
        self.fingers.insert(id, (Role::Ignore, [0.0; 2]));
    }

    /// Presses `button` for one poll.
    pub fn tap(&mut self, button: Button) {
        self.pending.set(button, true);
    }

    fn visible(&self, c: Control) -> bool {
        let visibility = self.settings.button_visibility[c as usize];
        if c == Control::Shield
            && (!self.shield_available
                || (self.force_crosshair
                    && visibility == layout::FOLLOW_SCHEME
                    && self.settings.button_visibility[Control::Use as usize]
                        == layout::FOLLOW_SCHEME))
        {
            // A forced crosshair brings back the use button, which raises the shield too.
            return false;
        }
        if self.auxiliary_only && !Self::auxiliary_control(c) {
            return false;
        }
        if self.force_crosshair
            && matches!(c, Control::Attack | Control::Use)
            && visibility == layout::FOLLOW_SCHEME
        {
            return true;
        }
        self.settings.visible(c)
            || (c == Control::Use
                && visibility != layout::HIDDEN
                && (self.projectile_crosshair
                    || (self.settings.contextual_interact
                        && (self.context_use
                            || self
                                .fingers
                                .values()
                                .any(|(r, _)| matches!(r, Role::Button(Button::LeftTrigger)))))))
    }

    /// Where a control is on this surface: the saved placement, scaled by
    /// `button_scale` and kept inside safe-area bounds.
    fn placement(&self, c: Control) -> Placement {
        let aspect = self.aspect();
        let mut p = self.settings.placement(c);
        let default_offhand = layout::DEFAULT_ROW[Control::Offhand as usize];
        let factory_swap = c == Control::Offhand
            && self.settings.layouts[self.settings.mode.index()][c as usize].center
                == default_offhand.center
            && (p.size - default_offhand.size).abs() < 0.0001;
        // On tall surfaces the factory top row would overlap: spread it out.
        // Moved buttons and every landscape layout stay where they were saved.
        let default_top = layout::DEFAULT_ROW[Control::Chat as usize];
        if aspect < 1.0
            && (p.center[1] - default_top.center[1]).abs() < 0.0001
            && (p.size - default_top.size).abs() < 0.0001
        {
            let mirror = |x: f32| {
                if self.settings.left_handed {
                    1.0 - x
                } else {
                    x
                }
            };
            let top = match c {
                Control::Debug => Some((layout::DEFAULT_ROW[Control::Debug as usize].center[0], 0.18)),
                Control::Chat => Some((layout::DEFAULT_ROW[Control::Chat as usize].center[0], 0.34)),
                Control::Pause => Some((layout::DEFAULT_ROW[Control::Pause as usize].center[0], 0.50)),
                Control::Settings => Some((layout::DEFAULT_ROW[Control::Settings as usize].center[0], 0.66)),
                Control::Perspective => Some((layout::DEFAULT_ROW[Control::Perspective as usize].center[0], 0.82)),
                _ => None,
            };
            if let Some((old, new)) = top
                && (p.center[0] - mirror(old)).abs() < 0.0001
            {
                p.center[0] = mirror(new);
                p.size = p.size.min(aspect * 0.12);
            }
        }
        let mut p = self.fit(p);
        let default_inv = layout::DEFAULT_ROW[Control::Inventory as usize];
        if c == Control::Inventory
            && self.settings.layouts[self.settings.mode.index()][c as usize].center
                == default_inv.center
            && let Some(([x, y], sz)) = self.hotbar_edge
        {
            p.center = [if self.settings.left_handed { 1.0 - x } else { x }, y];
            p.size = sz.clamp(0.045, 0.25);
        }
        if factory_swap {
            // The factory swap button sits beside inventory, in button sizes,
            // so it never covers the jump, sneak or sprint buttons.
            let inventory = self.placement(Control::Inventory);
            let side = if self.settings.left_handed { -1.0 } else { 1.0 };
            let half = p.size / aspect / 2.0;
            let lo = (self.safe_insets[0].clamp(0.0, 0.3) + half).min(0.5);
            let hi = (1.0 - self.safe_insets[2].clamp(0.0, 0.3) - half).max(0.5);
            p.center[0] = (inventory.center[0]
                + side * ((inventory.size + p.size) / 2.0 + 0.015) / aspect)
                .clamp(lo, hi);
        }
        p
    }

    /// Scales a placement by `button_scale` and keeps it inside the safe area.
    fn fit(&self, mut p: Placement) -> Placement {
        let aspect = self.aspect();
        p.size = (p.size * self.settings.button_scale).min(aspect).min(1.0);
        let half = [p.size / aspect / 2.0, p.size / 2.0];
        let [in_l, in_t, in_r, in_b] = self.safe_insets.map(|v| v.clamp(0.0, 0.25));
        let bounds = [
            (in_l + half[0], 1.0 - in_r - half[0]),
            (in_t + half[1], 1.0 - in_b - half[1]),
        ];
        for (c, (lo, hi)) in p.center.iter_mut().zip(bounds) {
            if lo <= hi {
                *c = c.clamp(lo, hi);
            } else {
                *c = 0.5;
            }
        }
        p
    }

    /// The controller button a control holds, if it holds one.
    fn button(c: Control) -> Option<Button> {
        Some(match c {
            Control::Sprint => Button::LeftStick,
            Control::Jump => Button::South,
            Control::Sneak => Button::RightStick,
            Control::Attack => Button::RightTrigger,
            Control::Use | Control::Shield => Button::LeftTrigger,
            Control::Pause => Button::Start,
            Control::Inventory => Button::North,
            _ => return None,
        })
    }

    /// Whether a finger holds the shield button.
    pub fn shield_held(&self) -> bool {
        self.settings.shield_button
            && self.shield_available
            && self
                .fingers
                .values()
                .any(|(r, _)| matches!(r, Role::Shield))
    }

    /// Where the touch marker goes in the tap schemes: under a look finger, or
    /// briefly where a short tap landed.
    pub fn pointer(&self) -> Option<[f32; 2]> {
        if self.crosshair() {
            return None;
        }
        self.fingers
            .values()
            .find_map(|(r, point)| matches!(r, Role::Look { .. }).then_some(*point))
            .or_else(|| {
                self.last_marker
                    .filter(|(_, t)| t.elapsed() < MARKER_TIME)
                    .map(|(p, _)| p)
            })
    }

    /// The aim point of a world action: the crosshair, or where the finger is.
    fn aim_at(&self, point: [f32; 2]) -> [f32; 2] {
        if self.settings.crosshair() || self.projectile_crosshair {
            [0.5, 0.5]
        } else {
            point
        }
    }

    fn world_gestures(&self) -> bool {
        !self.crosshair() || self.settings.crosshair_gestures
    }

    /// One touch event: finger `id` at `point` (0..1 of the surface), as it
    /// lands (`started`), moves, or lifts (`ended`). Returns how far the
    /// camera should turn, in surface fractions (see [`camera_delta`]).
    pub fn update(&mut self, id: u64, point: [f32; 2], started: bool, ended: bool) -> [f32; 2] {
        if ended {
            self.lift(id);
            return [0.0; 2];
        }
        if started {
            self.land(id, point);
            return [0.0; 2];
        }
        let aspect = self.aspect();
        let attack_look = self.settings.attack_button_look && self.crosshair();
        let gestures = self.world_gestures();
        let projectile_aim = self.projectile_crosshair;
        let dpad_mode = self.settings.mode == Mode::Dpad;
        let dpad_radius = self.placement(Control::Move).size / 2.0;
        let mut slid_out_of_dpad_center = false;
        let Some((role, previous)) = self.fingers.get_mut(&id) else {
            return [0.0; 2];
        };
        let delta = [point[0] - previous[0], point[1] - previous[1]];
        let step_dist = (delta[0] * aspect).hypot(delta[1]);
        let turns = match role {
            Role::Look {
                start,
                since,
                dragged,
                held_use,
                committed,
            } => {
                let delay = if projectile_aim && *held_use {
                    PROJECTILE_HOLD_DELAY
                } else {
                    HOLD_DELAY
                };
                // Allow bow/trident draw to commit even during slow tracking aim,
                // as long as the finger isn't making a fast camera flick.
                if (!*dragged || (projectile_aim && *held_use && step_dist < 0.025))
                    && since.elapsed() >= delay
                {
                    *committed = true;
                }
                let slop = if gestures { DRAG_SLOP } else { 0.0 };
                *dragged |= ((point[0] - start[0]) * aspect).hypot(point[1] - start[1]) > slop;
                *dragged
            }
            Role::Move(anchor) => {
                if dpad_mode && dpad_radius > 0.0 {
                    let dx = ((point[0] - anchor[0]) * aspect / dpad_radius).abs();
                    let dy = ((anchor[1] - point[1]) / dpad_radius).abs();
                    if dx >= 1.0 / 3.0 || dy >= 1.0 / 3.0 {
                        slid_out_of_dpad_center = true;
                    }
                }
                false
            }
            // In the crosshair schemes the attack button can steer while it mines.
            Role::Button(Button::RightTrigger | Button::LeftTrigger) => attack_look,
            _ => false,
        };
        *previous = point;
        if slid_out_of_dpad_center {
            self.last_sneak_tap = None;
        }
        if turns { delta } else { [0.0; 2] }
    }

    fn lift(&mut self, id: u64) {
        let Some((
            Role::Look {
                start,
                since,
                dragged,
                held_use,
                ..
            },
            _,
        )) = self.fingers.remove(&id)
        else {
            return;
        };
        let delay = if self.projectile_crosshair && held_use {
            PROJECTILE_HOLD_DELAY
        } else {
            HOLD_DELAY
        };
        if self.world_gestures() && !dragged && since.elapsed() < delay {
            self.tap_aim = Some(self.aim_at(start));
            self.last_marker = Some((start, Instant::now()));
            self.pending_tap = Some(held_use);
        }
    }

    fn land(&mut self, id: u64, point: [f32; 2]) {
        let role = match self.control_at(point) {
            Some(Control::Move) if !self.moving() => Role::Move(if self.dynamic_joystick() {
                point
            } else {
                self.placement(Control::Move).center
            }),
            Some(Control::Shield) => {
                self.clicked = true;
                self.pending.triggers[0] = 1.0;
                Role::Shield
            }
            Some(Control::Offhand) => {
                self.clicked = true;
                self.key_events.push((SWAP_HANDS.into(), true));
                self.key_events.push((SWAP_HANDS.into(), false));
                Role::Ignore
            }
            Some(Control::Chat) => {
                self.chat_requested = true;
                Role::Ignore
            }
            Some(Control::Settings) => {
                self.settings_requested = true;
                Role::Ignore
            }
            Some(Control::Debug) => {
                self.debug_requested = true;
                Role::Ignore
            }
            Some(Control::Perspective) => {
                self.perspective_requested = true;
                Role::Ignore
            }
            Some(Control::Jump) => {
                let now = Instant::now();
                if self.can_fly && !self.swimming && !self.mounted {
                    if self
                        .last_jump_tap
                        .take()
                        .is_some_and(|t| now.duration_since(t) < DOUBLE_JUMP)
                    {
                        self.fly_toggle_requested = true;
                    } else {
                        self.last_jump_tap = Some(now);
                    }
                }
                Role::Button(Button::South)
            }
            Some(Control::Sneak)
                if self.settings.mode == Mode::Dpad
                    && !self.settings.swap_jump_sneak
                    && !self.vertical_movement
                    && !self.swimming
                    && !self.mounted =>
            {
                // The D-pad's centre crouches on a double tap, while still
                // allowing the thumb to slide outwards onto any direction!
                let now = Instant::now();
                if self
                    .last_sneak_tap
                    .take()
                    .is_some_and(|t| now.duration_since(t) < DOUBLE_TAP)
                {
                    Role::Button(Button::RightStick)
                } else {
                    self.last_sneak_tap = Some(now);
                    if !self.moving() {
                        Role::Move(self.placement(Control::Move).center)
                    } else {
                        Role::Ignore
                    }
                }
            }
            Some(Control::Sprint) if self.settings.sprint_toggle => {
                self.sprint_latched = !self.sprint_latched;
                Role::Ignore
            }
            Some(c) => Self::button(c).map_or(Role::Ignore, Role::Button),
            None => Role::Look {
                start: point,
                since: Instant::now(),
                dragged: false,
                held_use: self.hold_uses_item,
                committed: false,
            },
        };
        self.fingers.insert(id, (role, point));
        // Latch the press so a tap shorter than a tick still reaches the game.
        match role {
            Role::Button(Button::LeftTrigger) => self.pending.triggers[0] = 1.0,
            Role::Button(Button::RightTrigger) => self.pending.triggers[1] = 1.0,
            Role::Button(button) => self.pending.set(button, true),
            _ => {}
        }
    }

    /// Lets go of everything: fingers, latched presses, requests.
    pub fn clear(&mut self) {
        self.fingers.clear();
        self.last_sneak_tap = None;
        self.last_jump_tap = None;
        self.sprint_latched = false;
        self.pending = PadState::default();
        self.aim = None;
        self.tap_aim = None;
        self.aim_until = None;
        self.pending_tap = None;
        self.last_marker = None;
        self.holding_use = false;
        self.holding = false;
        self.chat_requested = false;
        self.settings_requested = false;
        self.debug_requested = false;
        self.perspective_requested = false;
        self.fly_toggle_requested = false;
    }

    /// A short world tap since the last call, and whether it uses the item.
    /// A tap never holds attack or use into the next tick: the game acts once
    /// at [`Self::aim`].
    pub fn take_tap(&mut self) -> Option<bool> {
        self.pending_tap.take()
    }

    /// Key or action presses for the game's key bindings since the last call.
    pub fn key_events(&mut self) -> Vec<(String, bool)> {
        std::mem::take(&mut self.key_events)
    }

    /// This tick's controller state, with presses that already lifted, and
    /// sets [`Self::aim`], [`Self::holding`] and [`Self::holding_use`].
    pub fn poll_state(&mut self) -> PadState {
        let mut state = self.state().merge(&std::mem::take(&mut self.pending));
        self.aim = self.tap_aim.take();
        self.aim_until = None;
        self.holding_use = false;
        self.holding = false;
        if !self.world_gestures() {
            return state;
        }
        let projectile_aim = self.projectile_crosshair;
        let mut hold = None;
        for (role, point) in self.fingers.values_mut() {
            if let Role::Look {
                since,
                dragged,
                held_use,
                committed,
                ..
            } = role
            {
                let delay = if projectile_aim && *held_use {
                    PROJECTILE_HOLD_DELAY
                } else {
                    HOLD_DELAY
                };
                if !*dragged && since.elapsed() >= delay {
                    *committed = true;
                }
                if !*dragged || *committed {
                    hold = Some((*point, *committed, *held_use));
                    if *committed {
                        break;
                    }
                }
            }
        }
        if let Some((point, committed, held_use)) = hold {
            self.aim = Some(self.aim_at(point));
            if committed {
                self.holding = true;
                self.holding_use = held_use;
                state.triggers[usize::from(!held_use)] = 1.0;
            }
        }
        state
    }

    /// Keeps the current hold using the item, even after the last one was
    /// eaten or the target changed.
    pub fn latch_use(&mut self) {
        if let Some((Role::Look { held_use, .. }, _)) = self.fingers.values_mut().find(|(r, _)| {
            matches!(
                r,
                Role::Look {
                    committed: true,
                    ..
                }
            )
        }) {
            *held_use = true;
        }
        self.holding_use = true;
    }

    /// The controller state the fingers hold right now.
    pub fn state(&self) -> PadState {
        let mut state = PadState::default();
        if self.settings.sprint_toggle && self.settings.sprint_button && self.sprint_latched {
            state.set(Button::LeftStick, true);
        }
        for (role, point) in self.fingers.values() {
            match role {
                Role::Move(anchor) => {
                    let radius = self.placement(Control::Move).size / 2.0;
                    let mut left = [
                        (point[0] - anchor[0]) * self.aspect() / radius,
                        (anchor[1] - point[1]) / radius,
                    ];
                    if self.settings.mode == Mode::Dpad {
                        left = left.map(|v| if v.abs() < 1.0 / 3.0 { 0.0 } else { v.signum() });
                    }
                    // Pushing forward past the joystick's rim sprints; coming
                    // back inside stops.
                    let sprint = left[1] > 1.0;
                    let length = left[0].hypot(left[1]).max(1.0);
                    state.left = left.map(|v| v / length);
                    if sprint
                        && self.settings.sprint_with_joystick
                        && self.settings.mode != Mode::Dpad
                    {
                        state.set(Button::LeftStick, true);
                    }
                }
                Role::Button(Button::LeftTrigger) | Role::Shield => state.triggers[0] = 1.0,
                Role::Button(Button::RightTrigger) => state.triggers[1] = 1.0,
                Role::Button(b) => state.set(*b, true),
                Role::Look { .. } | Role::Ignore => {}
            }
        }
        state
    }
}

#[cfg(test)]
mod tests;
