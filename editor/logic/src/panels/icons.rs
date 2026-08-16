//! Flat, front facing icons painted with shapes. The pictorial glyphs in egui's
//! bundled fonts are drawn in perspective, which looks skewed next to square panels.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Folder,
    File,
    Text,
    Image,
    Material,
    Mesh,
    Audio,
    Prefab,
    Shader,
}

impl Icon {
    pub fn color(self) -> Color32 {
        match self {
            Icon::Folder => Color32::from_rgb(226, 184, 96),
            Icon::File => Color32::from_rgb(150, 162, 178),
            Icon::Text => Color32::from_rgb(168, 180, 196),
            Icon::Image => Color32::from_rgb(102, 196, 178),
            Icon::Material => Color32::from_rgb(176, 140, 246),
            Icon::Mesh => Color32::from_rgb(238, 152, 96),
            Icon::Audio => Color32::from_rgb(120, 200, 140),
            Icon::Prefab => Color32::from_rgb(112, 168, 248),
            Icon::Shader => Color32::from_rgb(206, 132, 168),
        }
    }
}

/// Paints `icon` centred in `rect`, scaled to a square that fits it.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let size = rect.width().min(rect.height());
    let square = Rect::from_center_size(rect.center(), Vec2::splat(size));
    let unit = |x: f32, y: f32| -> Pos2 {
        pos2(square.left() + x * size, square.top() + y * size)
    };
    let line = Stroke::new((size * 0.06).max(1.0), color);
    let dim = color.gamma_multiply(0.55);

    match icon {
        Icon::Folder => {
            painter.add(Shape::convex_polygon(
                vec![
                    unit(0.08, 0.24),
                    unit(0.42, 0.24),
                    unit(0.50, 0.34),
                    unit(0.92, 0.34),
                    unit(0.92, 0.80),
                    unit(0.08, 0.80),
                ],
                dim,
                Stroke::NONE,
            ));
            painter.rect_filled(
                Rect::from_min_max(unit(0.08, 0.40), unit(0.92, 0.80)),
                (size * 0.05) as u8,
                color,
            );
        }
        Icon::File | Icon::Text | Icon::Shader => {
            painter.add(Shape::convex_polygon(
                vec![
                    unit(0.20, 0.14),
                    unit(0.62, 0.14),
                    unit(0.80, 0.32),
                    unit(0.80, 0.86),
                    unit(0.20, 0.86),
                ],
                dim,
                Stroke::new(1.0, color),
            ));
            painter.add(Shape::convex_polygon(
                vec![unit(0.62, 0.14), unit(0.80, 0.32), unit(0.62, 0.32)],
                color,
                Stroke::NONE,
            ));
            let rows: &[f32] = match icon {
                Icon::Shader => &[0.46, 0.58, 0.70],
                _ => &[0.44, 0.56, 0.68, 0.78],
            };
            for (index, y) in rows.iter().enumerate() {
                let right = if index == rows.len() - 1 { 0.56 } else { 0.68 };
                painter.line_segment([unit(0.30, *y), unit(right, *y)], line);
            }
        }
        Icon::Image => {
            let frame = Rect::from_min_max(unit(0.14, 0.20), unit(0.86, 0.80));
            painter.rect_filled(frame, (size * 0.05) as u8, dim);
            painter.rect_stroke(
                frame,
                (size * 0.05) as u8,
                Stroke::new(1.0, color),
                egui::StrokeKind::Inside,
            );
            painter.circle_filled(unit(0.34, 0.36), size * 0.06, color);
            painter.add(Shape::convex_polygon(
                vec![
                    unit(0.20, 0.74),
                    unit(0.44, 0.46),
                    unit(0.62, 0.74),
                ],
                color,
                Stroke::NONE,
            ));
            painter.add(Shape::convex_polygon(
                vec![
                    unit(0.50, 0.74),
                    unit(0.66, 0.54),
                    unit(0.82, 0.74),
                ],
                color.gamma_multiply(0.8),
                Stroke::NONE,
            ));
        }
        Icon::Material => {
            painter.circle_filled(square.center(), size * 0.30, dim);
            painter.circle_stroke(square.center(), size * 0.30, Stroke::new(1.5, color));
            painter.circle_filled(
                square.center() - vec2(size * 0.10, size * 0.10),
                size * 0.08,
                color,
            );
        }
        Icon::Mesh => {
            let points = [unit(0.50, 0.16), unit(0.86, 0.78), unit(0.14, 0.78)];
            painter.add(Shape::convex_polygon(
                points.to_vec(),
                dim,
                Stroke::new(1.5, color),
            ));
            painter.line_segment([points[0], unit(0.50, 0.78)], line);
            painter.line_segment([unit(0.32, 0.47), unit(0.68, 0.47)], line);
            for point in points {
                painter.circle_filled(point, size * 0.05, color);
            }
        }
        Icon::Audio => {
            painter.add(Shape::convex_polygon(
                vec![
                    unit(0.18, 0.38),
                    unit(0.34, 0.38),
                    unit(0.54, 0.18),
                    unit(0.54, 0.82),
                    unit(0.34, 0.62),
                    unit(0.18, 0.62),
                ],
                color,
                Stroke::NONE,
            ));
            for (index, radius) in [0.16_f32, 0.26].iter().enumerate() {
                painter.circle_stroke(
                    unit(0.54, 0.50),
                    size * radius,
                    Stroke::new(
                        line.width,
                        color.gamma_multiply(0.9 - index as f32 * 0.3),
                    ),
                );
            }
        }
        Icon::Prefab => {
            painter.add(Shape::convex_polygon(
                vec![
                    unit(0.50, 0.14),
                    unit(0.88, 0.34),
                    unit(0.88, 0.70),
                    unit(0.50, 0.88),
                    unit(0.12, 0.70),
                    unit(0.12, 0.34),
                ],
                dim,
                Stroke::new(1.5, color),
            ));
            painter.line_segment([unit(0.12, 0.34), unit(0.50, 0.52)], line);
            painter.line_segment([unit(0.88, 0.34), unit(0.50, 0.52)], line);
            painter.line_segment([unit(0.50, 0.52), unit(0.50, 0.88)], line);
        }
    }
}
