//! Presence bits only. Application messages stay in the installed BEAM route.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Actions(u16);
impl Actions {
    pub const CLICK: u16 = 1;
    pub const FOCUS: u16 = 2;
    pub const BLUR: u16 = 4;
    pub const PRESS: u16 = 8;
    pub const UP: u16 = 16;
    pub const DOWN: u16 = 32;
    pub const LEFT: u16 = 64;
    pub const RIGHT: u16 = 128;
    const DRAG: u16 = 256;
    pub(super) fn set(&mut self, tag: u8, enabled: bool) {
        let bit = match tag {
            40 => Self::CLICK,
            57 => Self::FOCUS,
            58 => Self::BLUR,
            61 => Self::PRESS,
            71 => Self::UP,
            72 => Self::DOWN,
            73 => Self::LEFT,
            74 => Self::RIGHT,
            88 => Self::DRAG,
            _ => unreachable!(),
        };
        if enabled {
            self.0 |= bit
        } else {
            self.0 &= !bit
        }
    }
    pub fn has(self, mask: u16) -> bool {
        self.0 & mask != 0
    }
    pub fn focusable(self) -> bool {
        self.has(Self::FOCUS | Self::BLUR | Self::PRESS)
    }
    pub fn any(self) -> bool {
        self.0 != 0
    }
    pub fn drag(self) -> bool {
        self.has(Self::DRAG)
    }
    pub fn swipes(self) -> bool {
        self.has(Self::UP | Self::DOWN | Self::LEFT | Self::RIGHT)
    }
    pub fn has_competitor(self) -> bool {
        self.has(Self::CLICK | Self::PRESS) || self.swipes()
    }
}
