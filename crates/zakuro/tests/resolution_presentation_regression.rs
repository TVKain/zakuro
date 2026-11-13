//! Integration regression tests for resolution scaling, presentation pipeline, and screen layout geometry.

use zakuro::menus::{resolution_badge_label, resolution_label};
use zakuro::settings::Filter;
use zakuro_gpu::{place, ScreenFilter, ScreenLayout, ScreenOrientation, Shown, Stereo};

#[test]
fn test_resolution_label_mappings() {
    let expected = [
        (1, "1x (400x240 - Native 3DS)"),
        (2, "2x (800x480 - 480p)"),
        (3, "3x (1200x720 - 720p HD)"),
        (4, "4x (1600x960)"),
        (5, "5x (2000x1200 - 1080p FHD)"),
        (6, "6x (2400x1440 - 1440p 2K)"),
        (7, "7x (2800x1680)"),
        (8, "8x (3200x1920 - 4K UHD)"),
    ];

    for (scale, label) in expected {
        assert_eq!(resolution_label(scale), label);
    }

    // Out of range fallbacks
    assert_eq!(resolution_label(0), "Custom");
    assert_eq!(resolution_label(9), "Custom");
    assert_eq!(resolution_label(16), "Custom");
}

#[test]
fn test_resolution_badge_label_mappings() {
    let expected = [
        (1, "1x (400x240)"),
        (2, "2x (800x480)"),
        (3, "3x (1200x720 720p)"),
        (4, "4x (1600x960)"),
        (5, "5x (2000x1200 1080p)"),
        (6, "6x (2400x1440 2K)"),
        (7, "7x (2800x1680)"),
        (8, "8x (3200x1920 4K)"),
    ];

    for (scale, label) in expected {
        assert_eq!(resolution_badge_label(scale), label);
    }

    assert_eq!(resolution_badge_label(0), "Custom");
    assert_eq!(resolution_badge_label(10), "Custom");
}

#[test]
fn test_screen_filter_mode_ids() {
    // Mode IDs match the shader constants in present.frag / present_stereo.frag and OpenGL backend
    assert_eq!(ScreenFilter::Smooth.mode(), 0);
    assert_eq!(ScreenFilter::Pixels.mode(), 1);
    assert_eq!(ScreenFilter::Sharp.mode(), 2);
    assert_eq!(ScreenFilter::Xbrz.mode(), 3);
    assert_eq!(ScreenFilter::XbrzLinear.mode(), 4);

    // Verify Settings::Filter mapping to ScreenFilter
    assert_eq!(Filter::Smooth.screens().mode(), 0);
    assert_eq!(Filter::Pixels.screens().mode(), 1);
    assert_eq!(Filter::Sharp.screens().mode(), 2);
    assert_eq!(Filter::Xbrz.screens().mode(), 3);
    assert_eq!(Filter::XbrzLinear.screens().mode(), 4);
}

#[test]
fn test_screen_placement_geometry_largetop() {
    // MSI Claw 8 AI+ resolution: 1920 x 1200 (16:10 aspect ratio)
    let win_w = 1920;
    let win_h = 1200;

    let viewports = place(
        win_w,
        win_h,
        ScreenLayout::LargeTop,
        ScreenOrientation::North,
        400.0,
        false, // non-integer scaling
        Stereo::default(),
        false,
    );

    assert_eq!(viewports.len(), 2, "LargeTop layout must produce 2 viewports (Top and Bottom)");

    let (top_vp, top_screen) = &viewports[0];
    let (bottom_vp, bottom_screen) = &viewports[1];

    assert_eq!(*top_screen, Shown::Top);
    assert_eq!(*bottom_screen, Shown::Bottom);

    // Verify top screen conforms to 3DS 5:3 aspect ratio (400x240)
    let top_aspect = top_vp.width / top_vp.height;
    let expected_top_aspect = 400.0 / 240.0;
    assert!(
        (top_aspect - expected_top_aspect).abs() < 0.01,
        "Top screen aspect ratio {top_aspect} deviates from expected 5:3 {expected_top_aspect}"
    );

    // Verify bottom screen conforms to 3DS 4:3 aspect ratio (320x240)
    let bottom_aspect = bottom_vp.width / bottom_vp.height;
    let expected_bottom_aspect = 320.0 / 240.0;
    assert!(
        (bottom_aspect - expected_bottom_aspect).abs() < 0.01,
        "Bottom screen aspect ratio {bottom_aspect} deviates from expected 4:3 {expected_bottom_aspect}"
    );

    // Verify bounds fit within window dimensions
    assert!(top_vp.x >= 0.0 && top_vp.x + top_vp.width <= win_w as f32);
    assert!(top_vp.y >= 0.0 && top_vp.y + top_vp.height <= win_h as f32);
    assert!(bottom_vp.x >= 0.0 && bottom_vp.x + bottom_vp.width <= win_w as f32);
    assert!(bottom_vp.y >= 0.0 && bottom_vp.y + bottom_vp.height <= win_h as f32);

    // Integer scaling test
    let int_viewports = place(
        win_w,
        win_h,
        ScreenLayout::LargeTop,
        ScreenOrientation::North,
        400.0,
        true, // integer scaling ON
        Stereo::default(),
        false,
    );

    let (int_top_vp, _) = &int_viewports[0];
    let scale_factor = (int_top_vp.width / 400.0).round();
    assert!(
        ((int_top_vp.width / 400.0) - scale_factor).abs() < 0.001,
        "Integer scaling must produce integer multiples of native 400px width"
    );
    assert!(
        ((int_top_vp.height / 240.0) - scale_factor).abs() < 0.001,
        "Integer scaling must produce integer multiples of native 240px height"
    );
}

#[test]
fn test_screen_placement_geometry_sidebyside_and_solo() {
    let win_w = 1920;
    let win_h = 1080;

    // SideBySide
    let side_viewports = place(
        win_w,
        win_h,
        ScreenLayout::SideBySide,
        ScreenOrientation::North,
        400.0,
        false,
        Stereo::default(),
        false,
    );
    assert_eq!(side_viewports.len(), 2);
    let (left, _) = &side_viewports[0];
    let (right, _) = &side_viewports[1];
    // Left screen should be situated to the left of the right screen without horizontal overlap
    assert!(left.x + left.width <= right.x + 0.001);

    // TopOnly
    let top_only = place(
        win_w,
        win_h,
        ScreenLayout::TopOnly,
        ScreenOrientation::North,
        400.0,
        false,
        Stereo::default(),
        false,
    );
    assert_eq!(top_only.len(), 1);
    assert_eq!(top_only[0].1, Shown::Top);

    // BottomOnly
    let bottom_only = place(
        win_w,
        win_h,
        ScreenLayout::BottomOnly,
        ScreenOrientation::North,
        400.0,
        false,
        Stereo::default(),
        false,
    );
    assert_eq!(bottom_only.len(), 1);
    assert_eq!(bottom_only[0].1, Shown::Bottom);
}
