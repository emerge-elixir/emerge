// A moved element whose content casts a shadow stays clipped by its host;
// only the shadow escapes, as it does without the move.
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

const SCREEN: (u8, u8, u8, u8) = (128, 128, 128, 255);
const CARDS: [(u8, u8, u8); 3] = [(255, 0, 0), (0, 255, 0), (0, 0, 255)];

#[derive(Clone, Copy, Debug)]
enum Move {
    Translated,
    Rotated,
    // The reference: the strip laid out one page left instead of moved.
    LaidOut,
}

// A carousel: a 100 × 100 grey screen with a 50 × 100 host at x 0..50
// holding a strip of three 50-wide pages, moved one page left so the second
// page fills the host. Each page holds a card inset 5 px; on screen the
// second card is at 5..45 and the third at 55..95, past the host's edge.
fn carousel(movement: Move, shadowed: bool) -> ElementTree {
    let mut root = element(
        1,
        solid_fill_attrs((128, 128, 128)),
        frame(0.0, 0.0, 100.0, 100.0),
    );
    root.children = vec![NodeId::from_term_bytes(vec![2])];

    let mut host = element(2, Attrs::default(), frame(0.0, 0.0, 50.0, 100.0));
    host.children = vec![NodeId::from_term_bytes(vec![3])];

    let mut strip_attrs = Attrs::default();
    let mut left = 0.0;
    match movement {
        Move::Translated => strip_attrs.move_x = Some(-50.0),
        // A half turn about the strip's centre (75, 50) puts the third page
        // where the first was and the first at 100..150.
        Move::Rotated => strip_attrs.rotate = Some(180.0),
        Move::LaidOut => left = -50.0,
    }
    let mut strip = element(3, strip_attrs, frame(left, 0.0, 150.0, 100.0));

    let mut tree = ElementTree::new();
    tree.set_root_id(root.id);
    tree.insert(root);
    tree.insert(host);
    for (i, rgb) in CARDS.into_iter().enumerate() {
        let x = left + 50.0 * i as f32;
        let page_id = 10 + i as u8;
        let card_id = 20 + i as u8;
        strip.children.push(NodeId::from_term_bytes(vec![page_id]));
        let mut page = element(page_id, Attrs::default(), frame(x, 0.0, 50.0, 100.0));
        page.children = vec![NodeId::from_term_bytes(vec![card_id])];
        let mut card_attrs = solid_fill_attrs(rgb);
        if shadowed {
            card_attrs.box_shadows = shadow();
        }
        tree.insert(page);
        tree.insert(element(
            card_id,
            card_attrs,
            frame(x + 5.0, 5.0, 40.0, 90.0),
        ));
    }
    tree.insert(strip);
    tree
}

fn pixels(tree: &ElementTree) -> Vec<u8> {
    render_scene_to_pixels(100, 100, render_output(tree).scene)
}

fn opaque((r, g, b): (u8, u8, u8)) -> (u8, u8, u8, u8) {
    (r, g, b, 255)
}

fn same_inside_host(a: &[u8], b: &[u8]) -> bool {
    (0..100).all(|y| (0..50).all(|x| rgba_at(a, 100, x, y) == rgba_at(b, 100, x, y)))
}

// Upstream drew the moved strip whole past the host (the third card at
// 55..95) once a card in it cast a shadow; without a shadow it was clipped.
#[test]
fn a_translated_strip_with_shadowed_cards_is_clipped_by_its_host() {
    let moved = pixels(&carousel(Move::Translated, true));
    let at = |x, y| rgba_at(&moved, 100, x, y);
    // The second card fills the host, as if the strip were laid out there.
    assert_eq!(at(25, 50), opaque(CARDS[1]));
    assert!(same_inside_host(
        &moved,
        &pixels(&carousel(Move::LaidOut, true))
    ));
    // The third card, past the host's edge, is not drawn.
    for x in [55, 75, 94] {
        let (r, g, b, _) = at(x, 50);
        assert!(r == g && g == b, "third card drawn at x {x}: {r} {g} {b}");
    }
}

#[test]
fn a_rotated_strip_with_shadowed_cards_is_clipped_by_its_host() {
    let pixels = pixels(&carousel(Move::Rotated, true));
    let at = |x, y| rgba_at(&pixels, 100, x, y);
    assert_eq!(at(25, 50), opaque(CARDS[2]));
    for x in [55, 75, 94] {
        let (r, g, b, _) = at(x, 50);
        assert!(r == g && g == b, "second card drawn at x {x}: {r} {g} {b}");
    }
}

#[test]
fn a_strip_without_shadows_was_already_clipped() {
    for movement in [Move::Translated, Move::Rotated, Move::LaidOut] {
        let pixels = pixels(&carousel(movement, false));
        assert_eq!(rgba_at(&pixels, 100, 75, 50), SCREEN, "{movement:?}");
    }
}

// The shadow itself still escapes a plain host, as an unmoved card's does
// (`test_outer_shadow_escapes_non_scrollable_ancestor_clip`); a scroll
// container clips it on its scrolling axis. (Laid out, the third page is
// culled whole: it lies outside the host. Inside a transform nothing is
// culled against the host.)
#[test]
fn the_shadow_of_a_moved_card_still_escapes_its_host() {
    let pixels = pixels(&carousel(Move::Translated, true));
    assert_ne!(rgba_at(&pixels, 100, 75, 50), SCREEN);
}
