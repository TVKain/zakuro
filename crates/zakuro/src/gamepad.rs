//! game controllers, any the system knows, read through gilrs, and the
//! Switch's Pro Controller on macOS read from its reports, see
//! pro_controller.

use gilrs::{Axis, Button, EventType, Gamepad, Gilrs};
use zakuro_core::services::hid::{InputState, PadState};

use crate::pro_controller::{is_pro_controller, Controllers, Report};
use crate::settings::PadButtons;

/// how far a stick has to lean before it counts, some rest a little off
/// center.
const DEADZONE: f32 = 0.15;

pub struct Gamepads {
    /// none when the system could not be asked for controllers.
    gilrs: Option<Gilrs>,
    buttons: PadButtons,
    /// Pro Controllers read from their own reports, on macOS.
    pro: Controllers,
    /// the buttons they held at the last poll, to tell the ones pressed
    /// since.
    pro_held: Vec<Button>,
    stick_up: bool,
    stick_down: bool,
    stick_left: bool,
    stick_right: bool,
    dpad_axis_up: bool,
    dpad_axis_down: bool,
    dpad_axis_left: bool,
    dpad_axis_right: bool,
    suppressed_buttons: Vec<Button>,
    suppress_sticks: bool,
}

/// detects stick or axis flicks with hysteresis
fn update_axis_flick(
    val: f32,
    state_pos: &mut bool,
    state_neg: &mut bool,
    btn_pos: Button,
    btn_neg: Button,
    pressed: &mut Vec<Button>,
) {
    if val > 0.55 {
        if !*state_pos {
            *state_pos = true;
            pressed.push(btn_pos);
        }
    } else if val < 0.25 {
        *state_pos = false;
    }

    if val < -0.55 {
        if !*state_neg {
            *state_neg = true;
            pressed.push(btn_neg);
        }
    } else if val > -0.25 {
        *state_neg = false;
    }
}

/// whether gilrs's reading of a controller is left out, a Pro Controller's
/// while one is read from its reports.
fn left_out(pad: &Gamepad, reports: &[Report]) -> bool {
    !reports.is_empty() && is_pro_controller(pad.vendor_id(), pad.product_id(), pad.name())
}

impl Gamepads {
    pub fn new(buttons: PadButtons) -> Gamepads {
        let gilrs = Gilrs::new()
            .inspect_err(|error| log::warn!("controllers are not available, {error}"))
            .ok();
        Gamepads {
            gilrs,
            buttons,
            pro: Controllers::new(),
            pro_held: Vec::new(),
            stick_up: false,
            stick_down: false,
            stick_left: false,
            stick_right: false,
            dpad_axis_up: false,
            dpad_axis_down: false,
            dpad_axis_left: false,
            dpad_axis_right: false,
            suppressed_buttons: Vec::new(),
            suppress_sticks: false,
        }
    }

    pub fn set_buttons(&mut self, buttons: PadButtons) {
        self.buttons = buttons;
    }

    /// takes in what the controllers did since the last call, and returns the
    /// buttons pressed meanwhile.
    pub fn poll(&mut self) -> Vec<Button> {
        let mut pressed = Vec::new();
        let reports = self.pro.reports();
        if let Some(gilrs) = &mut self.gilrs {
            while let Some(event) = gilrs.next_event() {
                match event.event {
                    EventType::ButtonPressed(button, _) if !left_out(&gilrs.gamepad(event.id), &reports) => pressed.push(button),
                    EventType::Connected => {
                        log::info!("controller connected, {}", gilrs.gamepad(event.id).name());
                    }
                    EventType::Disconnected => log::info!("controller disconnected"),
                    _ => {}
                }
            }
            for (_, pad) in gilrs.gamepads() {
                if left_out(&pad, &reports) {
                    continue;
                }
                update_axis_flick(pad.value(Axis::LeftStickY), &mut self.stick_up, &mut self.stick_down, Button::DPadUp, Button::DPadDown, &mut pressed);
                update_axis_flick(pad.value(Axis::LeftStickX), &mut self.stick_right, &mut self.stick_left, Button::DPadRight, Button::DPadLeft, &mut pressed);
                update_axis_flick(pad.value(Axis::DPadY), &mut self.dpad_axis_up, &mut self.dpad_axis_down, Button::DPadUp, Button::DPadDown, &mut pressed);
                update_axis_flick(pad.value(Axis::DPadX), &mut self.dpad_axis_right, &mut self.dpad_axis_left, Button::DPadRight, Button::DPadLeft, &mut pressed);
            }
        }
        let held: Vec<Button> = reports.iter().flat_map(|report| report.buttons.iter().copied()).collect();
        pressed.extend(held.iter().filter(|button| !self.pro_held.contains(button)));
        self.pro_held = held;
        pressed
    }

