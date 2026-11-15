//! the interface's icons: outlines in a 24 by 24 box, drawn to the screen's
//! own pixels the first time they show at a size, then kept as textures.

use std::collections::HashMap;

use egui::{Color32, Rect, TextureHandle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Plus,
    Close,
    Check,
    ChevronLeft,
    ChevronRight,
    Users,
    Sliders,
    Power,
    Monitor,
    Screens,
    Cube,
    Gamepad,
    Speaker,
    Folder,
    Chip,
    AddOns,
}

impl Icon {
    /// its outline as SVG path data, stroked 2 wide with round ends, then
    /// circles, (x, y, radius), stroked the same.
    fn shape(self) -> (&'static str, &'static [(f32, f32, f32)]) {
        match self {
            Icon::Plus => ("M12 5v14M5 12h14", &[]),
            Icon::Close => ("M6 6l12 12M18 6L6 18", &[]),
            Icon::Check => ("M5 12.5l4.5 4.5L19 7", &[]),
            Icon::ChevronLeft => ("M15 5.5L8.5 12l6.5 6.5", &[]),
            Icon::ChevronRight => ("M9 5.5l6.5 6.5L9 18.5", &[]),
            Icon::Users => ("M2.5 20v-1a5 5 0 0 1 5-5h3a5 5 0 0 1 5 5v1M15.5 4.3a3.6 3.6 0 0 1 0 7M18 14.3a5 5 0 0 1 3.5 4.7v1", &[(9.0, 7.8, 3.6)]),
            Icon::Sliders => ("M4 7h9M19 7h1M4 17h1M11 17h9", &[(16.0, 7.0, 2.6), (8.0, 17.0, 2.6)]),
            Icon::Power => ("M12 3v8M6.7 6.7A7.5 7.5 0 1 0 17.3 6.7", &[]),
            Icon::Monitor => ("M5 4h14a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2zM8.5 21h7M12 17v4", &[]),
            Icon::Screens => (
                "M5.5 2.5h13a1.5 1.5 0 0 1 1.5 1.5v6a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 10V4a1.5 1.5 0 0 1 1.5-1.5zM7.5 13.5h9A1.5 1.5 0 0 1 18 15v5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 6 20v-5a1.5 1.5 0 0 1 1.5-1.5z",
                &[],
            ),
            Icon::Cube => ("M12 2.8l8 4.4v9.6l-8 4.4-8-4.4V7.2zM4.3 7.4L12 11.8l7.7-4.4M12 11.8v9.2", &[]),
            Icon::Gamepad => (
                "M7 7.5h10a5 5 0 0 1 5 5v1.3a3.7 3.7 0 0 1-6.6 2.3L14 14.5h-4l-1.4 1.6A3.7 3.7 0 0 1 2 13.8v-1.3a5 5 0 0 1 5-5zM7.5 10v4M5.5 12h4M15.5 11h.01M18 13.5h.01",
                &[],
            ),
            Icon::Speaker => ("M4 9.5h3l5-4.5v14l-5-4.5H4zM16 9a4.2 4.2 0 0 1 0 6M18.8 6.2a8.2 8.2 0 0 1 0 11.6", &[]),
            Icon::Folder => ("M3 7a2 2 0 0 1 2-2h4l2 2.5h8a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z", &[]),
            Icon::Chip => (
                "M8 6h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2zM10 3v3M14 3v3M10 18v3M14 18v3M3 10h3M3 14h3M18 10h3M18 14h3",
                &[],
            ),
            Icon::AddOns => (
                "M5 4h4a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1zM15 4h4a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1h-4a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1zM5 14h4a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1v-4a1 1 0 0 1 1-1zM17 14v6M14 17h6",
                &[],
            ),
        }
    }
}

