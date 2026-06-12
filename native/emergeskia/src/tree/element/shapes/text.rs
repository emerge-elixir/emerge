use super::super::geometry::{Rect, Size};
use super::*;
use smallvec::SmallVec;
use indexmap::IndexMap;

pub(crate) struct TextSpec {
    pub content: String,
}

pub(crate) struct Text;

impl ShapeMeta for Text {
    type ElementSpec = TextSpec;

    const CAN_BE_ROOT: bool = false;
}

impl ElementInsert for Text {
    type Store = TextStore;

    fn data_from_spec(spec: TextSpec) -> TextData {
        TextData {
            content: spec.content,
        }
    }

    fn valid_as_parent(_data_key: TextKey, _data: &TextData) -> Result<ContainerKey, InsertError> {
        Err(InsertError::ParentCannotHaveChildren)
    }
}

impl LayoutBehaviour<&TextData> for Text {
    fn context(data: &TextData, parent: &Context) -> Context {
        let size = measure_text(data.content.as_str(), parent.font_size);

        Context {
            font_size: parent.font_size,
            content_size: Some(size),
            ..Context::default()
        }
    }

    fn measure(context: &Context, _children: &[ChildMeasure<'_>]) -> Measure {
        let Some(size) = context.content_size else {
            unreachable!("text context expected");
        };

        Measure { intrinsic: size }
    }

    fn resolve(
        _context: &Context,
        measure: &Measure,
        _children: &[ChildMeasure<'_>],
        placement: &Placement,
    ) -> Resolve {
        let frame = placement.frame(measure.intrinsic);
        Resolve {
            frame,
            content: Rect {
                origin: frame.origin,
                size: frame.size,
            },
            content_size: frame.size,
            children: IndexMap::new(),
        }
    }
}

fn measure_text(content: &str, font_size: f32) -> Size {
    Size {
        width: content.len() as f32 * font_size * 0.5,
        height: font_size,
    }
}