    /// whether a controller holds button down.
    pub fn holding(&self, button: Button) -> bool {
        let reports = self.pro.reports();
        reports.iter().any(|report| report.buttons.contains(&button))
            || self.gilrs.as_ref().is_some_and(|gilrs| {
                gilrs.gamepads().any(|(_, pad)| !left_out(&pad, &reports) && pad.is_pressed(button))
            })
    }

    /// marks all currently held buttons and active stick deflections as suppressed
    /// until they are released / recentered, preventing inputs from menu navigation
    /// or modal closure from bleeding into the emulated game.
    pub fn suppress_current_held(&mut self) {
        let reports = self.pro.reports();
        let mut held = Vec::new();
        for report in &reports {
            held.extend(report.buttons.iter().copied());
        }
        if let Some(gilrs) = &self.gilrs {
            for (_, pad) in gilrs.gamepads() {
                if left_out(&pad, &reports) {
                    continue;
                }
                for (_, bound) in self.buttons.map() {
                    if pad.is_pressed(bound) {
                        held.push(bound);
                    }
                }
                for special in [
                    Button::South,
                    Button::East,
                    Button::North,
                    Button::West,
                    Button::Start,
                    Button::Select,
                    Button::Mode,
                    Button::LeftTrigger,
                    Button::RightTrigger,
                    Button::LeftTrigger2,
                    Button::RightTrigger2,
                    Button::DPadUp,
                    Button::DPadDown,
                    Button::DPadLeft,
                    Button::DPadRight,
                ] {
                    if pad.is_pressed(special) {
                        held.push(special);
                    }
                }
            }
        }
        self.suppressed_buttons = held;
        self.suppress_sticks = true;
    }

    /// what the controllers hold down, over what the keyboard does.
    pub fn apply(&mut self, mut state: InputState) -> InputState {
        // Unsuppress any buttons that have been physically released
        let held_buttons = std::mem::take(&mut self.suppressed_buttons);
        self.suppressed_buttons = held_buttons.into_iter().filter(|&b| self.holding(b)).collect();

        let reports = self.pro.reports();
        for report in &reports {
            for (button, bound) in self.buttons.map() {
                if !self.suppressed_buttons.contains(&bound) && report.buttons.contains(&bound) {
                    state.buttons.set(button, true);
                }
            }
            if !self.suppress_sticks {
                sticks(&mut state, report.left, report.right);
            }
        }
        let Some(gilrs) = &self.gilrs else { return state };

        let mut sticks_centered = true;
        for (_, pad) in gilrs.gamepads() {
            if left_out(&pad, &reports) {
                continue;
            }
            for (button, bound) in self.buttons.map() {
                if !self.suppressed_buttons.contains(&bound) && pad.is_pressed(bound) {
                    state.buttons.set(button, true);
                }
            }
            // the d-pad of some controllers is a pair of axes
            let dpad = [
                (PadState::RIGHT, Button::DPadRight, pad.value(Axis::DPadX) > 0.5),
                (PadState::LEFT, Button::DPadLeft, pad.value(Axis::DPadX) < -0.5),
                (PadState::UP, Button::DPadUp, pad.value(Axis::DPadY) > 0.5),
                (PadState::DOWN, Button::DPadDown, pad.value(Axis::DPadY) < -0.5),
            ];
            for (button, gilrs_btn, held) in dpad {
                if held && !self.suppressed_buttons.contains(&gilrs_btn) {
                    state.buttons.set(button, true);
                }
            }
            let left = [pad.value(Axis::LeftStickX), pad.value(Axis::LeftStickY)];
            let right = [pad.value(Axis::RightStickX), pad.value(Axis::RightStickY)];
            if left[0].hypot(left[1]) > DEADZONE || right[0].hypot(right[1]) > DEADZONE {
                sticks_centered = false;
            }
            if !self.suppress_sticks {
                sticks(&mut state, left, right);
            }
        }
        if sticks_centered {
            self.suppress_sticks = false;
        }
        state
    }
}

