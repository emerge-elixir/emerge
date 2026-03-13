use super::*;

pub(super) fn render_tree(tree: &ElementTree) -> Vec<DrawCmd> {
    super::super::render_tree(tree).commands
}

pub(super) fn build_tree_with_attrs(mut attrs: Attrs) -> ElementTree {
    if attrs.background.is_none() {
        attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    }

    let id = ElementId::from_term_bytes(vec![1]);
    let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
    element.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 50.0,
        content_width: 100.0,
        content_height: 50.0,
    });

    let mut tree = ElementTree::new();
    tree.root = Some(id);
    tree.insert(element);
    tree
}

pub(super) fn build_tree_with_frame(mut attrs: Attrs, frame: Frame) -> ElementTree {
    if attrs.background.is_none() {
        attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    }

    let id = ElementId::from_term_bytes(vec![1]);
    let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
    element.frame = Some(frame);

    let mut tree = ElementTree::new();
    tree.root = Some(id);
    tree.insert(element);
    tree
}

pub(super) fn build_text_tree_with_frame(attrs: Attrs, frame: Frame) -> ElementTree {
    let id = ElementId::from_term_bytes(vec![2]);
    let mut element = Element::with_attrs(id.clone(), ElementKind::Text, Vec::new(), attrs);
    element.frame = Some(frame);

    let mut tree = ElementTree::new();
    tree.root = Some(id);
    tree.insert(element);
    tree
}

pub(super) fn build_text_input_tree_with_frame(attrs: Attrs, frame: Frame) -> ElementTree {
    let id = ElementId::from_term_bytes(vec![3]);
    let mut element = Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);
    element.frame = Some(frame);

    let mut tree = ElementTree::new();
    tree.root = Some(id);
    tree.insert(element);
    tree
}

pub(super) fn build_tree_with_child_frame(
    mut parent_attrs: Attrs,
    parent_frame: Frame,
    mut child_attrs: Attrs,
    child_frame: Frame,
) -> ElementTree {
    if parent_attrs.background.is_none() {
        parent_attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    }

    if child_attrs.background.is_none() {
        child_attrs.background = Some(Background::Color(Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        }));
    }

    let parent_id = ElementId::from_term_bytes(vec![4]);
    let child_id = ElementId::from_term_bytes(vec![5]);

    let mut parent =
        Element::with_attrs(parent_id.clone(), ElementKind::El, Vec::new(), parent_attrs);
    parent.children = vec![child_id.clone()];
    parent.frame = Some(parent_frame);

    let mut child = Element::with_attrs(child_id.clone(), ElementKind::El, Vec::new(), child_attrs);
    child.frame = Some(child_frame);

    let mut tree = ElementTree::new();
    tree.root = Some(parent_id);
    tree.insert(parent);
    tree.insert(child);
    tree
}

pub(super) fn mount_nearby(
    tree: &mut ElementTree,
    host_id: &ElementId,
    slot: NearbySlot,
    kind: ElementKind,
    attrs: Attrs,
    frame: Frame,
    id_byte: u8,
) {
    let nearby_id = ElementId::from_term_bytes(vec![id_byte]);
    let mut nearby = Element::with_attrs(nearby_id.clone(), kind, Vec::new(), attrs);
    nearby.frame = Some(frame);
    tree.insert(nearby);
    tree.get_mut(host_id)
        .expect("host should exist")
        .nearby
        .set(slot, Some(nearby_id));
}

pub(super) fn solid_fill_attrs(rgb: (u8, u8, u8)) -> Attrs {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Rgb {
        r: rgb.0,
        g: rgb.1,
        b: rgb.2,
    }));
    attrs
}

pub(super) fn nearby_origin(
    parent_frame: Frame,
    nearby_frame: Frame,
    slot: NearbySlot,
    align_x: AlignX,
    align_y: AlignY,
) -> (f32, f32) {
    let x = match slot {
        NearbySlot::BehindContent | NearbySlot::Above | NearbySlot::Below | NearbySlot::InFront => {
            match align_x {
                AlignX::Left => parent_frame.x,
                AlignX::Center => parent_frame.x + (parent_frame.width - nearby_frame.width) / 2.0,
                AlignX::Right => parent_frame.x + parent_frame.width - nearby_frame.width,
            }
        }
        NearbySlot::OnLeft => parent_frame.x - nearby_frame.width,
        NearbySlot::OnRight => parent_frame.x + parent_frame.width,
    };

    let y = match slot {
        NearbySlot::Above => parent_frame.y - nearby_frame.height,
        NearbySlot::Below => parent_frame.y + parent_frame.height,
        NearbySlot::BehindContent
        | NearbySlot::OnLeft
        | NearbySlot::OnRight
        | NearbySlot::InFront => match align_y {
            AlignY::Top => parent_frame.y,
            AlignY::Center => parent_frame.y + (parent_frame.height - nearby_frame.height) / 2.0,
            AlignY::Bottom => parent_frame.y + parent_frame.height - nearby_frame.height,
        },
    };

    (x, y)
}

pub(super) fn build_paragraph_tree(mut attrs: Attrs, frame: Frame) -> ElementTree {
    let id = ElementId::from_term_bytes(vec![10]);
    attrs.background = attrs.background.take();
    let mut element = Element::with_attrs(id.clone(), ElementKind::Paragraph, Vec::new(), attrs);
    element.frame = Some(frame);

    let mut tree = ElementTree::new();
    tree.root = Some(id);
    tree.insert(element);
    tree
}
