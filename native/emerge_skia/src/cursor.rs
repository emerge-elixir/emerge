#[derive(Debug, Clone, Copy)]
pub struct CursorState {
    pub pos: (f32, f32),
    pub visible: bool,
}

impl CursorState {
    pub fn new() -> Self {
        Self {
            pos: (0.0, 0.0),
            visible: true,
        }
    }
}
