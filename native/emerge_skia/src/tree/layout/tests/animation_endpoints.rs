//! Static allocation controls for layout-resolved animation lengths.
//!
//! These use ordinary native layout, independently of animation sampling or
//! endpoint-cache implementations. Equal resolved dimensions do not necessarily
//! mean that a symbolic length can be replaced by pixels without changing layout.

use super::super::*;
use super::common::*;

#[derive(Clone, Copy, Debug)]
enum Axis {
    Width,
    Height,
}

impl Axis {
    fn attrs(self, length: Length) -> Attrs {
        match self {
            Self::Width => Attrs {
                width: Some(length),
                height: Some(Length::Px(20.0)),
                ..Attrs::default()
            },
            Self::Height => Attrs {
                width: Some(Length::Px(20.0)),
                height: Some(length),
                ..Attrs::default()
            },
        }
    }

    fn size(self, frame: Frame) -> f32 {
        match self {
            Self::Width => frame.width,
            Self::Height => frame.height,
        }
    }
}

fn allocation(axis: Axis, first: Length, second: Length, scale: f32) -> (f32, f32) {
    let (kind, width, height) = match axis {
        Axis::Width => (ElementKind::Row, 600.0, 20.0),
        Axis::Height => (ElementKind::Column, 20.0, 600.0),
    };
    let mut root = make_element("root", kind, fixed_box_attrs(width, height));
    let first = make_element("first", ElementKind::El, axis.attrs(first));
    let second = make_element("second", ElementKind::El, axis.attrs(second));
    root.children = vec![first.id, second.id];
    let (root_id, first_id, second_id) = (root.id, first.id, second.id);
    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    [root, first, second]
        .into_iter()
        .for_each(|element| tree.insert(element));
    layout_tree(
        &mut tree,
        Constraint::new(width as f32 * scale, height as f32 * scale),
        scale,
        &MockTextMeasurer,
    );
    let size = |id| axis.size(tree.get(&id).unwrap().layout.frame.unwrap()) / scale;
    (size(first_id), size(second_id))
}

fn capped_fill(weight: f64) -> Length {
    Length::Min(
        Box::new(Length::Px(50.0)),
        Box::new(Length::FillWeighted(weight)),
    )
}

#[test]
fn unbounded_fill_endpoint_pixel_substitution_preserves_allocation() {
    for axis in [Axis::Width, Axis::Height] {
        for scale in [0.5, 1.0, 2.0] {
            let symbolic = allocation(axis, Length::Fill, Length::Fill, scale);
            let pixels = allocation(axis, Length::Px(symbolic.0 as f64), Length::Fill, scale);
            assert_eq!(symbolic, (300.0, 300.0), "{axis:?}, scale={scale}");
            assert_eq!(pixels, symbolic, "{axis:?}, scale={scale}");
            assert_eq!(
                allocation(axis, Length::Px(170.0), Length::Fill, scale),
                (170.0, 430.0),
                "40px → fill resolved-box midpoint, {axis:?}, scale={scale}",
            );
        }
    }
}

#[test]
fn bounded_fill_endpoint_pixels_lose_sibling_allocation() {
    // A bound limits the visible box, not the share reserved by fill allocation.
    // The animated box alone is an insufficient endpoint representation.
    for axis in [Axis::Width, Axis::Height] {
        for scale in [0.5, 1.0, 2.0] {
            let symbolic = allocation(axis, capped_fill(1.0), Length::Fill, scale);
            let pixels = allocation(axis, Length::Px(symbolic.0 as f64), Length::Fill, scale);
            assert_eq!(symbolic, (50.0, 300.0), "{axis:?}, scale={scale}");
            assert_eq!(pixels, (50.0, 550.0), "{axis:?}, scale={scale}");
            assert_eq!(symbolic.0, pixels.0);
            assert_ne!(symbolic.1, pixels.1);
        }
    }
}

#[test]
fn equal_visible_endpoints_can_have_different_allocation_weights() {
    for axis in [Axis::Width, Axis::Height] {
        let one = allocation(axis, capped_fill(1.0), Length::Fill, 1.0);
        let three = allocation(axis, capped_fill(3.0), Length::Fill, 1.0);
        assert_eq!(one, (50.0, 300.0), "{axis:?}");
        assert_eq!(three, (50.0, 150.0), "{axis:?}");
    }
}

#[test]
fn pixel_only_bounded_endpoint_handoff_jumps_the_sibling() {
    for axis in [Axis::Width, Axis::Height] {
        let symbolic = allocation(axis, capped_fill(1.0), Length::Fill, 1.0);
        let before_completion = allocation(axis, Length::Px(49.99), Length::Fill, 1.0);
        let numeric_endpoint = allocation(axis, Length::Px(symbolic.0 as f64), Length::Fill, 1.0);
        assert!((before_completion.0 - symbolic.0).abs() < 0.02);
        assert!(before_completion.1 - symbolic.1 > 250.0);
        assert_eq!(numeric_endpoint.1 - symbolic.1, 250.0);
    }
}

#[test]
fn recursively_bounded_fill_also_cannot_be_lowered_to_only_pixels() {
    for axis in [Axis::Width, Axis::Height] {
        let length = Length::Max(Box::new(Length::Px(30.0)), Box::new(capped_fill(1.0)));
        let symbolic = allocation(axis, length, Length::Fill, 1.0);
        let pixels = allocation(axis, Length::Px(symbolic.0 as f64), Length::Fill, 1.0);
        assert_eq!(symbolic, (50.0, 300.0), "{axis:?}");
        assert_eq!(pixels, (50.0, 550.0), "{axis:?}");
    }
}
