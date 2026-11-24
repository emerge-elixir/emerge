// A shadowed element inside an animated, rotated or faded wrapper in scroll
// content stays clipped by the scroll container.
use super::common::*;
use super::*;
use crate::tree::animation::{AnimationCurve, AnimationRepeat, AnimationSpec};

fn frame(x: f32, y: f32, width: f32, height: f32) -> Frame {
    Frame {
        x,
        y,
        width,
        height,
        content_width: width,
        content_height: height,
    }
}

fn element(id: u8, attrs: Attrs, frame: Frame) -> Element {
    let mut element = Element::with_attrs(
        NodeId::from_term_bytes(vec![id]),
        ElementKind::El,
        Vec::new(),
        attrs,
    );
    element.layout.frame = Some(frame);
    element
}

fn shadow() -> Option<Vec<BoxShadow>> {
    Some(vec![BoxShadow {
        offset_x: 0.0,
        offset_y: 2.0,
        blur: 4.0,
        size: 0.0,
        color: Color::Rgba {
            r: 0,
            g: 0,
            b: 0,
            a: 60,
        },
        inset: false,
    }])
}

fn animate_enter() -> AnimationSpec {
    AnimationSpec {
        keyframes: vec![
            Attrs {
                scale: Some(0.94),
                alpha: Some(0.0),
                ..Attrs::default()
            },
            Attrs {
                scale: Some(1.0),
                alpha: Some(1.0),
                ..Attrs::default()
            },
        ],
        duration_ms: 260.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Once,
    }
}

const RED: (u8, u8, u8, u8) = (255, 0, 0, 255);
const NAV: (u8, u8, u8, u8) = (128, 128, 128, 255);

const PAGE: (u8, u8, u8, u8) = (0, 0, 80, 255);

#[derive(Clone, Copy, Debug)]
enum Wrapper {
    Plain,
    AnimateEnter,
    Rotated,
    Faded,
}

// The Media screen's hero art: a 100 × 100 screen with a nav bar at y 0..20
// over a page (y 20..100) scrolled by 30. The page holds a wrapper around a
// rounded, shadowed red cover at 10..50 × 30..70, so on screen the cover is
// at y 0..40 and its top 20 px have scrolled under the nav.
fn screen(wrapper: Wrapper, shadowed: bool) -> ElementTree {
    let mut root = element(
        1,
        solid_fill_attrs((0, 0, 0)),
        frame(0.0, 0.0, 100.0, 100.0),
    );
    root.children = vec![
        NodeId::from_term_bytes(vec![2]),
        NodeId::from_term_bytes(vec![3]),
    ];
    let nav = element(
        2,
        solid_fill_attrs((128, 128, 128)),
        frame(0.0, 0.0, 100.0, 20.0),
    );

    let mut page_attrs = solid_fill_attrs((0, 0, 80));
    page_attrs.scrollbar_y = Some(true);
    let mut page = element(3, page_attrs, frame(0.0, 20.0, 100.0, 80.0));
    page.layout.scroll_y = 30.0;
    page.layout.scroll_y_max = 100.0;
    page.children = vec![NodeId::from_term_bytes(vec![4])];

    let mut wrapper_attrs = Attrs::default();
    match wrapper {
        Wrapper::Plain => {}
        Wrapper::AnimateEnter => wrapper_attrs.animate_enter = Some(animate_enter()),
        Wrapper::Rotated => wrapper_attrs.rotate = Some(0.5),
        Wrapper::Faded => wrapper_attrs.alpha = Some(0.999),
    }
    let mut wrapper = element(4, wrapper_attrs, frame(10.0, 30.0, 40.0, 40.0));
    wrapper.children = vec![NodeId::from_term_bytes(vec![5])];

    let mut cover_attrs = solid_fill_attrs((255, 0, 0));
    cover_attrs.border_radius = Some(BorderRadius::Uniform(3.0));
    if shadowed {
        cover_attrs.box_shadows = shadow();
    }
    let cover = element(5, cover_attrs, frame(10.0, 30.0, 40.0, 40.0));

    let mut tree = ElementTree::new();
    tree.set_root_id(root.id);
    tree.insert(root);
    tree.insert(nav);
    tree.insert(page);
    tree.insert(wrapper);
    tree.insert(cover);
    tree
}

fn pixels_both_paths(tree: &ElementTree) -> [Vec<u8>; 2] {
    let plain = render_output(tree).scene;
    let layered = super::super::render_tree_scene_with_scroll_layers(tree).scene;
    [
        render_scene_to_pixels(100, 100, plain),
        render_scene_to_pixels(100, 100, layered),
    ]
}

fn assert_clipped_to_the_page(wrapper: Wrapper, shadowed: bool) {
    for (path, pixels) in pixels_both_paths(&screen(wrapper, shadowed))
        .iter()
        .enumerate()
    {
        let at = |y| rgba_at(pixels, 100, 30, y);
        // In the viewport the cover is drawn.
        assert_eq!(at(30), RED, "{wrapper:?}, path {path}");
        // Scrolled above the page, it is not drawn over the nav.
        assert_eq!(at(10), NAV, "{wrapper:?}, path {path}");
        assert_eq!(at(19), NAV, "{wrapper:?}, path {path}");
        // Its shadow still falls on the page under it.
        if shadowed {
            assert_ne!(at(41), PAGE, "{wrapper:?}, path {path}");
        }
    }
}

#[test]
fn shadowed_cover_in_an_animate_enter_wrapper_is_clipped_by_the_scroll_container() {
    assert_clipped_to_the_page(Wrapper::AnimateEnter, true);
}

#[test]
fn shadowed_cover_in_a_rotated_wrapper_is_clipped_by_the_scroll_container() {
    assert_clipped_to_the_page(Wrapper::Rotated, true);
}

#[test]
fn shadowed_cover_in_a_faded_wrapper_is_clipped_by_the_scroll_container() {
    assert_clipped_to_the_page(Wrapper::Faded, true);
}

#[test]
fn covers_without_a_mixed_shadow_were_already_clipped() {
    assert_clipped_to_the_page(Wrapper::Plain, true);
    assert_clipped_to_the_page(Wrapper::AnimateEnter, false);
}