/// the left stick is the circle pad and the right one the C-stick, up
/// positive as the console has it, each once it leans past the deadzone.
fn sticks(state: &mut InputState, [x, y]: [f32; 2], [cx, cy]: [f32; 2]) {
    if x.hypot(y) > DEADZONE {
        state.circle_x = x.clamp(-1.0, 1.0);
        state.circle_y = y.clamp(-1.0, 1.0);
    }
    if cx.hypot(cy) > DEADZONE {
        state.c_stick_x = cx.clamp(-1.0, 1.0);
        state.c_stick_y = cy.clamp(-1.0, 1.0);
    }
}

/// a controller button as a person would call it.
pub fn button_name(button: Button) -> &'static str {
    match button {
        Button::South => "Bottom face",
        Button::East => "Right face",
        Button::North => "Top face",
        Button::West => "Left face",
        Button::LeftTrigger => "LB / L1",
        Button::RightTrigger => "RB / R1",
        Button::LeftTrigger2 => "LT / L2",
        Button::RightTrigger2 => "RT / R2",
        Button::Select => "View / Select",
        Button::Start => "Menu / Start",
        Button::Mode => "Home",
        Button::LeftThumb => "Left stick",
        Button::RightThumb => "Right stick",
        Button::DPadUp => "D-pad up",
        Button::DPadDown => "D-pad down",
        Button::DPadLeft => "D-pad left",
        Button::DPadRight => "D-pad right",
        Button::C => "C",
        Button::Z => "Z",
        Button::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// with no controller, or no way to ask for one, the keyboard's input
    /// goes through as it was.
    #[test]
    fn no_controller_leaves_the_keyboard_alone() {
        let mut gamepads = Gamepads::new(PadButtons::default());
        if gamepads.gilrs.as_ref().is_some_and(|gilrs| gilrs.gamepads().next().is_some()) {
            return;
        }
        let state = InputState { buttons: PadState::A, circle_x: 1.0, circle_y: 0.0, touch: Some((1, 2)), ..InputState::default() };
        let applied = gamepads.apply(state);
        assert_eq!(applied.buttons, PadState::A);
        assert_eq!((applied.circle_x, applied.circle_y, applied.touch), (1.0, 0.0, Some((1, 2))));
    }

    #[test]
    fn suppression_preserves_neutral_gameplay() {
        let mut gamepads = Gamepads::new(PadButtons::default());
        gamepads.suppress_current_held();
        let state = InputState::default();
        let applied = gamepads.apply(state);
        assert_eq!(applied.buttons, PadState::default());
    }

    #[test]
    fn test_update_axis_flick_hysteresis() {
        let mut up = false;
        let mut down = false;
        let mut pressed = Vec::new();

        // Below threshold - no activation
        update_axis_flick(0.5, &mut up, &mut down, Button::DPadUp, Button::DPadDown, &mut pressed);
        assert!(!up);
        assert!(pressed.is_empty());

        // Above 0.55 threshold - activates once
        update_axis_flick(0.7, &mut up, &mut down, Button::DPadUp, Button::DPadDown, &mut pressed);
        assert!(up);
        assert_eq!(pressed, vec![Button::DPadUp]);

        // Still above reset threshold (0.25) - does not re-trigger
        update_axis_flick(0.4, &mut up, &mut down, Button::DPadUp, Button::DPadDown, &mut pressed);
        assert!(up);
        assert_eq!(pressed.len(), 1);

        // Drops below 0.25 - resets
        update_axis_flick(0.1, &mut up, &mut down, Button::DPadUp, Button::DPadDown, &mut pressed);
        assert!(!up);

        // Negative axis flick
        update_axis_flick(-0.8, &mut up, &mut down, Button::DPadUp, Button::DPadDown, &mut pressed);
        assert!(down);
        assert_eq!(pressed, vec![Button::DPadUp, Button::DPadDown]);

        // Resets when above -0.25
        update_axis_flick(-0.1, &mut up, &mut down, Button::DPadUp, Button::DPadDown, &mut pressed);
        assert!(!down);
    }
}
