//! Event-owned scroll progress and app proposals. Pixel offsets are projections.
use crate::tree::element::{Element, ElementTree, NodeId, ParentLink};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollPosition {
    Fraction(u64),
    Target { declaration: u64, node: NodeId },
}
impl ScrollPosition {
    pub fn same_declaration(self, other: Self) -> bool {
        match (self, other) {
            (Self::Target { declaration: a, .. }, Self::Target { declaration: b, .. }) => a == b,
            _ => self == other,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ScrollProposal {
    pub id: u64,
    pub value: Option<ScrollPosition>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ScrollNode {
    pub mount: u64,
    pub position: [f64; 2],
    pub range: [f64; 2],
    pub enabled: [bool; 2],
    pub proposals: [ScrollProposal; 2],
    pub targets: [Option<f64>; 2],
    pub emit: bool,
}
impl ScrollNode {
    pub fn from_element(tree: &ElementTree, element: &Element) -> Self {
        let enabled = [
            element.layout.effective.scrollbar_x.unwrap_or(false),
            element.layout.effective.scrollbar_y.unwrap_or(false),
        ];
        let range = [
            element.layout.scroll_x_max as f64,
            element.layout.scroll_y_max as f64,
        ];
        let offsets = [
            element.layout.scroll_x as f64,
            element.layout.scroll_y as f64,
        ];
        let position = element
            .runtime
            .scroll_position
            .map(|p| p.map(f64::from_bits))
            .unwrap_or_else(|| {
                std::array::from_fn(|axis| {
                    if range[axis] > 0.0 {
                        offsets[axis] / range[axis]
                    } else {
                        0.0
                    }
                })
            });
        let proposals = element.runtime.scroll_proposals;
        let targets = std::array::from_fn(|axis| match proposals[axis].value {
            Some(ScrollPosition::Target { node, .. }) => {
                target_fraction(tree, element, node, axis, range[axis])
            }
            _ => None,
        });
        Self {
            mount: element.lifecycle.mounted_at_revision,
            position,
            range,
            enabled,
            proposals,
            targets,
            emit: element.layout.effective.on_scroll.unwrap_or(false),
        }
    }
}

// A target must belong to this scroll content, not a nearby mount or another
// independently scrolling viewport. Frames are in unscrolled layout coordinates.
fn target_fraction(
    tree: &ElementTree,
    owner: &Element,
    target: NodeId,
    axis: usize,
    range: f64,
) -> Option<f64> {
    if target == NodeId(0) {
        return None; // Reserved wire sentinel for an unresolved semantic key.
    }
    let child = tree.get(&target).filter(|node| node.is_live())?;
    let mut ix = tree.ix_of(&target)?;
    loop {
        let ParentLink::Child { parent, .. } = tree.parent_link_of(ix)? else {
            return None;
        };
        let ancestor = tree.get_ix(parent)?;
        if ancestor.id == owner.id {
            break;
        }
        if if axis == 0 {
            ancestor.layout.effective.scrollbar_x
        } else {
            ancestor.layout.effective.scrollbar_y
        }
        .unwrap_or(false)
        {
            return None;
        }
        ix = parent;
    }
    let origin = owner.layout.frame?;
    let frame = child.layout.frame?;
    let (top, _, _, left) = super::text_content_insets(&owner.layout.effective);
    let offset = if axis == 0 {
        frame.x - origin.x - left
    } else {
        frame.y - origin.y - top
    };
    Some(if range > 0.0 {
        (offset as f64 / range).clamp(0.0, 1.0)
    } else {
        0.0
    })
}

#[derive(Debug)]
pub struct ScrollSession {
    pub node: ScrollNode,
    pub position: [f64; 2],
    processed: [u64; 2],
    echoes: [VecDeque<(f64, Instant)>; 2],
}
impl ScrollSession {
    pub fn new(node: ScrollNode) -> Self {
        Self {
            position: node.position,
            node,
            processed: [0; 2],
            echoes: Default::default(),
        }
    }
    pub fn has_new_unresolved_target(&self, node: &ScrollNode) -> bool {
        node.proposals.iter().enumerate().any(|(axis, proposal)| {
            proposal.id > self.processed[axis]
                && matches!(proposal.value, Some(ScrollPosition::Target { .. }))
                && node.targets[axis].is_none()
        })
    }

    // Return which axes had app replacements that should cancel captured motion.
    pub fn reconcile(&mut self, node: ScrollNode, now: Instant) -> [bool; 2] {
        let mut replaced = [false; 2];
        for (axis, replaced) in replaced.iter_mut().enumerate() {
            prune_echoes(&mut self.echoes[axis], now);
            let proposal = node.proposals[axis];
            if proposal.id > self.processed[axis] {
                self.processed[axis] = proposal.id;
                let target = match proposal.value {
                    Some(ScrollPosition::Fraction(bits)) => {
                        let value = f64::from_bits(bits);
                        if let Some(index) = self.echoes[axis]
                            .iter()
                            .position(|(expected, _)| *expected == value)
                        {
                            self.echoes[axis].drain(..=index);
                            None
                        } else {
                            Some(value)
                        }
                    }
                    Some(ScrollPosition::Target { .. }) => node.targets[axis],
                    None => None,
                };
                if let Some(value) = target.filter(|value| *value != self.position[axis]) {
                    self.position[axis] = value;
                    *replaced = true;
                }
            }
            if !node.enabled[axis] {
                self.position[axis] = 0.0;
                *replaced |= self.node.enabled[axis];
            }
        }
        self.node = node;
        replaced
    }
    pub fn move_by(&mut self, delta: [f64; 2]) -> bool {
        let before = self.position;
        for (axis, delta) in delta.into_iter().enumerate() {
            if self.node.enabled[axis] && self.node.range[axis] > 0.0 && delta.is_finite() {
                self.position[axis] =
                    (self.position[axis] - delta / self.node.range[axis]).clamp(0.0, 1.0);
            }
        }
        before != self.position
    }
    pub fn expect_echo(&mut self, now: Instant) {
        for (axis, queue) in self.echoes.iter_mut().enumerate() {
            prune_echoes(queue, now);
            if queue
                .back()
                .is_some_and(|(value, _)| *value == self.position[axis])
            {
                queue.pop_back();
            }
            queue.push_back((self.position[axis], now + Duration::from_secs(5)));
        }
    }
}

fn prune_echoes(queue: &mut VecDeque<(f64, Instant)>, now: Instant) {
    // Enqueue deadlines are monotonic; do not rescan the live five-second
    // window on every pointer sample or geometry receipt.
    while queue.front().is_some_and(|(_, until)| *until <= now) {
        queue.pop_front();
    }
}

#[cfg(test)]
mod tests;
