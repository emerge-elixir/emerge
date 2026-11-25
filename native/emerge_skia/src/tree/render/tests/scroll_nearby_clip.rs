// Nearby content attached inside a scroll container's
// content is clipped by that scroll container; the container's own nearby
// still overflows it.
use super::common::*;
use super::*;

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

const RED: (u8, u8, u8, u8) = (255, 0, 0, 255);
const NAV: (u8, u8, u8, u8) = (128, 128, 128, 255);

// A 100 × 50 page scrolled by 20 over a nav bar at y 50..100. A row in the
// page's content (y 30..50 on screen) carries an in_front knob 20 tall,
// at 35..55 on screen: its bottom 5 px have scrolled under the nav.
fn page_with_knob_under_the_nav(page_clip_nearby: bool) -> ElementTree {
    let mut root = element(
        1,
        solid_fill_attrs((0, 0, 0)),
        frame(0.0, 0.0, 100.0, 100.0),
    );
    root.children = vec![
        NodeId::from_term_bytes(vec![2]),
        NodeId::from_term_bytes(vec![4]),
    ];

    let mut page_attrs = solid_fill_attrs((0, 0, 80));
    page_attrs.scrollbar_y = Some(true);
    if page_clip_nearby {
        page_attrs.clip_nearby = Some(true);
    }
    let mut page = element(2, page_attrs, frame(0.0, 0.0, 100.0, 50.0));
    page.layout.scroll_y = 20.0;
    page.layout.scroll_y_max = 40.0;
    page.children = vec![NodeId::from_term_bytes(vec![3])];

    let row = element(
        3,
        solid_fill_attrs((255, 255, 255)),
        frame(0.0, 50.0, 100.0, 20.0),
    );
    let nav = element(
        4,
        solid_fill_attrs((128, 128, 128)),
        frame(0.0, 50.0, 100.0, 50.0),
    );

    let mut tree = ElementTree::new();
    tree.set_root_id(root.id);
    tree.insert(root);
    tree.insert(page);
    tree.insert(row);
    tree.insert(nav);
    mount_nearby(
        &mut tree,
        &NodeId::from_term_bytes(vec![3]),
        NearbySlot::InFront,
        ElementKind::El,
        solid_fill_attrs((255, 0, 0)),
        frame(40.0, 55.0, 20.0, 20.0),
        5,
    );
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

#[test]
fn in_front_nearby_in_scroll_content_is_clipped_by_the_scroll_container() {
    let tree = page_with_knob_under_the_nav(false);
    for pixels in pixels_both_paths(&tree) {
        // Inside the viewport the knob paints over its row.
        assert_eq!(rgba_at(&pixels, 100, 50, 40), RED);
        assert_eq!(rgba_at(&pixels, 100, 50, 48), RED);
        // The part scrolled out of the viewport is not drawn over the nav.
        assert_eq!(rgba_at(&pixels, 100, 50, 52), NAV);
    }
}

#[test]
fn scroll_container_clips_nearby_content_as_clip_nearby_did() {
    let implicit = pixels_both_paths(&page_with_knob_under_the_nav(false));
    let explicit = pixels_both_paths(&page_with_knob_under_the_nav(true));
    assert_eq!(implicit, explicit);
}

#[test]
fn scroll_container_own_nearby_still_overflows_it() {
    let mut tree = page_with_knob_under_the_nav(false);
    // A scrim the size of the screen, attached to the page itself (as the
    // hub's panel overlays are): not scroll content, so not clipped.
    let mut scrim = solid_fill_attrs((0, 255, 0));
    scrim.alpha = Some(1.0);
    mount_nearby(
        &mut tree,
        &NodeId::from_term_bytes(vec![2]),
        NearbySlot::InFront,
        ElementKind::El,
        scrim,
        frame(0.0, 0.0, 100.0, 100.0),
        6,
    );
    for pixels in pixels_both_paths(&tree) {
        assert_eq!(rgba_at(&pixels, 100, 50, 80), (0, 255, 0, 255));
    }
}

#[test]
fn nearby_in_scroll_content_still_escapes_its_non_scrolling_host() {
    let mut tree = page_with_knob_under_the_nav(false);
    // A badge above the row, outside the row's box but inside the viewport.
    mount_nearby(
        &mut tree,
        &NodeId::from_term_bytes(vec![3]),
        NearbySlot::Above,
        ElementKind::El,
        solid_fill_attrs((0, 255, 0)),
        frame(10.0, 40.0, 10.0, 10.0),
        7,
    );
    for pixels in pixels_both_paths(&tree) {
        // Row top is at 30 on screen; the badge sits at 20..30.
        assert_eq!(rgba_at(&pixels, 100, 15, 25), (0, 255, 0, 255));
    }
}