/// draws the icon filling rect, in color.
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let ctx = painter.ctx();
    let scale = ctx.pixels_per_point();
    // whole pixels, at a whole pixel, so the outline stays crisp
    let pixels = (rect.width().max(rect.height()) * scale).round().max(4.0) as u32;
    let size = pixels as f32 / scale;
    let min = ((rect.center().to_vec2() - egui::Vec2::splat(size / 2.0)) * scale).round() / scale;
    let at = Rect::from_min_size(min.to_pos2(), egui::Vec2::splat(size));
    let Some(texture) = texture(ctx, icon, pixels) else { return };
    painter.image(texture.id(), at, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), color);
}

/// the icon at pixels square, drawn the first time it is asked for.
fn texture(ctx: &egui::Context, icon: Icon, pixels: u32) -> Option<TextureHandle> {
    let id = egui::Id::new("zakuro icons");
    if let Some(texture) = ctx.data(|data| data.get_temp::<HashMap<(Icon, u32), TextureHandle>>(id).and_then(|map| map.get(&(icon, pixels)).cloned())) {
        return Some(texture);
    }
    let image = render(icon, pixels)?;
    let texture = ctx.load_texture(format!("icon {icon:?} {pixels}"), image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<HashMap<(Icon, u32), TextureHandle>>(id).insert((icon, pixels), texture.clone());
    });
    Some(texture)
}

/// the icon's outline in white, its edges anti-aliased, on nothing.
fn render(icon: Icon, pixels: u32) -> Option<egui::ColorImage> {
    let (data, circles) = icon.shape();
    let path = kurbo::BezPath::from_svg(data).ok()?;
    let mut builder = tiny_skia::PathBuilder::new();
    for element in path.elements() {
        match *element {
            kurbo::PathEl::MoveTo(p) => builder.move_to(p.x as f32, p.y as f32),
            kurbo::PathEl::LineTo(p) => builder.line_to(p.x as f32, p.y as f32),
            kurbo::PathEl::QuadTo(a, p) => builder.quad_to(a.x as f32, a.y as f32, p.x as f32, p.y as f32),
            kurbo::PathEl::CurveTo(a, b, p) => builder.cubic_to(a.x as f32, a.y as f32, b.x as f32, b.y as f32, p.x as f32, p.y as f32),
            kurbo::PathEl::ClosePath => builder.close(),
        }
    }
    for &(x, y, radius) in circles {
        builder.push_circle(x, y, radius);
    }
    let path = builder.finish()?;
    let mut pixmap = tiny_skia::Pixmap::new(pixels, pixels)?;
    let mut paint = tiny_skia::Paint::default();
    paint.set_color(tiny_skia::Color::WHITE);
    paint.anti_alias = true;
    // a little heavier when small, so thin lines don't fade
    let width = if pixels < 24 { 2.2 } else { 2.0 };
    let stroke = tiny_skia::Stroke { width, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Default::default() };
    let scale = pixels as f32 / 24.0;
    pixmap.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::from_scale(scale, scale), None);
    // premultiplied white is its coverage in every channel
    let pixels_rgba: Vec<Color32> = pixmap.pixels().iter().map(|pixel| Color32::from_white_alpha(pixel.alpha())).collect();
    Some(egui::ColorImage::new([pixels as usize, pixels as usize], pixels_rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_draws() {
        let all = [
            Icon::Plus,
            Icon::Close,
            Icon::Check,
            Icon::ChevronLeft,
            Icon::ChevronRight,
            Icon::Users,
            Icon::Sliders,
            Icon::Power,
            Icon::Monitor,
            Icon::Screens,
            Icon::Cube,
            Icon::Gamepad,
            Icon::Speaker,
            Icon::Folder,
            Icon::Chip,
            Icon::AddOns,
        ];
        for icon in all {
            let image = render(icon, 48).unwrap_or_else(|| panic!("{icon:?} should draw"));
            let covered = image.pixels.iter().filter(|pixel| pixel.a() > 0).count();
            assert!(covered > 48 * 2, "{icon:?} draws next to nothing");
            assert!(covered < 48 * 48 / 2, "{icon:?} fills most of its box");
        }
    }
}
