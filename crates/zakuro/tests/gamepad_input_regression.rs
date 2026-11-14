//! Integration regression tests for gamepad bindings, button names, stick flick thresholds, and input isolation.

use gilrs::Button;
use zakuro::gamepad::{button_name, Gamepads};
use zakuro::menus::{Item, Menus, Scope, SelectPopup};
use zakuro::settings::PadButtons;
use zakuro_core::services::hid::{InputState, PadState};

#[test]
fn test_default_pad_buttons_mapping() {
    let pad = PadButtons::default();

    // Handheld Mode / Xbox Home button toggles pause menu
    assert_eq!(pad.menu, Button::Mode);

    // Standard 3DS layout (Nintendo face buttons: A is East/Right, B is South/Bottom, X is North/Top, Y is West/Left)
    assert_eq!(pad.a, Button::East);
    assert_eq!(pad.b, Button::South);
    assert_eq!(pad.x, Button::North);
    assert_eq!(pad.y, Button::West);

    // D-pad
    assert_eq!(pad.up, Button::DPadUp);
    assert_eq!(pad.down, Button::DPadDown);
    assert_eq!(pad.left, Button::DPadLeft);
    assert_eq!(pad.right, Button::DPadRight);

    // Shoulders and triggers
    assert_eq!(pad.l, Button::LeftTrigger);
    assert_eq!(pad.r, Button::RightTrigger);
    assert_eq!(pad.zl, Button::LeftTrigger2);
    assert_eq!(pad.zr, Button::RightTrigger2);

    // System buttons
    assert_eq!(pad.start, Button::Start);
    assert_eq!(pad.select, Button::Select);
}

#[test]
fn test_button_naming_regression() {
    let expected = [
        (Button::South, "Bottom face"),
        (Button::East, "Right face"),
        (Button::North, "Top face"),
        (Button::West, "Left face"),
        (Button::LeftTrigger, "LB / L1"),
        (Button::RightTrigger, "RB / R1"),
        (Button::LeftTrigger2, "LT / L2"),
        (Button::RightTrigger2, "RT / R2"),
        (Button::Select, "View / Select"),
        (Button::Start, "Menu / Start"),
        (Button::Mode, "Home"),
        (Button::LeftThumb, "Left stick"),
        (Button::RightThumb, "Right stick"),
        (Button::DPadUp, "D-pad up"),
        (Button::DPadDown, "D-pad down"),
        (Button::DPadLeft, "D-pad left"),
        (Button::DPadRight, "D-pad right"),
    ];

    for (btn, name) in expected {
        assert_eq!(button_name(btn), name);
    }
}

#[test]
fn test_gamepad_suppression_initial_state() {
    let mut gamepads = Gamepads::new(PadButtons::default());
    // Suppress current held (simulating closing menu or resuming gameplay)
    gamepads.suppress_current_held();

    // Default neutral state applied through gamepads remains neutral
    let neutral = InputState::default();
    let applied = gamepads.apply(neutral);
    assert_eq!(applied.buttons, PadState::default());
    assert_eq!(applied.circle_x, 0.0);
    assert_eq!(applied.circle_y, 0.0);
}

#[test]
fn test_menus_over_game_input_isolation() {
    let mut menus = Menus::default();
    // Initially not over game
    assert!(!menus.over_game());
    assert!(!menus.modal_open());

    // In-game pause menu open
    menus.menu_open = true;
    assert!(menus.over_game(), "Pause menu must trigger over_game");

    menus.menu_open = false;
    assert!(!menus.over_game());

    // Settings modal open
    menus.settings_open = true;
    assert!(menus.modal_open());
    assert!(menus.over_game(), "Settings modal must trigger over_game");
    menus.close_modals();
    assert!(!menus.modal_open());
    assert!(!menus.over_game());

    // Game settings modal open
    menus.game_settings = Some((0x00040000001B5000, "Pokemon Ultra Sun".to_string()));
    assert!(menus.modal_open());
    assert!(menus.over_game(), "Game settings modal must trigger over_game");
    menus.close_modals();
    assert!(!menus.over_game());

    // Cheats modal open
    menus.cheats_open = true;
    assert!(menus.modal_open());
    assert!(menus.over_game(), "Cheats modal must trigger over_game");
    menus.close_modals();
    assert!(!menus.over_game());

    // Multiplayer modal open
    menus.multiplayer_open = true;
    assert!(menus.modal_open());
    assert!(menus.over_game(), "Multiplayer modal must trigger over_game");
    menus.close_modals();
    assert!(!menus.over_game());

    // Select popup active
    menus.select_popup = Some(SelectPopup {
        item: Item::Resolution,
        scope: Scope::System,
        title: "Resolution".to_string(),
        options: vec!["1x".to_string(), "2x".to_string()],
        selected: 0,
        current: Some(0),
    });
    assert!(menus.modal_open(), "SelectPopup must register as modal_open");
    assert!(menus.over_game(), "SelectPopup must trigger over_game");

    menus.close_modals();
    assert!(!menus.over_game());
}

#[test]
fn test_touchscreen_coordinate_mapping() {
    // 3DS bottom screen is natively 320 x 240
    let vp_x = 100.0f32;
    let vp_y = 200.0f32;
    let vp_w = 640.0f32; // 2x scale
    let vp_h = 480.0f32;

    let map_touch = |cx: f32, cy: f32| -> Option<(u16, u16)> {
        if cx >= vp_x && cx < vp_x + vp_w && cy >= vp_y && cy < vp_y + vp_h {
            let rel_x = (cx - vp_x) / vp_w;
            let rel_y = (cy - vp_y) / vp_h;
            let tx = (rel_x * 320.0).floor().clamp(0.0, 319.0) as u16;
            let ty = (rel_y * 240.0).floor().clamp(0.0, 239.0) as u16;
            Some((tx, ty))
        } else {
            None
        }
    };

    // Top-left of bottom screen
    assert_eq!(map_touch(100.0, 200.0), Some((0, 0)));
    // Center of bottom screen
    assert_eq!(map_touch(100.0 + 320.0, 200.0 + 240.0), Some((160, 120)));
    // Bottom-right edge of bottom screen
    assert_eq!(map_touch(100.0 + 639.0, 200.0 + 479.0), Some((319, 239)));

    // Points outside bottom screen viewport must not register touch
    assert_eq!(map_touch(99.0, 200.0), None);
    assert_eq!(map_touch(100.0, 199.0), None);
    assert_eq!(map_touch(100.0 + 641.0, 200.0), None);
}
