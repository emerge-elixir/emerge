use super::super::{FontContext, font_info_with_inheritance};
use crate::tree::attrs::{Attrs, FontWeight};

#[test]
fn font_info_with_inheritance_supports_regular_and_extra_light_weights() {
    let inherited = FontContext::default();

    let regular = Attrs {
        font_weight: Some(FontWeight("regular".to_string())),
        ..Attrs::default()
    };
    assert_eq!(font_info_with_inheritance(&regular, &inherited).1, 400);

    let extra_light = Attrs {
        font_weight: Some(FontWeight("extra_light".to_string())),
        ..Attrs::default()
    };
    assert_eq!(font_info_with_inheritance(&extra_light, &inherited).1, 200);
}
