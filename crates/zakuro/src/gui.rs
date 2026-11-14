//! egui, fed the window's events and turned into an overlay the presenters
//! draw over the screens.

use egui::epaint::Primitive;
use egui::{TextureId, ViewportId};
use winit::event::WindowEvent;
use winit::window::Window;

use zakuro_gpu::{Overlay, OverlayMesh, OverlayTexture, OverlayVertex};

pub struct Gui {
    pub ctx: egui::Context,
    state: egui_winit::State,
    extra_events: Vec<egui::Event>,
}

impl Gui {
    pub fn new(window: &Window) -> Gui {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts(system_fonts()));
        let mut visuals = egui::Visuals::dark();
        // anything that can be clicked shows a hand under the mouse
        visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
        visuals.panel_fill = egui::Color32::from_rgb(18, 19, 23);
        visuals.window_fill = egui::Color32::from_rgb(28, 30, 36);
        visuals.extreme_bg_color = egui::Color32::from_rgb(12, 14, 18);
        visuals.faint_bg_color = egui::Color32::from_rgb(22, 25, 33);
        visuals.window_corner_radius = egui::CornerRadius::same(12);
        visuals.menu_corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(32, 36, 48);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(50, 56, 72));
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(230, 235, 245));
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(46, 54, 72);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(62, 198, 255));
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(30, 90, 160);
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(80, 180, 255));
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
        visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(22, 25, 33);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(42, 48, 62));
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(175, 185, 200));
        visuals.selection.bg_fill = egui::Color32::from_rgba_premultiplied(40, 120, 220, 160);
        visuals.selection.stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(100, 190, 255));
        ctx.set_visuals(visuals);

        ctx.style_mut_of(egui::Theme::Dark, |style| {
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 7.0);
            style.spacing.window_margin = egui::Margin::same(14);
            // labels are read, not selected, so no text cursor over them
            style.interaction.selectable_labels = false;
            // windows fade in over this long
            style.animation_time = 0.16;
        });
        let state = egui_winit::State::new(
            ctx.clone(),
            ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,
            Some(8192),
        );
        Gui { ctx, state, extra_events: Vec::new() }
    }

    /// hands egui an event, true when it wants it for itself.
    pub fn event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.state.on_window_event(window, event).consumed
    }

    /// moves egui focus to the next interactive widget.
    pub fn move_focus_next(&mut self) {
        self.ctx.memory_mut(|m| {
            m.move_focus(egui::FocusDirection::Next);
        });
        self.ctx.request_repaint();
    }

    /// moves egui focus to the previous interactive widget.
    #[allow(dead_code)]
    pub fn move_focus_prev(&mut self) {
        self.ctx.memory_mut(|m| {
            m.move_focus(egui::FocusDirection::Previous);
        });
        self.ctx.request_repaint();
    }

    /// clears any current widget focus.
    pub fn clear_focus(&mut self) {
        self.ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
        self.ctx.request_repaint();
    }

    #[allow(dead_code)]
    pub fn send_key(&mut self, key: egui::Key) {
        self.extra_events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        self.extra_events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
    }

    /// whether egui is using the keyboard or the pointer, in which case the
    /// game should not see them.
    pub fn wants_keyboard(&self) -> bool {
        self.ctx.egui_wants_keyboard_input()
    }

    pub fn wants_pointer(&self) -> bool {
        self.ctx.egui_wants_pointer_input()
    }

    /// runs the interface for a frame and returns what to draw.
    pub fn frame(&mut self, window: &Window, ui: impl FnMut(&mut egui::Ui)) -> Overlay {
        let mut input = self.state.take_egui_input(window);
        input.events.append(&mut self.extra_events);
        let mut output = self.ctx.run_ui(input, ui);
        self.state.handle_platform_output(window, output.platform_output);
        let pixels_per_point = output.pixels_per_point;
        let size = window.inner_size();

        let textures = std::mem::take(&mut output.textures_delta.set)
            .into_iter()
            .flat_map(|(id, deltas)| {
                deltas.into_iter().map(move |delta| {
                    let egui::ImageData::Color(image) = &delta.image;
                    OverlayTexture {
                        id: texture(id),
                        offset: delta.pos.map(|[x, y]| [x as u32, y as u32]),
                        size: [image.width() as u32, image.height() as u32],
                        pixels: image.pixels.iter().flat_map(|color| color.to_array()).collect(),
                        linear: delta.options.magnification == egui::TextureFilter::Linear,
                    }
                })
            })
            .collect();
        let meshes = self
            .ctx
            .tessellate(output.shapes, pixels_per_point)
            .into_iter()
            .filter_map(|clipped| {
                let Primitive::Mesh(mesh) = clipped.primitive else { return None };
                let rect = clipped.clip_rect;
                let pixel = |value: f32, most: u32| ((value * pixels_per_point).round().max(0.0) as u32).min(most);
                Some(OverlayMesh {
                    texture: texture(mesh.texture_id),
                    clip: [
                        pixel(rect.min.x, size.width),
                        pixel(rect.min.y, size.height),
                        pixel(rect.max.x, size.width),
                        pixel(rect.max.y, size.height),
                    ],
                    vertices: mesh
                        .vertices
                        .iter()
                        .map(|vertex| OverlayVertex {
                            position: [vertex.pos.x * pixels_per_point, vertex.pos.y * pixels_per_point],
                            uv: [vertex.uv.x, vertex.uv.y],
                            color: vertex.color.to_array(),
                        })
                        .collect(),
                    indices: mesh.indices,
                })
            })
            .collect();
        let free = std::mem::take(&mut output.textures_delta.free).into_iter().map(texture).collect();
        Overlay { textures, meshes, free }
    }
}

