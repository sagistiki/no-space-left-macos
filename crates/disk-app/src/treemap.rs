//! Treemap layout and rendering.
//!
//! [`squarify`] is the pure layout core (Bruls/Huizing/van Wijk squarified
//! treemap): it turns a list of weights and a rectangle into tiles with good
//! aspect ratios. It has no egui-drawing dependencies and is unit-tested.

use crate::theme;
use disk_core::model::{EntryKind, Node};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};

/// One laid-out tile: which input it came from, and where it sits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tile {
    pub index: usize,
    pub rect: Rect,
}

/// Lay `weights` out inside `area` as a squarified treemap.
///
/// Tiles are returned for nonzero weights only; tile area is proportional to
/// weight, and together the tiles fill `area`.
pub fn squarify(weights: &[f64], area: Rect) -> Vec<Tile> {
    let mut tiles = Vec::new();
    if area.width() <= 0.0 || area.height() <= 0.0 {
        return tiles;
    }

    // Keep only positive weights, remembering their original indices.
    let indexed: Vec<(usize, f64)> = weights
        .iter()
        .enumerate()
        .filter(|&(_, &w)| w > 0.0)
        .map(|(i, &w)| (i, w))
        .collect();
    let total: f64 = indexed.iter().map(|&(_, w)| w).sum();
    if total <= 0.0 {
        return tiles;
    }

    // Scale weights so their summed pixel area equals the region's area.
    let scale = area.width() as f64 * area.height() as f64 / total;
    let indices: Vec<usize> = indexed.iter().map(|&(i, _)| i).collect();
    let areas: Vec<f64> = indexed.iter().map(|&(_, w)| w * scale).collect();

    let mut remaining = area;
    let n = areas.len();
    let mut i = 0;
    while i < n {
        let side = remaining.width().min(remaining.height()) as f64;

        // Greedily grow the current row while the worst aspect ratio improves.
        let mut best = f64::INFINITY;
        let mut row_sum = 0.0;
        let mut end = i;
        let mut j = i;
        while j < n {
            let candidate = row_sum + areas[j];
            let worst = worst_ratio(&areas[i..=j], candidate, side);
            if worst > best {
                break;
            }
            best = worst;
            row_sum = candidate;
            end = j;
            j += 1;
        }

        layout_row(
            &areas[i..=end],
            &indices[i..=end],
            row_sum,
            &mut remaining,
            &mut tiles,
        );
        i = end + 1;
    }

    tiles
}

/// Worst (largest) aspect ratio in a candidate row laid along `side`.
fn worst_ratio(row: &[f64], sum: f64, side: f64) -> f64 {
    let rmax = row.iter().cloned().fold(f64::MIN, f64::max);
    let rmin = row.iter().cloned().fold(f64::MAX, f64::min);
    let side2 = side * side;
    let sum2 = sum * sum;
    (side2 * rmax / sum2).max(sum2 / (side2 * rmin))
}

/// Place one row of tiles along the shorter edge of `remaining`, then shrink
/// `remaining` to the leftover region.
fn layout_row(
    row: &[f64],
    indices: &[usize],
    row_sum: f64,
    remaining: &mut Rect,
    tiles: &mut Vec<Tile>,
) {
    let horizontal = remaining.width() <= remaining.height();
    if horizontal {
        let strip_h = (row_sum / remaining.width() as f64) as f32;
        let mut x = remaining.left();
        let y = remaining.top();
        for (k, &a) in row.iter().enumerate() {
            let w = (a / row_sum * remaining.width() as f64) as f32;
            tiles.push(Tile {
                index: indices[k],
                rect: Rect::from_min_size(pos2(x, y), vec2(w, strip_h)),
            });
            x += w;
        }
        remaining.min.y = y + strip_h;
    } else {
        let strip_w = (row_sum / remaining.height() as f64) as f32;
        let x = remaining.left();
        let mut y = remaining.top();
        for (k, &a) in row.iter().enumerate() {
            let h = (a / row_sum * remaining.height() as f64) as f32;
            tiles.push(Tile {
                index: indices[k],
                rect: Rect::from_min_size(pos2(x, y), vec2(strip_w, h)),
            });
            y += h;
        }
        remaining.min.x = x + strip_w;
    }
}

/// Cap on tiles drawn at once. Children are size-sorted, so this keeps the
/// largest items; the long tail of tiny entries is folded away.
const MAX_TILES: usize = 250;

/// What the user did on the treemap this frame.
#[derive(Default)]
pub struct TreemapAction {
    /// A child was single-clicked (index into `node.children`).
    pub selected: Option<usize>,
    /// A directory child was double-clicked — drill into it.
    pub drill: Option<usize>,
}

