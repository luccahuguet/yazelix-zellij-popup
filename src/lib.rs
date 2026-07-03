pub mod popup_contract;

use popup_contract::TransientPaneGeometry;
use zellij_tile::prelude::FloatingPaneCoordinates;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PopupViewport {
    pub columns: usize,
    pub rows: usize,
}

pub fn floating_coordinates(
    geometry: TransientPaneGeometry,
    viewport: Option<PopupViewport>,
) -> Option<FloatingPaneCoordinates> {
    if let Some(viewport) =
        viewport.filter(|_| geometry.side_margin > 0 || geometry.vertical_margin > 0)
    {
        let side_margin = bounded_margin(geometry.side_margin, viewport.columns);
        let vertical_margin = bounded_margin(geometry.vertical_margin, viewport.rows);
        return FloatingPaneCoordinates::new(
            Some(side_margin.to_string()),
            Some((vertical_margin + 1).to_string()),
            Some(viewport.columns.saturating_sub(side_margin * 2).to_string()),
            Some(
                viewport
                    .rows
                    .saturating_sub(vertical_margin * 2)
                    .to_string(),
            ),
            None,
            None,
        );
    }

    FloatingPaneCoordinates::new(
        None,
        None,
        Some(format!("{}%", geometry.width_percent)),
        Some(format!("{}%", geometry.height_percent)),
        None,
        None,
    )
}

fn bounded_margin(margin: usize, size: usize) -> usize {
    margin.min(size.saturating_sub(1) / 2)
}

// Test lane: default
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_popup_geometry_lets_zellij_center_after_cell_rounding() {
        let coordinates = floating_coordinates(
            TransientPaneGeometry {
                width_percent: 98,
                height_percent: 97,
                side_margin: 0,
                vertical_margin: 0,
            },
            None,
        )
        .unwrap();

        assert_eq!(coordinates.x, None);
        assert_eq!(coordinates.y, None);
        assert_eq!(
            coordinates,
            FloatingPaneCoordinates::new(
                None,
                None,
                Some("98%".to_string()),
                Some("97%".to_string()),
                None,
                None
            )
            .unwrap()
        );
    }

    #[test]
    fn margin_popup_geometry_uses_fixed_cell_coordinates() {
        let coordinates = floating_coordinates(
            TransientPaneGeometry {
                width_percent: 100,
                height_percent: 100,
                side_margin: 2,
                vertical_margin: 1,
            },
            Some(PopupViewport {
                columns: 80,
                rows: 24,
            }),
        )
        .unwrap();

        assert_eq!(
            coordinates,
            FloatingPaneCoordinates::new(
                Some("2".to_string()),
                Some("2".to_string()),
                Some("76".to_string()),
                Some("22".to_string()),
                None,
                None
            )
            .unwrap()
        );
    }

    #[test]
    fn oversized_margins_clamp_to_visible_pane() {
        let coordinates = floating_coordinates(
            TransientPaneGeometry {
                width_percent: 100,
                height_percent: 100,
                side_margin: 99,
                vertical_margin: 99,
            },
            Some(PopupViewport {
                columns: 4,
                rows: 3,
            }),
        )
        .unwrap();

        assert_eq!(
            coordinates,
            FloatingPaneCoordinates::new(
                Some("1".to_string()),
                Some("2".to_string()),
                Some("2".to_string()),
                Some("1".to_string()),
                None,
                None
            )
            .unwrap()
        );
    }
}