/// egui's two kinds of texture in one space of ids.
fn texture(id: TextureId) -> u64 {
    match id {
        TextureId::Managed(n) => n * 2,
        TextureId::User(n) => n * 2 + 1,
    }
}

/// the family titles are drawn in: Inter's semibold, falling back on the
/// same fonts as the body text.
pub const STRONG: &str = "strong";

/// Inter, which the interface is drawn in, in the two weights it uses.
/// it is under the SIL Open Font License, in fonts/Inter-LICENSE.txt.
const INTER: &[u8] = include_bytes!("../fonts/Inter-Regular.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../fonts/Inter-SemiBold.ttf");

/// Inter first, then egui's own fonts for the symbols and emoji Inter
/// lacks, then extra ones to fall back on for what they all lack. game
/// names come in Japanese, Korean and Chinese, whose characters egui's
/// fonts have none of and drew as boxes.
fn fonts(extra: Vec<Vec<u8>>) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    for (name, data) in [("Inter", INTER), ("Inter SemiBold", INTER_SEMIBOLD)] {
        fonts.font_data.insert(name.into(), std::sync::Arc::new(egui::FontData::from_static(data)));
    }
    let proportional = fonts.families.entry(egui::FontFamily::Proportional).or_default();
    proportional.insert(0, "Inter".into());
    let mut strong = proportional.clone();
    strong[0] = "Inter SemiBold".into();
    fonts.families.insert(egui::FontFamily::Name(STRONG.into()), strong);
    for (i, font) in extra.into_iter().enumerate() {
        let name = format!("system {i}");
        fonts.font_data.insert(name.clone(), std::sync::Arc::new(egui::FontData::from_owned(font)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace, egui::FontFamily::Name(STRONG.into())] {
            fonts.families.entry(family).or_default().push(name.clone());
        }
    }
    fonts
}

/// a context with the interface's fonts, for drawing it in tests.
#[cfg(test)]
pub fn test_context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts(Vec::new()));
    ctx
}

/// the system's fonts for Japanese, Korean and Chinese, the first one
/// there of each kind that has the kind's characters.
fn system_fonts() -> Vec<Vec<u8>> {
    font_candidates()
        .into_iter()
        .filter_map(|(character, paths)| first_drawing(character, paths.iter().filter_map(|path| std::fs::read(path).ok())))
        .collect()
}

/// the first of fonts with a glyph for character. fontconfig names a font
/// even when the system has none for the language asked for.
fn first_drawing(character: char, fonts: impl IntoIterator<Item = Vec<u8>>) -> Option<Vec<u8>> {
    use ab_glyph::Font;
    fonts.into_iter().find(|font| ab_glyph::FontRef::try_from_slice_and_index(font, 0).is_ok_and(|font| font.glyph_id(character).0 != 0))
}

