//! Touch state machine and side toolbar for container screens (`Inventory`,
//! `Crafting`, `Chest`, `Furnace`, `Creative`, stations).
//!
//! Supports both [`ContainerTouchStyle::Bedrock`] (long-press proportional split
//! bar, `[⇧]`/`[½]`/`[🗑]` side toolbar, outside-tap drop protection, swipe-to-scroll
//! without accidental slot pickup) and [`ContainerTouchStyle::Classic`] (fixed
//! long-press half-split + compact `[⇧]` Shift toggle).

use super::layout::ContainerTouchStyle;
use crate::hud::{HudAtlas, HudBatch, rgba};
use std::time::{Duration, Instant};

/// Hold threshold before Bedrock proportional stack-splitting begins.
const SPLIT_START: Duration = Duration::from_millis(220);
/// Duration over which the Bedrock split bar ramps from `1` to `total`.
const SPLIT_RAMP: Duration = Duration::from_millis(720);
/// Hold threshold for Classic right-click (50% split) when empty-handed.
const CLASSIC_HOLD: Duration = Duration::from_millis(320);
/// Hold threshold before holding a carried stack drip-places single items.
const DRIP_START: Duration = Duration::from_millis(280);
/// Interval between single-item placements while holding a carried stack.
const DRIP_INTERVAL: Duration = Duration::from_millis(140);
/// Movement threshold in GUI pixels before a slot touch becomes a scroll or drag.
const MOVE_SLOP_GUI: f32 = 7.0;

/// Metadata about the slot under a touch point when a finger lands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotTouchInfo {
    pub slot: usize,
    /// `[x, y, w, h]` in GUI pixels.
    pub rect: [f32; 4],
    /// Stack size currently in the slot (`0` when empty).
    pub count: u32,
    /// False for crafting result (`200`) and creative display slots (`>= DISPLAY`).
    pub splittable: bool,
}

/// High-level container action produced by [`ContainerTouch`].
#[derive(Clone, Debug, PartialEq)]
pub enum ContainerAction {
    None,
    /// Non-slot press/release (tabs, recipe book, search box, scrollbar, buttons).
    PassThroughPress([f32; 2]),
    PassThroughRelease([f32; 2]),
    /// Scroll list/grid by `lines` (`MouseWheel` vertical delta).
    Scroll(f32),
    /// Click `slot` (`right` = half/single, `quick_move` = Shift+Click).
    SlotClick {
        slot: usize,
        right: bool,
        quick_move: bool,
    },
    /// Take exact `take` items (`1..=total`) from `slot` into `carried`.
    SplitExact {
        slot: usize,
        take: u32,
        total: u32,
    },
    /// Begin quick-craft spreading `carried` starting at `slot`.
    DragStart {
        slot: usize,
        right: bool,
    },
    /// Add `slot` to the active quick-craft drag.
    DragSlot(usize),
    /// Finish the active quick-craft drag.
    DragEnd,
    /// Throw carried items (`all = true` -> whole stack, `false` -> 1 item).
    DropCarried {
        all: bool,
    },
}

/// Visual state of the Bedrock green proportional split bar above a held slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitOverlay {
    pub slot_rect: [f32; 4],
    pub fraction: f32,
    pub take: u32,
    pub total: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolBtn {
    QuickMove,
    Half,
    Drop,
}

#[derive(Clone, Debug)]
enum Gesture {
    Slot {
        start_gui: [f32; 2],
        last_y: f32,
        info: SlotTouchInfo,
        started_at: Instant,
        scrolling: bool,
        quick_crafting: bool,
        hold_committed: bool,
        last_drip: Option<Instant>,
    },
    OutsideSafe {
        start_gui: [f32; 2],
        last_y: f32,
        scrolling: bool,
    },
    PassThrough {
        start_gui: [f32; 2],
        last_y: f32,
        scrolling: bool,
    },
    Toolbar,
}

