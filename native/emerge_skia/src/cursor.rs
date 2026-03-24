use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorState {
    pub pos: (f32, f32),
    pub visible: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct CursorSnapshot {
    pub state: CursorState,
    pub version: u64,
}

#[derive(Debug)]
struct CursorData {
    state: CursorState,
    version: u64,
}

#[derive(Debug)]
pub struct SharedCursorState {
    inner: Mutex<CursorData>,
}

impl SharedCursorState {
    pub fn new(initial: CursorState) -> Self {
        Self {
            inner: Mutex::new(CursorData {
                state: initial,
                version: 0,
            }),
        }
    }

    pub fn snapshot(&self) -> CursorSnapshot {
        let data = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        CursorSnapshot {
            state: data.state,
            version: data.version,
        }
    }

    pub fn update(&self, state: CursorState) -> CursorSnapshot {
        let mut data = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if data.state != state {
            data.state = state;
            data.version += 1;
        }

        CursorSnapshot {
            state: data.state,
            version: data.version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_cursor_state_only_increments_version_on_change() {
        let state = SharedCursorState::new(CursorState {
            pos: (0.0, 0.0),
            visible: true,
        });

        let initial = state.snapshot();
        assert_eq!(initial.version, 0);
        assert_eq!(initial.state.pos, (0.0, 0.0));
        assert!(initial.state.visible);

        let unchanged = state.update(CursorState {
            pos: (0.0, 0.0),
            visible: true,
        });
        assert_eq!(unchanged.version, 0);

        let changed = state.update(CursorState {
            pos: (4.0, 8.0),
            visible: false,
        });
        assert_eq!(changed.version, 1);
        assert_eq!(changed.state.pos, (4.0, 8.0));
        assert!(!changed.state.visible);
    }
}
