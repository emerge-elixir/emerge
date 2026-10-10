use super::*;

fn node() -> ScrollNode {
    ScrollNode {
        mount: 1,
        position: [0.0; 2],
        range: [100.0; 2],
        enabled: [true; 2],
        proposals: Default::default(),
        targets: [None; 2],
        emit: true,
    }
}
fn proposal(id: u64, fraction: f64) -> ScrollProposal {
    ScrollProposal {
        id,
        value: Some(ScrollPosition::Fraction(fraction.to_bits())),
    }
}

#[test]
fn native_geometry_receipts_never_consume_echoes_or_extend_deadlines() {
    let now = Instant::now();
    let mut state = ScrollSession::new(node());
    state.move_by([-10.0, -20.0]);
    state.expect_echo(now);
    state.move_by([-10.0, -20.0]);
    state.expect_echo(now + Duration::from_secs(1));
    state.reconcile(node(), now + Duration::from_secs(4));
    assert_eq!(state.echoes[0].len(), 2);
    let mut app = node();
    app.proposals[0] = proposal(1, 0.1);
    assert_eq!(
        state.reconcile(app.clone(), now + Duration::from_secs(4)),
        [false; 2]
    );
    assert_eq!(state.position, [0.2, 0.4]);
    assert_eq!(state.echoes[0].len(), 1);
    state.reconcile(app, now + Duration::from_secs(8));
    assert_eq!(state.position, [0.2, 0.4]); // processed proposals never replay
    assert!(state.echoes[0].is_empty());
}
#[test]
fn expired_first_seen_fraction_is_a_replacement_and_does_not_affect_other_axis() {
    let now = Instant::now();
    let mut state = ScrollSession::new(node());
    state.move_by([-10.0, -20.0]);
    state.expect_echo(now);
    state.move_by([-10.0, -20.0]);
    state.expect_echo(now);
    let mut app = node();
    app.proposals[0] = proposal(1, 0.1);
    assert_eq!(
        state.reconcile(app, now + Duration::from_secs(5)),
        [true, false]
    );
    assert_eq!(state.position, [0.1, 0.4]);
}
#[test]
fn coalesced_echo_retirement_preserves_newer_movement_and_replacement() {
    let now = Instant::now();
    let mut state = ScrollSession::new(node());
    for delta in [10.0, 20.0, 40.0] {
        state.move_by([-delta, 0.0]);
        state.expect_echo(now);
    }
    let mut app = node();
    app.proposals[0] = proposal(1, 0.9);
    state.reconcile(app.clone(), now);
    app.proposals[0] = proposal(2, 0.1 + 0.2);
    state.reconcile(app, now);
    assert_eq!(state.position[0], 0.9);
    assert_eq!(state.echoes[0].len(), 1);
}
#[test]
fn temporary_zero_range_preserves_progress_but_disabling_resets_it() {
    let now = Instant::now();
    let mut state = ScrollSession::new(node());
    state.move_by([-50.0; 2]);
    let mut zero = node();
    zero.range = [0.0; 2];
    state.reconcile(zero, now);
    assert!(!state.move_by([-50.0; 2]));
    assert_eq!(state.position, [0.5; 2]);
    let mut disabled = node();
    disabled.enabled[0] = false;
    state.reconcile(disabled, now);
    assert_eq!(state.position, [0.0, 0.5]);
    state.reconcile(node(), now);
    assert_eq!(state.position, [0.0, 0.5]);
}
#[test]
fn removing_control_does_not_jump_and_reintroduction_applies_once() {
    let now = Instant::now();
    let mut state = ScrollSession::new(node());
    let mut app = node();
    app.proposals[0] = proposal(1, 0.5);
    state.reconcile(app.clone(), now);
    state.move_by([-10.0, 0.0]);
    app.proposals[0] = ScrollProposal { id: 2, value: None };
    state.reconcile(app.clone(), now);
    assert_eq!(state.position[0], 0.6);
    app.proposals[0] = proposal(3, 0.5);
    state.reconcile(app, now);
    assert_eq!(state.position[0], 0.5);
}
#[test]
fn missing_target_and_same_declaration_with_new_resolution_are_not_deferred_jumps() {
    let now = Instant::now();
    let mut state = ScrollSession::new(node());
    state.move_by([-50.0; 2]);
    let mut app = node();
    app.proposals[0] = ScrollProposal {
        id: 1,
        value: Some(ScrollPosition::Target {
            declaration: 4,
            node: NodeId(0),
        }),
    };
    state.reconcile(app.clone(), now);
    app.proposals[0].value = Some(ScrollPosition::Target {
        declaration: 4,
        node: NodeId(8),
    });
    app.targets[0] = Some(1.0);
    state.reconcile(app, now);
    assert_eq!(state.position, [0.5; 2]);
}
#[test]
fn movement_is_finite_clamped_and_edges_do_not_change_state() {
    let mut state = ScrollSession::new(node());
    assert!(!state.move_by([f64::NAN, f64::INFINITY]));
    assert!(state.move_by([-1000.0; 2]));
    assert_eq!(state.position, [1.0; 2]);
    assert!(!state.move_by([-1.0; 2]));
}
#[test]
fn decoder_rejects_nonfinite_out_of_range_and_unknown_position_variants() {
    for value in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        let bytes: Vec<_> = [0, 1, 86, 0]
            .into_iter()
            .chain(value.to_be_bytes())
            .collect();
        assert!(crate::tree::attrs::decode_attrs(&bytes).is_err());
    }
    assert!(crate::tree::attrs::decode_attrs(&[0, 1, 86, 9]).is_err());
    assert!(crate::tree::attrs::decode_attrs(&[0, 1, 86, 1, 0]).is_err());
}

