use super::super::geometry::Size;
use super::*;
use indexmap::IndexMap;
use smallvec::SmallVec;

use super::super::Attrs;

pub(crate) struct ElSpec {
    pub attrs: Attrs,
}

pub(crate) struct El;

impl ShapeMeta for El {
    type ElementSpec = ElSpec;

    const CAN_BE_ROOT: bool = true;
}

impl ElementInsert for El {
    type Store = ContainerStore;

    fn data_from_spec(spec: ElSpec) -> ContainerData {
        ContainerData {
            attrs: spec.attrs,
            children: SmallVec::new(),
        }
    }

    fn valid_as_parent(
        data_key: ContainerKey,
        data: &ContainerData,
    ) -> Result<ContainerKey, InsertError> {
        if data.children.is_empty() {
            Ok(data_key)
        } else {
            Err(InsertError::ElAlreadyHasChild)
        }
    }

    fn source_children(data: &<Self::Store as ShapeStore>::Data) -> &[Key] {
        data.children.as_slice()
    }
}

impl LayoutBehaviour<&ContainerData> for El {
    fn context(data: &ContainerData, parent: &Context) -> Context {
        let font_size = data.attrs.font_size.unwrap_or(parent.font_size);
        Context {
            font_size,
            padding: data.attrs.padding,
            spacing: data.attrs.spacing,
            content_size: None,
        }
    }

    fn measure(context: &Context, children: &[ChildMeasure<'_>]) -> Measure {
        let child = children
            .first()
            .map(|child| child.measure.intrinsic)
            .unwrap_or(Size::ZERO);

        Measure {
            intrinsic: Size {
                width: child.width + context.padding * 2.0,
                height: child.height + context.padding * 2.0,
            },
        }
    }

    fn resolve(
        context: &Context,
        measure: &Measure,
        children: &[ChildMeasure<'_>],
        placement: &Placement,
    ) -> Resolve {
        let frame = placement.frame(measure.intrinsic);
        let content = frame.inside_padding(context.padding);

        let child_placement: IndexMap<Key, Placement> = children
            .iter()
            .map(|child| {
                (
                    child.key,
                    Placement {
                        origin: content.origin,
                        size: None,
                    },
                )
            })
            .collect();

        let content_size = children
            .first()
            .map_or_else(|| Size::ZERO, |child| child.measure.intrinsic);

        Resolve {
            frame,
            content,
            content_size,
            children: child_placement,
        }
    }
}