/// Stateful touch controller for an open container screen.
#[derive(Default)]
pub struct ContainerTouch {
    /// Latched `Shift` (`QuickMove`) mode from the `[⇧]` side button.
    pub quick_move: bool,
    /// Latched `Right-Click` (`Half / Place 1`) mode from the `[½]` side button.
    pub half_mode: bool,
    active: Option<(u64, Gesture)>,
}

impl ContainerTouch {
    /// Resets transient fingers when a container closes; unlatches mode toggles.
    pub fn reset(&mut self) {
        self.active = None;
        self.quick_move = false;
        self.half_mode = false;
    }

    pub fn clear_finger(&mut self) {
        self.active = None;
    }

    /// Toolbar button rectangles `[(ToolBtn, [x, y, w, h])]` in GUI pixels.
    fn toolbar_rects(
        style: ContainerTouchStyle,
        panel_rect: [f32; 4],
        gui_size: [f32; 2],
        safe_left_gui: f32,
    ) -> Vec<(ToolBtn, [f32; 4])> {
        let btn_size = 22.0;
        let gap = 4.0;
        let left_x = panel_rect[0] - btn_size - 6.0;
        let x = if left_x >= safe_left_gui + 2.0 {
            left_x
        } else {
            (panel_rect[0] + panel_rect[2] + 6.0).min(gui_size[0] - btn_size - 4.0)
        };
        let buttons: &[ToolBtn] = match style {
            ContainerTouchStyle::Bedrock => &[ToolBtn::QuickMove, ToolBtn::Half, ToolBtn::Drop],
            ContainerTouchStyle::Classic => &[ToolBtn::QuickMove],
        };
        let total_h = buttons.len() as f32 * btn_size + (buttons.len().saturating_sub(1)) as f32 * gap;
        let start_y = (panel_rect[1] + (panel_rect[3] - total_h) / 2.0)
            .clamp(4.0, (gui_size[1] - total_h - 4.0).max(4.0));
        buttons
            .iter()
            .enumerate()
            .map(|(i, &btn)| {
                (
                    btn,
                    [x, start_y + i as f32 * (btn_size + gap), btn_size, btn_size],
                )
            })
            .collect()
    }

    fn hit_toolbar(
        style: ContainerTouchStyle,
        panel_rect: [f32; 4],
        gui_size: [f32; 2],
        safe_left_gui: f32,
        pt: [f32; 2],
    ) -> Option<ToolBtn> {
        Self::toolbar_rects(style, panel_rect, gui_size, safe_left_gui)
            .into_iter()
            .find(|(_, [x, y, w, h])| {
                pt[0] >= *x && pt[0] <= *x + *w && pt[1] >= *y && pt[1] <= *y + *h
            })
            .map(|(b, _)| b)
    }

    /// Computes proportional split `(fraction, take)` for a hold of `elapsed`.
    pub fn split_amount(elapsed: Duration, total: u32) -> Option<(f32, u32)> {
        if total < 2 || elapsed < SPLIT_START {
            return None;
        }
        let over = elapsed.saturating_sub(SPLIT_START).as_secs_f32();
        let fraction = (over / SPLIT_RAMP.as_secs_f32()).clamp(0.0, 1.0);
        let take = ((fraction * (total - 1) as f32).round() as u32 + 1).clamp(1, total);
        Some((fraction, take))
    }

