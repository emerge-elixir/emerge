pub(crate) struct Layout {
    frame: Frame,
}

struct Frame {
    location: Coordinate,
    size: Size,
}

struct Coordinate {
    x: f32,
    y: f32,
}

struct Size {
    width: f32,
    height: f32,
}