/// Draw `node`'s children as a treemap filling the available space, and report
/// the interaction. `selected` highlights one child.
pub fn show(ui: &mut egui::Ui, node: &Node, selected: Option<usize>) -> TreemapAction {
    let mut action = TreemapAction::default();
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());

    let count = node.children.len().min(MAX_TILES);
    if count == 0 {
        ui.painter().text(
            area.center(),
            Align2::CENTER_CENTER,
            "Empty — nothing to show here",
            FontId::proportional(14.0),
            ui.visuals().weak_text_color(),
        );
        return action;
    }

    let painter = ui.painter_at(area);
    let weights: Vec<f64> = node.children[..count].iter().map(|c| c.size as f64).collect();
    let tiles = squarify(&weights, area);

    for tile in &tiles {
        let child = &node.children[tile.index];
        let resp = ui.interact(tile.rect, ui.id().with(("tile", tile.index)), Sense::click());

        let base = tile_color(tile.index);
        let fill = if resp.hovered() {
            base.linear_multiply(1.22)
        } else {
            base
        };
        let r = tile.rect.shrink(1.5);
        painter.rect_filled(r, 5.0, fill);
        if Some(tile.index) == selected {
            painter.rect_stroke(r, 5.0, Stroke::new(2.5, theme::ACCENT));
        }

        // Labels only when the tile is roomy enough to read.
        if r.width() > 56.0 && r.height() > 26.0 {
            let tc = contrast_color(fill);
            painter.text(
                r.left_top() + vec2(8.0, 6.0),
                Align2::LEFT_TOP,
                &child.name,
                FontId::proportional(12.5),
                tc,
            );
            if r.height() > 46.0 {
                painter.text(
                    r.left_top() + vec2(8.0, 24.0),
                    Align2::LEFT_TOP,
                    crate::format::human_size(child.size),
                    FontId::proportional(11.0),
                    tc.gamma_multiply(0.85),
                );
            }
        }

        if resp.double_clicked() && child.kind == EntryKind::Dir {
            action.drill = Some(tile.index);
        } else if resp.clicked() {
            action.selected = Some(tile.index);
        }
    }

    action
}

/// A soft, Apple-ish palette cycled by tile position.
fn tile_color(index: usize) -> Color32 {
    const PALETTE: [Color32; 6] = [
        Color32::from_rgb(10, 132, 255),
        Color32::from_rgb(48, 209, 88),
        Color32::from_rgb(255, 159, 10),
        Color32::from_rgb(191, 90, 242),
        Color32::from_rgb(255, 105, 97),
        Color32::from_rgb(94, 200, 230),
    ];
    PALETTE[index % PALETTE.len()]
}

/// Pick black or white text for legibility over `bg`.
fn contrast_color(bg: Color32) -> Color32 {
    let [r, g, b, _] = bg.to_array();
    let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
    if luma > 140.0 {
        Color32::from_rgb(20, 20, 20)
    } else {
        Color32::WHITE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h))
    }

    #[test]
    fn one_tile_per_nonzero_weight() {
        let tiles = squarify(&[50.0, 30.0, 20.0], area(100.0, 100.0));
        assert_eq!(tiles.len(), 3);
    }

    #[test]
    fn tile_areas_are_proportional_to_weights() {
        // total 100 over a 10_000 px area → 6000 / 3000 / 1000 px.
        let tiles = squarify(&[60.0, 30.0, 10.0], area(100.0, 100.0));
        let area_of = |i: usize| {
            let t = tiles.iter().find(|t| t.index == i).unwrap();
            (t.rect.width() * t.rect.height()) as f64
        };
        assert!((area_of(0) - 6000.0).abs() < 1.0, "got {}", area_of(0));
        assert!((area_of(1) - 3000.0).abs() < 1.0, "got {}", area_of(1));
        assert!((area_of(2) - 1000.0).abs() < 1.0, "got {}", area_of(2));
    }

    #[test]
    fn tiles_stay_within_the_area() {
        let a = Rect::from_min_size(pos2(10.0, 20.0), vec2(200.0, 150.0));
        let tiles = squarify(&[5.0, 3.0, 2.0, 1.0, 8.0], a);
        assert!(!tiles.is_empty());
        for t in &tiles {
            assert!(t.rect.left() >= a.left() - 0.01);
            assert!(t.rect.top() >= a.top() - 0.01);
            assert!(t.rect.right() <= a.right() + 0.01);
            assert!(t.rect.bottom() <= a.bottom() + 0.01);
        }
    }

    #[test]
    fn tiles_fill_the_region() {
        let tiles = squarify(&[1.0, 2.0, 3.0, 4.0, 5.0], area(120.0, 80.0)); // 9600 px
        let total: f64 = tiles
            .iter()
            .map(|t| (t.rect.width() * t.rect.height()) as f64)
            .sum();
        assert!((total - 9600.0).abs() < 5.0, "tiles should fill area; got {total}");
    }

    #[test]
    fn handles_empty_zero_and_degenerate() {
        assert!(squarify(&[], area(100.0, 100.0)).is_empty());
        assert!(squarify(&[0.0, 0.0], area(100.0, 100.0)).is_empty());
        assert!(squarify(&[1.0, 2.0], area(0.0, 50.0)).is_empty());
    }
}