#[test]
fn target_alignment_uses_layout_content_origin_and_rejects_nested_scrollports() {
    use crate::tree::{
        attrs::Attrs,
        element::{ElementKind, Frame},
    };
    let mut tree = ElementTree::new();
    for id in 0..=3 {
        let mut element =
            Element::with_attrs(NodeId(id), ElementKind::Column, vec![], Attrs::default());
        element.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            ..Frame::default()
        });
        tree.insert(element);
    }
    tree.set_root_id(NodeId(1));
    tree.set_children(&NodeId(1), vec![NodeId(2), NodeId(0)])
        .unwrap();
    tree.set_children(&NodeId(2), vec![NodeId(3)]).unwrap();
    tree.get_mut(&NodeId(1)).unwrap().layout.effective.padding =
        Some(crate::tree::attrs::Padding::Uniform(10.0));
    let frame = tree
        .get_mut(&NodeId(3))
        .unwrap()
        .layout
        .frame
        .as_mut()
        .unwrap();
    frame.x = 60.0;
    frame.y = 60.0;
    assert_eq!(
        target_fraction(&tree, tree.get(&NodeId(1)).unwrap(), NodeId(3), 1, 100.0),
        Some(0.5)
    );
    assert_eq!(
        target_fraction(&tree, tree.get(&NodeId(1)).unwrap(), NodeId(3), 1, 10.0),
        Some(1.0)
    );
    assert_eq!(
        target_fraction(&tree, tree.get(&NodeId(1)).unwrap(), NodeId(0), 1, 100.0),
        None
    );
    tree.get_mut(&NodeId(2))
        .unwrap()
        .layout
        .effective
        .scrollbar_y = Some(true);
    assert_eq!(
        target_fraction(&tree, tree.get(&NodeId(1)).unwrap(), NodeId(3), 0, 100.0),
        Some(0.5)
    );
    assert_eq!(
        target_fraction(&tree, tree.get(&NodeId(1)).unwrap(), NodeId(3), 1, 100.0),
        None
    );
    assert_eq!(
        target_fraction(&tree, tree.get(&NodeId(1)).unwrap(), NodeId(1), 1, 100.0),
        None
    );
}