/// where Windows keeps fonts for Japanese, Korean and Chinese, each kind
/// a character of its own and the fonts to try in turn.
#[cfg(windows)]
fn font_candidates() -> Vec<(char, Vec<std::path::PathBuf>)> {
    let folder = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
    let kinds: [(char, &[&str]); 3] = [('あ', &["YuGothM.ttc", "meiryo.ttc", "msgothic.ttc"]), ('한', &["malgun.ttf"]), ('汉', &["msyh.ttc", "simsun.ttc"])];
    kinds.iter().map(|(character, names)| (*character, names.iter().map(|name| folder.join(name)).collect())).collect()
}

/// where macOS keeps fonts for Japanese, Korean and Chinese.
#[cfg(target_os = "macos")]
fn font_candidates() -> Vec<(char, Vec<std::path::PathBuf>)> {
    let kinds: [(char, &[&str]); 3] = [
        ('あ', &["/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc", "/System/Library/Fonts/Hiragino Sans GB.ttc"]),
        ('한', &["/System/Library/Fonts/AppleSDGothicNeo.ttc"]),
        ('汉', &["/System/Library/Fonts/PingFang.ttc"]),
    ];
    kinds.iter().map(|(character, paths)| (*character, paths.iter().map(std::path::PathBuf::from).collect())).collect()
}

/// where Linux distributions put Noto's collection, which has Japanese,
/// Korean and Chinese all in one, or else the font fontconfig picks for
/// Japanese.
#[cfg(all(unix, not(target_os = "macos")))]
fn font_candidates() -> Vec<(char, Vec<std::path::PathBuf>)> {
    let mut paths: Vec<std::path::PathBuf> = [
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    ]
    .iter()
    .map(std::path::PathBuf::from)
    .collect();
    if !paths.iter().any(|path| path.exists()) {
        let matched = std::process::Command::new("fc-match").args(["--format=%{file}", ":lang=ja"]).output();
        if let Some(output) = matched.ok().filter(|output| output.status.success()) {
            paths.push(String::from_utf8_lossy(&output.stdout).into_owned().into());
        }
    }
    vec![('あ', paths)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::text::{Fonts, TextOptions};

    /// Japanese names have every character to draw with the system's
    /// fonts, where the system has them, and none with egui's alone. only
    /// their Japanese characters are asked about, epaint says no for any
    /// character the font with its replacement glyph has, Hack's letters
    /// for monospace among them.
    #[test]
    fn japanese_names_are_drawn_with_the_system_fonts() {
        let name = "イナズマイレブン・クロノストーン";
        let mut plain = Fonts::new(TextOptions::default(), fonts(Vec::new()));
        assert!(!plain.has_glyphs(&egui::FontId::proportional(14.0), name), "egui's own fonts have no Japanese");
        let found = system_fonts();
        if first_drawing('あ', found.clone()).is_none() {
            eprintln!("no font for Japanese on this system");
            return;
        }
        let mut with = Fonts::new(TextOptions::default(), fonts(found));
        for font in [egui::FontId::proportional(14.0), egui::FontId::monospace(14.0)] {
            assert!(with.has_glyphs(&font, name), "{font:?}");
        }
    }

    /// the interface is drawn in Inter, titles in its semibold, both
    /// falling back on the same fonts for what Inter lacks.
    #[test]
    fn the_interface_is_drawn_in_inter() {
        let fonts = fonts(vec![b"extra".to_vec()]);
        let body = &fonts.families[&egui::FontFamily::Proportional];
        let titles = &fonts.families[&egui::FontFamily::Name(STRONG.into())];
        assert_eq!(body[0], "Inter");
        assert_eq!(titles[0], "Inter SemiBold");
        assert_eq!(body[1..], titles[1..]);
        assert_eq!(body.last().map(String::as_str), Some("system 0"));
    }

    /// a font without the characters asked for is passed over, like the
    /// one fontconfig names on a system with no font for Japanese.
    #[test]
    fn fonts_without_japanese_are_passed_over() {
        let latin = egui::FontDefinitions::default().font_data["Ubuntu-Light"].font.to_vec();
        assert_eq!(first_drawing('あ', [latin.clone()]), None);
        assert_eq!(first_drawing('A', [latin.clone()]), Some(latin));
    }
}
