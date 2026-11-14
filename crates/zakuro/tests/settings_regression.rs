//! Integration regression tests for Zakuro settings, serialization, and per-game overrides.

use std::time::Duration;
use gilrs::Button;
use zakuro::settings::{
    game_key, Filter, FpsLimit, GameSettings, Orientation, Renderer, Screens,
    Settings, Stereo3d, TopShape, WindowMode,
};

#[test]
fn test_default_settings_invariants() {
    let settings = Settings::default();

    // Verify critical handheld and architecture defaults
    assert_eq!(settings.renderer, Renderer::Vulkan);
    assert!(settings.hardware_rasterizer);
    assert_eq!(settings.resolution, 3);
    assert!(settings.texture_packs);
    assert!(!settings.frame_interpolation);
    assert!(settings.recompiled);
    assert_eq!(settings.window_mode, WindowMode::Maximized);
    assert_eq!(settings.fps_limit, FpsLimit::Fps60);
    assert_eq!(settings.layout, Screens::Stacked);
    assert_eq!(settings.orientation, Orientation::North);
    assert_eq!(settings.filter, Filter::Smooth);
    assert!(!settings.integer_scale);
    assert_eq!(settings.top_shape, TopShape::Console);
    assert_eq!(settings.stereo, Stereo3d::Off);
    assert_eq!(settings.depth_3d, 50);
    assert!(!settings.show_fps);
    assert_eq!(settings.pad.menu, Button::Mode);
}

use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Wrap<T> {
    val: T,
}

#[test]
fn test_fps_limit_serialization_and_presentation_time() {
    let cases = [
        (FpsLimit::Fps30, "30", Some(Duration::from_nanos(1_000_000_000 / 30))),
        (FpsLimit::Fps60, "60", Some(Duration::from_nanos(1_000_000_000 / 60))),
        (FpsLimit::Fps90, "90", Some(Duration::from_nanos(1_000_000_000 / 90))),
        (FpsLimit::Fps120, "120", Some(Duration::from_nanos(1_000_000_000 / 120))),
        (FpsLimit::Uncapped, "uncapped", None),
    ];

    for (limit, expected_str, expected_dur) in cases {
        assert_eq!(limit.presentation_time(), expected_dur);

        // Serialize wrapped to TOML
        let toml_str = toml::to_string(&Wrap { val: limit }).expect("failed to serialize FpsLimit");
        assert!(
            toml_str.contains(expected_str),
            "expected {expected_str} in {toml_str}"
        );

        // Round-trip deserialization
        let deserialized: Wrap<FpsLimit> =
            toml::from_str(&toml_str).expect("failed to deserialize FpsLimit");
        assert_eq!(deserialized.val, limit);
        assert_eq!(deserialized.val.presentation_time(), expected_dur);
    }
}

#[test]
fn test_filter_serialization_and_display() {
    let cases = [
        (Filter::Smooth, "smooth", "Smooth"),
        (Filter::Pixels, "pixels", "Pixels"),
        (Filter::Sharp, "sharp", "Sharp"),
        (Filter::Xbrz, "xbrz", "xBRZ"),
        (Filter::XbrzLinear, "xbrz_linear", "xBRZ + Linear"),
    ];

    for (filter, expected_str, expected_name) in cases {
        assert_eq!(filter.name(), expected_name);

        let toml_str = toml::to_string(&Wrap { val: filter }).expect("failed to serialize Filter");
        assert!(
            toml_str.contains(expected_str),
            "expected {expected_str} in {toml_str}"
        );

        let deserialized: Wrap<Filter> =
            toml::from_str(&toml_str).expect("failed to deserialize Filter");
        assert_eq!(deserialized.val, filter);
        assert_eq!(deserialized.val.name(), expected_name);
    }
}

#[test]
fn test_window_mode_serialization() {
    let cases = [
        (WindowMode::Maximized, "maximized", "Maximized"),
        (WindowMode::Fullscreen, "fullscreen", "Fullscreen"),
        (WindowMode::Windowed, "windowed", "Windowed"),
    ];

    for (mode, expected_str, expected_name) in cases {
        assert_eq!(mode.name(), expected_name);

        let toml_str = toml::to_string(&Wrap { val: mode }).expect("failed to serialize WindowMode");
        assert!(
            toml_str.contains(expected_str),
            "expected {expected_str} in {toml_str}"
        );

        let deserialized: Wrap<WindowMode> =
            toml::from_str(&toml_str).expect("failed to deserialize WindowMode");
        assert_eq!(deserialized.val, mode);
        assert_eq!(deserialized.val.name(), expected_name);
    }
}

#[test]
fn test_legacy_toml_backward_compatibility() {
    // Older configuration without modern fields (fps_limit, window_mode, filter, etc.)
    let legacy_toml = r#"
        language = "english"
        renderer = "vulkan"
        hardware_rasterizer = true
        resolution = 4
        texture_packs = true
        recompiled = true
        scale = 2
        layout = "stacked"
        integer_scale = false
        volume = 0.8
    "#;

    let settings: Settings =
        toml::from_str(legacy_toml).expect("legacy TOML failed to deserialize");

    // Existing fields parsed accurately
    assert_eq!(settings.resolution, 4);
    assert_eq!(settings.scale, 2);
    assert_eq!(settings.volume, 0.8);
    assert!(settings.hardware_rasterizer);

    // Newly added fields must take their correct defaults
    assert_eq!(settings.fps_limit, FpsLimit::Fps60);
    assert_eq!(settings.window_mode, WindowMode::Maximized);
    assert_eq!(settings.filter, Filter::Smooth);
    assert!(!settings.show_fps);
    assert_eq!(settings.pad.menu, Button::Mode);
}

