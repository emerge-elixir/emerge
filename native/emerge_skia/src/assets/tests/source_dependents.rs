use super::*;
use crate::tree::{
    attrs::{Attrs, Background, ImageFit, MouseOverAttrs},
    element::{Element, ElementKind, ElementTree, NodeId},
};
use std::cell::Cell;

#[test]
fn status_changes_visit_existing_source_owners_not_unrelated_elements_and_stale_declarations_fall_back()
 {
    for unrelated in [0, 1, 64, 512, 2048] {
        let runtime = AssetRuntime::new();
        let _guard = runtime.enter();
        let first = ImageSource::Id("first".into());
        let inactive = ImageSource::Id("inactive".into());
        let mut tree = ElementTree::new();
        tree.insert(Element::with_attrs(
            NodeId(1),
            ElementKind::Column,
            Vec::new(),
            Attrs::default(),
        ));
        tree.insert(Element::with_attrs(
            NodeId(2),
            ElementKind::Image,
            Vec::new(),
            Attrs {
                image_src: Some(first.clone()),
                ..Attrs::default()
            },
        ));
        tree.insert(Element::with_attrs(
            NodeId(3),
            ElementKind::El,
            Vec::new(),
            Attrs {
                mouse_over: Some(MouseOverAttrs {
                    background: Some(Background::Image {
                        source: inactive.clone(),
                        fit: ImageFit::Contain,
                    }),
                    ..MouseOverAttrs::default()
                }),
                ..Attrs::default()
            },
        ));
        for i in 0..unrelated {
            tree.insert(Element::with_attrs(
                NodeId(i + 4),
                ElementKind::El,
                Vec::new(),
                Attrs::default(),
            ));
        }
        tree.set_root_id(NodeId(1));
        tree.set_children(&NodeId(1), (2..unrelated + 4).map(NodeId).collect())
            .unwrap();
        tree.clear_refresh_dirty();
        for (source, expected) in [(&first, NodeId(2)), (&inactive, NodeId(3))] {
            let visits = Cell::new(0);
            let owners = tree
                .changed_source_owners(|candidate| {
                    visits.set(visits.get() + 1);
                    candidate == source
                })
                .unwrap();
            assert_eq!(owners, vec![expected]);
            assert_eq!(visits.get(), 2, "unrelated={unrelated}");
        }
        tree.get_mut(&NodeId(2)).unwrap().spec.declared.image_src = Some(inactive.clone());
        assert!(
            tree.changed_source_owners(|source| source == &inactive)
                .is_none(),
            "unflushed moved reference is unknown, not absent"
        );
        tree.clear_refresh_dirty();
        assert_eq!(
            tree.changed_source_owners(|source| source == &inactive),
            Some(vec![NodeId(2), NodeId(3)])
        );
    }
}