    /// Current Bedrock green split-bar overlay, if a stack is being held to split.
    pub fn split_overlay(
        &self,
        now: Instant,
        style: ContainerTouchStyle,
        carried_count: u32,
    ) -> Option<SplitOverlay> {
        if style != ContainerTouchStyle::Bedrock || carried_count > 0 || self.quick_move {
            return None;
        }
        let (_, Gesture::Slot {
            info,
            started_at,
            scrolling: false,
            quick_crafting: false,
            hold_committed: false,
            ..
        }) = self.active.as_ref()?
        else {
            return None;
        };
        if !info.splittable || info.count < 2 {
            return None;
        }
        let (fraction, take) = Self::split_amount(now.saturating_duration_since(*started_at), info.count)?;
        Some(SplitOverlay {
            slot_rect: info.rect,
            fraction,
            take,
            total: info.count,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn on_start(
        &mut self,
        id: u64,
        gui_pt: [f32; 2],
        slot_info: Option<SlotTouchInfo>,
        carried_count: u32,
        style: ContainerTouchStyle,
        panel_rect: [f32; 4],
        gui_size: [f32; 2],
        safe_left_gui: f32,
        now: Instant,
    ) -> ContainerAction {
        if let Some(btn) = Self::hit_toolbar(style, panel_rect, gui_size, safe_left_gui, gui_pt) {
            self.active = Some((id, Gesture::Toolbar));
            return match btn {
                ToolBtn::QuickMove => {
                    self.quick_move = !self.quick_move;
                    if self.quick_move {
                        self.half_mode = false;
                    }
                    ContainerAction::None
                }
                ToolBtn::Half => {
                    self.half_mode = !self.half_mode;
                    if self.half_mode {
                        self.quick_move = false;
                    }
                    ContainerAction::None
                }
                ToolBtn::Drop => {
                    if carried_count > 0 {
                        ContainerAction::DropCarried {
                            all: !self.half_mode,
                        }
                    } else {
                        ContainerAction::None
                    }
                }
            };
        }

        if let Some(info) = slot_info {
            self.active = Some((
                id,
                Gesture::Slot {
                    start_gui: gui_pt,
                    last_y: gui_pt[1],
                    info,
                    started_at: now,
                    scrolling: false,
                    quick_crafting: false,
                    hold_committed: false,
                    last_drip: None,
                },
            ));
            return ContainerAction::None;
        }

        let inside_panel = gui_pt[0] >= panel_rect[0]
            && gui_pt[0] <= panel_rect[0] + panel_rect[2]
            && gui_pt[1] >= panel_rect[1]
            && gui_pt[1] <= panel_rect[1] + panel_rect[3];
        if !inside_panel && style == ContainerTouchStyle::Bedrock && carried_count > 0 {
            self.active = Some((
                id,
                Gesture::OutsideSafe {
                    start_gui: gui_pt,
                    last_y: gui_pt[1],
                    scrolling: false,
                },
            ));
            return ContainerAction::None;
        }

        self.active = Some((
            id,
            Gesture::PassThrough {
                start_gui: gui_pt,
                last_y: gui_pt[1],
                scrolling: false,
            },
        ));
        ContainerAction::PassThroughPress(gui_pt)
    }

    pub fn on_move(
        &mut self,
        id: u64,
        gui_pt: [f32; 2],
        hovered_slot: Option<usize>,
        carried_count: u32,
    ) -> Vec<ContainerAction> {
        let Some((active_id, gesture)) = self.active.as_mut() else {
            return Vec::new();
        };
        if *active_id != id {
            return Vec::new();
        }
        let mut out = Vec::new();
        match gesture {
            Gesture::Toolbar => {}
            Gesture::Slot {
                start_gui,
                last_y,
                info,
                scrolling,
                quick_crafting,
                hold_committed,
                ..
            } => {
                let dist = (gui_pt[0] - start_gui[0]).hypot(gui_pt[1] - start_gui[1]);
                let dy = gui_pt[1] - *last_y;
                *last_y = gui_pt[1];
                if *quick_crafting {
                    if let Some(s) = hovered_slot {
                        out.push(ContainerAction::DragSlot(s));
                    }
                    return out;
                }
                if !*scrolling && !*hold_committed && dist >= MOVE_SLOP_GUI {
                    if carried_count > 0 && info.splittable {
                        *quick_crafting = true;
                        out.push(ContainerAction::DragStart {
                            slot: info.slot,
                            right: self.half_mode,
                        });
                        if let Some(s) = hovered_slot {
                            out.push(ContainerAction::DragSlot(s));
                        }
                        return out;
                    }
                    *scrolling = true;
                }
                if *scrolling && dy.abs() > 0.2 {
                    out.push(ContainerAction::Scroll(dy / 12.0));
                }
            }
            Gesture::OutsideSafe {
                start_gui,
                last_y,
                scrolling,
            }
            | Gesture::PassThrough {
                start_gui,
                last_y,
                scrolling,
            } => {
                let dx = (gui_pt[0] - start_gui[0]).abs();
                let dy_total = (gui_pt[1] - start_gui[1]).abs();
                let dy = gui_pt[1] - *last_y;
                *last_y = gui_pt[1];
                if (*scrolling || (dy_total >= MOVE_SLOP_GUI && dy_total > dx)) && dy.abs() > 0.2 {
                    *scrolling = true;
                    out.push(ContainerAction::Scroll(dy / 12.0));
                }
            }
        }
        out
    }

    /// Periodic tick for Classic hold-split and drip-placing single items.
    pub fn poll(
        &mut self,
        now: Instant,
        style: ContainerTouchStyle,
        carried_count: u32,
    ) -> ContainerAction {
        let Some((_, Gesture::Slot {
            info,
            started_at,
            scrolling: false,
            quick_crafting: false,
            hold_committed,
            last_drip,
            ..
        })) = self.active.as_mut()
        else {
            return ContainerAction::None;
        };
        if self.quick_move || !info.splittable {
            return ContainerAction::None;
        }
        let elapsed = now.saturating_duration_since(*started_at);
        if carried_count == 0 {
            if style == ContainerTouchStyle::Classic
                && !*hold_committed
                && info.count > 0
                && elapsed >= CLASSIC_HOLD
            {
                *hold_committed = true;
                return ContainerAction::SlotClick {
                    slot: info.slot,
                    right: true,
                    quick_move: false,
                };
            }
        } else if elapsed >= DRIP_START {
            let due = last_drip.is_none_or(|t| now.saturating_duration_since(t) >= DRIP_INTERVAL);
            if due {
                *hold_committed = true;
                *last_drip = Some(now);
                return ContainerAction::SlotClick {
                    slot: info.slot,
                    right: true,
                    quick_move: false,
                };
            }
        }
        ContainerAction::None
    }

    #[allow(clippy::too_many_arguments)]
    pub fn on_end(
        &mut self,
        id: u64,
        gui_pt: [f32; 2],
        carried_count: u32,
        style: ContainerTouchStyle,
        panel_rect: [f32; 4],
        gui_size: [f32; 2],
        safe_left_gui: f32,
        now: Instant,
    ) -> ContainerAction {
        let Some((_, gesture)) = self.active.take().filter(|(aid, _)| *aid == id) else {
            return ContainerAction::None;
        };
        match gesture {
            Gesture::Toolbar | Gesture::OutsideSafe { .. } => ContainerAction::None,
            Gesture::PassThrough { .. } => ContainerAction::PassThroughRelease(gui_pt),
            Gesture::Slot {
                info,
                started_at,
                scrolling,
                quick_crafting,
                hold_committed,
                ..
            } => {
                if quick_crafting {
                    if carried_count > 0
                        && Self::hit_toolbar(style, panel_rect, gui_size, safe_left_gui, gui_pt)
                            == Some(ToolBtn::Drop)
                    {
                        return ContainerAction::DropCarried {
                            all: !self.half_mode,
                        };
                    }
                    return ContainerAction::DragEnd;
                }
                if scrolling || hold_committed {
                    return ContainerAction::None;
                }
                let elapsed = now.saturating_duration_since(started_at);
                if style == ContainerTouchStyle::Bedrock
                    && carried_count == 0
                    && !self.quick_move
                    && info.splittable
                    && let Some((_, take)) = Self::split_amount(elapsed, info.count)
                {
                    return ContainerAction::SplitExact {
                        slot: info.slot,
                        take,
                        total: info.count,
                    };
                }
                ContainerAction::SlotClick {
                    slot: info.slot,
                    right: self.half_mode,
                    quick_move: self.quick_move,
                }
            }
        }
    }

    /// Draws the side toolbar (`[⇧]`, `[½]`, `[🗑]`) and any active Bedrock split bar.
    pub fn draw(
        &self,
        b: &mut HudBatch,
        a: &HudAtlas,
        style: ContainerTouchStyle,
        panel_rect: [f32; 4],
        safe_left_gui: f32,
        carried_count: u32,
        now: Instant,
    ) {
        for (btn, [x, y, w, h]) in
            Self::toolbar_rects(style, panel_rect, [b.width, b.height], safe_left_gui)
        {
            let active = match btn {
                ToolBtn::QuickMove => self.quick_move,
                ToolBtn::Half => self.half_mode,
                ToolBtn::Drop => false,
            };
            let bg = match (btn, active) {
                (ToolBtn::QuickMove, true) => rgba(36, 130, 62, 215),
                (ToolBtn::Half, true) => rgba(38, 108, 168, 215),
                (ToolBtn::Drop, _) if carried_count > 0 => rgba(135, 38, 38, 195),
                _ => rgba(16, 16, 22, 175),
            };
            let border = if active {
                rgba(130, 255, 150, 245)
            } else {
                rgba(210, 210, 220, 185)
            };
            b.fill(a, x, y, w, h, bg);
            b.fill(a, x, y, w, 1.0, border);
            b.fill(a, x, y + h - 1.0, w, 1.0, border);
            b.fill(a, x, y, 1.0, h, border);
            b.fill(a, x + w - 1.0, y, 1.0, h, border);

            let label = match btn {
                ToolBtn::QuickMove => ">>",
                ToolBtn::Half => "1/2",
                ToolBtn::Drop => " Q ",
            };
            let tw = HudBatch::text_width(a, label);
            let scale = ((w - 4.0) / tw.max(1.0)).min(1.0);
            let start = b.vertices.len();
            b.text(
                a,
                label,
                x + (w - tw) / 2.0,
                y + (h - 8.0) / 2.0,
                rgba(255, 255, 255, 240),
                true,
            );
            if scale < 1.0 {
                b.scale_since(start, x + w / 2.0, y + h / 2.0, scale, scale);
            }
        }

        if let Some(split) = self.split_overlay(now, style, carried_count) {
            let [sx, sy, sw, _] = split.slot_rect;
            let bar_w = sw.max(20.0);
            let bar_h = 4.0;
            let bx = sx + (sw - bar_w) / 2.0;
            let by = (sy - 7.0).max(2.0);
            b.fill(a, bx - 1.0, by - 1.0, bar_w + 2.0, bar_h + 2.0, rgba(0, 0, 0, 210));
            b.fill(
                a,
                bx,
                by,
                (bar_w * split.fraction).clamp(1.0, bar_w),
                bar_h,
                rgba(85, 235, 85, 245),
            );
            let badge = format!("{}/{}", split.take, split.total);
            let tw = HudBatch::text_width(a, &badge);
            let tx = sx + (sw - tw * 0.75) / 2.0;
            let ty = (by - 8.0).max(1.0);
            let start = b.vertices.len();
            b.text(a, &badge, tx, ty, rgba(160, 255, 160, 255), true);
            b.scale_since(start, sx + sw / 2.0, ty + 4.0, 0.75, 0.75);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(count: u32) -> Option<SlotTouchInfo> {
        Some(SlotTouchInfo {
            slot: 10,
            rect: [100.0, 80.0, 16.0, 16.0],
            count,
            splittable: true,
        })
    }

    #[test]
    fn bedrock_hold_ramps_from_one_to_full_stack_without_premature_pickup() {
        let mut ct = ContainerTouch::default();
        let panel = [80.0, 40.0, 176.0, 166.0];
        let gui = [427.0, 240.0];
        let t0 = Instant::now();
        // Touch-down does NOT pick up all 64 items immediately!
        assert_eq!(
            ct.on_start(1, [108.0, 88.0], slot(64), 0, ContainerTouchStyle::Bedrock, panel, gui, 0.0, t0),
            ContainerAction::None
        );
        // At +580ms (halfway through 720ms ramp), takes ~32-33 items.
        let mid = t0 + Duration::from_millis(220 + 360);
        let ov = ct.split_overlay(mid, ContainerTouchStyle::Bedrock, 0).unwrap();
        assert!((31..=34).contains(&ov.take), "got {}", ov.take);
        let act = ct.on_end(1, [108.0, 88.0], 0, ContainerTouchStyle::Bedrock, panel, gui, 0.0, mid);
        assert_eq!(
            act,
            ContainerAction::SplitExact {
                slot: 10,
                take: ov.take,
                total: 64
            }
        );
    }

    #[test]
    fn classic_long_press_fires_single_right_click_split_and_never_re_clicks_on_lift() {
        let mut ct = ContainerTouch::default();
        let panel = [80.0, 40.0, 176.0, 166.0];
        let gui = [427.0, 240.0];
        let t0 = Instant::now();
        assert_eq!(
            ct.on_start(1, [108.0, 88.0], slot(64), 0, ContainerTouchStyle::Classic, panel, gui, 0.0, t0),
            ContainerAction::None
        );
        let t1 = t0 + Duration::from_millis(350);
        assert_eq!(
            ct.poll(t1, ContainerTouchStyle::Classic, 0),
            ContainerAction::SlotClick {
                slot: 10,
                right: true,
                quick_move: false
            }
        );
        // Lifting afterwards fires nothing (fixes the 64 -> 63 bug!).
        assert_eq!(
            ct.on_end(1, [108.0, 88.0], 32, ContainerTouchStyle::Classic, panel, gui, 0.0, t1 + Duration::from_millis(50)),
            ContainerAction::None
        );
    }

    #[test]
    fn bedrock_protects_carried_items_from_accidental_outside_taps_and_supports_toolbar() {
        let mut ct = ContainerTouch::default();
        let panel = [80.0, 40.0, 176.0, 166.0];
        let gui = [427.0, 240.0];
        let t0 = Instant::now();
        // Outside tap while carrying 64 diamonds in Bedrock mode does NOT drop them!
        assert_eq!(
            ct.on_start(1, [10.0, 10.0], None, 64, ContainerTouchStyle::Bedrock, panel, gui, 0.0, t0),
            ContainerAction::None
        );
        assert_eq!(
            ct.on_end(1, [10.0, 10.0], 64, ContainerTouchStyle::Bedrock, panel, gui, 0.0, t0 + Duration::from_millis(80)),
            ContainerAction::None
        );
        // Tapping QuickMove toolbar button latches quick_move.
        let rects = ContainerTouch::toolbar_rects(ContainerTouchStyle::Bedrock, panel, gui, 0.0);
        let qm_center = [rects[0].1[0] + 11.0, rects[0].1[1] + 11.0];
        ct.on_start(2, qm_center, None, 0, ContainerTouchStyle::Bedrock, panel, gui, 0.0, t0);
        ct.on_end(2, qm_center, 0, ContainerTouchStyle::Bedrock, panel, gui, 0.0, t0);
        assert!(ct.quick_move);
        // Next short tap on a slot sends QuickMove!
        ct.on_start(3, [108.0, 88.0], slot(64), 0, ContainerTouchStyle::Bedrock, panel, gui, 0.0, t0);
        assert_eq!(
            ct.on_end(3, [108.0, 88.0], 0, ContainerTouchStyle::Bedrock, panel, gui, 0.0, t0 + Duration::from_millis(60)),
            ContainerAction::SlotClick {
                slot: 10,
                right: false,
                quick_move: true
            }
        );
    }

    #[test]
    fn swiping_over_slots_empty_handed_scrolls_without_picking_up_items() {
        let mut ct = ContainerTouch::default();
        let panel = [80.0, 40.0, 195.0, 136.0];
        let gui = [427.0, 240.0];
        let t0 = Instant::now();
        ct.on_start(1, [108.0, 88.0], slot(1), 0, ContainerTouchStyle::Bedrock, panel, gui, 0.0, t0);
        let moves = ct.on_move(1, [108.0, 108.0], Some(19), 0);
        assert!(moves.iter().any(|m| matches!(m, ContainerAction::Scroll(_))));
        assert_eq!(
            ct.on_end(1, [108.0, 108.0], 0, ContainerTouchStyle::Bedrock, panel, gui, 0.0, t0 + Duration::from_millis(120)),
            ContainerAction::None
        );
    }
}