#[test]
fn test_unknown_fields_forward_compatibility() {
    // Newer / unknown fields should not cause errors or discard known settings
    let future_toml = r#"
        resolution = 8
        fps_limit = "120"
        filter = "xbrz_linear"
        window_mode = "fullscreen"
        future_super_sampling = "dlss_fg"
        unknown_haptic_strength = 99
    "#;

    let settings: Settings =
        toml::from_str(future_toml).expect("future TOML with unknown keys should deserialize safely");

    assert_eq!(settings.resolution, 8);
    assert_eq!(settings.fps_limit, FpsLimit::Fps120);
    assert_eq!(settings.filter, Filter::XbrzLinear);
    assert_eq!(settings.window_mode, WindowMode::Fullscreen);
}

#[test]
fn test_per_game_settings_hierarchy_and_overrides() {
    let mut settings = Settings {
        resolution: 3,
        fps_limit: FpsLimit::Fps60,
        filter: Filter::Smooth,
        show_fps: false,
        window_mode: WindowMode::Maximized,
        ..Default::default()
    };

    let title_id: u64 = 0x00040000001B5000; // Pokémon Ultra Sun
    let key = game_key(title_id);
    assert_eq!(key, "00040000001B5000");

    let game_settings = GameSettings {
        resolution: Some(8),
        fps_limit: Some(FpsLimit::Fps120),
        filter: Some(Filter::Sharp),
        show_fps: Some(true),
        window_mode: Some(WindowMode::Fullscreen),
        ..Default::default()
    };
    // Notice frame_interpolation is left as None (should inherit global false)
    assert_eq!(game_settings.frame_interpolation, None);

    settings.per_game.insert(key, game_settings);

    // Active settings for Pokémon Ultra Sun
    let active = settings.for_game(Some(title_id));
    assert_eq!(active.resolution, 8);
    assert_eq!(active.fps_limit, FpsLimit::Fps120);
    assert_eq!(active.filter, Filter::Sharp);
    assert!(active.show_fps);
    assert_eq!(active.window_mode, WindowMode::Fullscreen);
    assert!(!active.frame_interpolation); // Inherited global false

    // Different title without custom settings
    let other_title_id: u64 = 0x0004000000055800;
    let other_active = settings.for_game(Some(other_title_id));
    assert_eq!(other_active.resolution, 3);
    assert_eq!(other_active.fps_limit, FpsLimit::Fps60);
    assert_eq!(other_active.filter, Filter::Smooth);
    assert!(!other_active.show_fps);
    assert_eq!(other_active.window_mode, WindowMode::Maximized);

    // None title ID (main menu/library view)
    let global_active = settings.for_game(None);
    assert_eq!(global_active.resolution, 3);
    assert_eq!(global_active.fps_limit, FpsLimit::Fps60);
}

#[test]
fn test_per_game_round_trip_serialization() {
    let mut settings = Settings {
        resolution: 4,
        ..Default::default()
    };

    let title_id: u64 = 0x00040000001B5000;
    let game_settings = GameSettings {
        resolution: Some(8),
        fps_limit: Some(FpsLimit::Fps120),
        filter: Some(Filter::XbrzLinear),
        ..Default::default()
    };
    settings.per_game.insert(game_key(title_id), game_settings);

    let serialized = toml::to_string(&settings).expect("failed to serialize full settings");
    assert!(serialized.contains("[per_game.00040000001B5000]"));
    assert!(serialized.contains("resolution = 8"));
    assert!(serialized.contains("fps_limit = \"120\""));
    assert!(serialized.contains("filter = \"xbrz_linear\""));

    let reloaded: Settings =
        toml::from_str(&serialized).expect("failed to deserialize reloaded settings");
    let active = reloaded.for_game(Some(title_id));
    assert_eq!(active.resolution, 8);
    assert_eq!(active.fps_limit, FpsLimit::Fps120);
    assert_eq!(active.filter, Filter::XbrzLinear);
}

#[test]
fn test_per_game_options_drop_system_only_fields_and_empty_entries() {
    let mut settings = Settings::default();
    let id: u64 = 0x00040000001B5000;
    settings.game_mut(id).show_fps = Some(true);
    settings.game_mut(id).window_mode = Some(WindowMode::Fullscreen);
    settings.game_mut(0x0004000000055D00).filter = Some(Filter::Xbrz);
    settings.migrate();
    // the window and frame counter are the system's alone now, and a game
    // left with nothing of its own goes from the file
    assert!(!settings.has_own(id));
    assert!(!settings.per_game.contains_key(&zakuro::settings::game_key(id)));
    // what a game keeps of its own stays
    assert!(settings.has_own(0x0004000000055D00));
    assert_eq!(settings.for_game(Some(0x0004000000055D00)).filter, Filter::Xbrz);
}

#[test]
fn test_smooth_textures_defaults_on_and_overrides_per_game() {
    assert!(Settings::default().smooth_textures);
    // settings saved before the option existed get it on
    let legacy: Settings = toml::from_str("resolution = 8\n").expect("legacy settings parse");
    assert!(legacy.smooth_textures);

    let mut settings = Settings { smooth_textures: true, ..Default::default() };
    let id = 0x0004_0000_001B_5000;
    settings.per_game.insert(game_key(id), GameSettings { smooth_textures: Some(false), ..Default::default() });
    assert!(!settings.for_game(Some(id)).smooth_textures);
    assert!(settings.for_game(None).smooth_textures);
}
