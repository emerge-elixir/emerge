pub const PROTOCOL_NAME: &str = "emerge_skia_macos";
pub const PROTOCOL_VERSION: u16 = 19;
pub const REQUEST_SET_WINDOW_TITLE: u16 = 0x001D;

pub fn decode_window_title(payload: &[u8]) -> Result<String, String> {
    let Some(prefix) = payload.get(..4) else {
        return Err("invalid window title length".into());
    };
    let length = u32::from_be_bytes([prefix[0], prefix[1], prefix[2], prefix[3]]) as usize;
    if payload.len() - 4 != length {
        return Err("invalid window title payload size".into());
    }
    std::str::from_utf8(&payload[4..])
        .map(str::to_owned)
        .map_err(|_| "invalid window title UTF-8".into())
}

pub const FRAME_INIT: u8 = 1;
pub const FRAME_INIT_OK: u8 = 2;
pub const FRAME_REQUEST: u8 = 3;
pub const FRAME_REPLY: u8 = 4;
pub const FRAME_NOTIFY: u8 = 5;
pub const FRAME_ERROR: u8 = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedFrame {
    pub frame_type: u8,
    pub request_id: u32,
    pub session_id: u64,
    pub tag: u16,
    pub payload: Vec<u8>,
}

pub fn encode_frame(
    frame_type: u8,
    request_id: u32,
    session_id: u64,
    tag: u16,
    payload: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 4 + 8 + 2 + payload.len());
    out.push(frame_type);
    out.extend_from_slice(&request_id.to_be_bytes());
    out.extend_from_slice(&session_id.to_be_bytes());
    out.extend_from_slice(&tag.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn decode_frame(frame: &[u8]) -> Result<DecodedFrame, String> {
    if frame.len() < 15 {
        return Err("frame too short".to_string());
    }

    Ok(DecodedFrame {
        frame_type: frame[0],
        request_id: u32::from_be_bytes(frame[1..5].try_into().unwrap()),
        session_id: u64::from_be_bytes(frame[5..13].try_into().unwrap()),
        tag: u16::from_be_bytes(frame[13..15].try_into().unwrap()),
        payload: frame[15..].to_vec(),
    })
}

pub fn encode_init_ok_payload(host_id: u64, host_pid: u32) -> Vec<u8> {
    let protocol_name = PROTOCOL_NAME.as_bytes();
    let mut out = Vec::with_capacity(2 + protocol_name.len() + 2 + 8 + 4);
    out.extend_from_slice(&(protocol_name.len() as u16).to_be_bytes());
    out.extend_from_slice(protocol_name);
    out.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    out.extend_from_slice(&host_id.to_be_bytes());
    out.extend_from_slice(&host_pid.to_be_bytes());
    out
}

pub fn decode_init_payload(payload: &[u8]) -> Result<(String, u16), String> {
    if payload.len() < 4 {
        return Err("invalid init payload".to_string());
    }

    let name_len = u16::from_be_bytes(payload[0..2].try_into().unwrap()) as usize;

    if payload.len() != 2 + name_len + 2 {
        return Err("invalid init payload size".to_string());
    }

    let protocol_name = String::from_utf8(payload[2..2 + name_len].to_vec())
        .map_err(|_| "invalid init protocol name".to_string())?;
    let version = u16::from_be_bytes(payload[2 + name_len..4 + name_len].try_into().unwrap());
    Ok((protocol_name, version))
}

#[cfg(test)]
pub mod fixtures {
    use super::*;

    pub const SESSION_ID: u64 = 0x0102_0304_0506_0708;
    pub const REQUEST_ID: u32 = 0x1122_3344;
    pub const TAG: u16 = 0x5566;
    pub const PAYLOAD: &[u8] = b"payload";
    pub const HOST_ID: u64 = 0x8877_6655_4433_2211;
    pub const HOST_PID: u32 = 0x99AA_BBCC;

    pub fn request_frame() -> Vec<u8> {
        encode_frame(FRAME_REQUEST, REQUEST_ID, SESSION_ID, TAG, PAYLOAD)
    }

    pub fn init_payload() -> Vec<u8> {
        let protocol_name = PROTOCOL_NAME.as_bytes();
        let mut out = Vec::with_capacity(2 + protocol_name.len() + 2);
        out.extend_from_slice(&(protocol_name.len() as u16).to_be_bytes());
        out.extend_from_slice(protocol_name);
        out.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{fixtures, *};

    #[test]
    fn window_title_payload_is_exact_utf8_with_a_byte_length() {
        assert_eq!(
            decode_window_title(b"\x00\x00\x00\x0aHello \xf0\x9f\x8c\x8d").unwrap(),
            "Hello 🌍"
        );
        assert_eq!(decode_window_title(&[0, 0, 0, 0]).unwrap(), "");
        for invalid in [
            &b""[..],
            &b"\x00\x00\x00"[..],
            &b"\x00\x00\x00\x01"[..],
            &b"\x00\x00\x00\x00x"[..],
            &b"\x00\x00\x00\x01\xff"[..],
        ] {
            assert!(decode_window_title(invalid).is_err());
        }
    }

    #[test]
    fn frame_fixture_roundtrips() {
        let decoded = decode_frame(&fixtures::request_frame()).unwrap();

        assert_eq!(
            decoded,
            DecodedFrame {
                frame_type: FRAME_REQUEST,
                request_id: fixtures::REQUEST_ID,
                session_id: fixtures::SESSION_ID,
                tag: fixtures::TAG,
                payload: fixtures::PAYLOAD.to_vec(),
            }
        );
        assert_eq!(
            encode_frame(
                decoded.frame_type,
                decoded.request_id,
                decoded.session_id,
                decoded.tag,
                &decoded.payload,
            ),
            fixtures::request_frame()
        );
    }

    #[test]
    fn init_fixture_decodes_protocol_identity() {
        assert_eq!(
            decode_init_payload(&fixtures::init_payload()).unwrap(),
            (PROTOCOL_NAME.to_string(), PROTOCOL_VERSION)
        );
    }

    #[test]
    fn init_ok_payload_uses_protocol_identity_and_host_values() {
        let payload = encode_init_ok_payload(fixtures::HOST_ID, fixtures::HOST_PID);
        let name_len = u16::from_be_bytes(payload[0..2].try_into().unwrap()) as usize;

        assert_eq!(&payload[2..2 + name_len], PROTOCOL_NAME.as_bytes());
        assert_eq!(
            u16::from_be_bytes(payload[2 + name_len..4 + name_len].try_into().unwrap()),
            PROTOCOL_VERSION
        );
        assert_eq!(
            u64::from_be_bytes(payload[4 + name_len..12 + name_len].try_into().unwrap()),
            fixtures::HOST_ID
        );
        assert_eq!(
            u32::from_be_bytes(payload[12 + name_len..16 + name_len].try_into().unwrap()),
            fixtures::HOST_PID
        );
    }
}

/// Element notification body shared by the native host and codec tests.
pub fn encode_element_event_payload(
    id: &crate::tree::element::NodeId,
    kind: crate::events::ElementEventKind,
    payload: Option<&crate::events::registry_builder::ElixirEventPayload>,
) -> Vec<u8> {
    use crate::events::registry_builder::ElixirEventPayload;
    let value = match payload {
        Some(ElixirEventPayload::Drag(value)) => [match value.phase {
            crate::events::DragPhase::Start => 0,
            crate::events::DragPhase::Move => 1,
            crate::events::DragPhase::End => 2,
            crate::events::DragPhase::Cancel => 3,
        }]
        .into_iter()
        .chain(value.x.to_be_bytes())
        .chain(value.y.to_be_bytes())
        .collect(),
        Some(ElixirEventPayload::Position(value)) => value
            .x
            .to_be_bytes()
            .into_iter()
            .chain(value.y.to_be_bytes())
            .collect(),
        Some(ElixirEventPayload::String(value)) => value.as_bytes().to_vec(),
        Some(ElixirEventPayload::Float(value)) => value.to_string().into_bytes(),
        Some(ElixirEventPayload::Scroll(value)) => {
            format!("{:?},{:?}", value.x, value.y).into_bytes()
        }
        None => Vec::new(),
    };
    let id = id.to_be_bytes();
    [encode_element_event_kind(kind), u8::from(payload.is_some())]
        .into_iter()
        .chain((id.len() as u32).to_be_bytes())
        .chain(id)
        .chain((value.len() as u32).to_be_bytes())
        .chain(value)
        .collect()
}

pub fn encode_element_event_kind(kind: crate::events::ElementEventKind) -> u8 {
    use crate::events::ElementEventKind;
    match kind {
        ElementEventKind::Click => ELEMENT_EVENT_CLICK,
        ElementEventKind::Press => ELEMENT_EVENT_PRESS,
        ElementEventKind::SwipeUp => ELEMENT_EVENT_SWIPE_UP,
        ElementEventKind::SwipeDown => ELEMENT_EVENT_SWIPE_DOWN,
        ElementEventKind::SwipeLeft => ELEMENT_EVENT_SWIPE_LEFT,
        ElementEventKind::SwipeRight => ELEMENT_EVENT_SWIPE_RIGHT,
        ElementEventKind::KeyDown => ELEMENT_EVENT_KEY_DOWN,
        ElementEventKind::KeyUp => ELEMENT_EVENT_KEY_UP,
        ElementEventKind::KeyPress => ELEMENT_EVENT_KEY_PRESS,
        ElementEventKind::VirtualKeyHold => ELEMENT_EVENT_VIRTUAL_KEY_HOLD,
        ElementEventKind::MouseDown => ELEMENT_EVENT_MOUSE_DOWN,
        ElementEventKind::MouseUp => ELEMENT_EVENT_MOUSE_UP,
        ElementEventKind::MouseEnter => ELEMENT_EVENT_MOUSE_ENTER,
        ElementEventKind::MouseLeave => ELEMENT_EVENT_MOUSE_LEAVE,
        ElementEventKind::MouseMove => ELEMENT_EVENT_MOUSE_MOVE,
        ElementEventKind::Drag => ELEMENT_EVENT_DRAG,
        ElementEventKind::Focus => ELEMENT_EVENT_FOCUS,
        ElementEventKind::Blur => ELEMENT_EVENT_BLUR,
        ElementEventKind::Change => ELEMENT_EVENT_CHANGE,
        ElementEventKind::Scroll => ELEMENT_EVENT_SCROLL,
    }
}

const ELEMENT_EVENT_CLICK: u8 = 1;
const ELEMENT_EVENT_PRESS: u8 = 2;
const ELEMENT_EVENT_SWIPE_UP: u8 = 3;
const ELEMENT_EVENT_SWIPE_DOWN: u8 = 4;
const ELEMENT_EVENT_SWIPE_LEFT: u8 = 5;
const ELEMENT_EVENT_SWIPE_RIGHT: u8 = 6;
const ELEMENT_EVENT_KEY_DOWN: u8 = 7;
const ELEMENT_EVENT_KEY_UP: u8 = 8;
const ELEMENT_EVENT_KEY_PRESS: u8 = 9;
const ELEMENT_EVENT_VIRTUAL_KEY_HOLD: u8 = 10;
const ELEMENT_EVENT_MOUSE_DOWN: u8 = 11;
const ELEMENT_EVENT_MOUSE_UP: u8 = 12;
const ELEMENT_EVENT_MOUSE_ENTER: u8 = 13;
const ELEMENT_EVENT_MOUSE_LEAVE: u8 = 14;
const ELEMENT_EVENT_MOUSE_MOVE: u8 = 15;
const ELEMENT_EVENT_FOCUS: u8 = 16;
const ELEMENT_EVENT_BLUR: u8 = 17;
const ELEMENT_EVENT_CHANGE: u8 = 18;
const ELEMENT_EVENT_DRAG: u8 = 20;
const ELEMENT_EVENT_SCROLL: u8 = 19;

#[cfg(test)]
mod position_tests {
    use super::*;
    use crate::events::{ElementEventKind, PointerPosition, registry_builder::ElixirEventPayload};
    use crate::tree::element::NodeId;

    #[test]
    fn element_position_notifications_use_exact_f64_wire_values() {
        for (kind, tag) in [
            (ElementEventKind::Click, 1),
            (ElementEventKind::MouseDown, 11),
            (ElementEventKind::MouseUp, 12),
            (ElementEventKind::MouseEnter, 13),
            (ElementEventKind::MouseLeave, 14),
            (ElementEventKind::MouseMove, 15),
        ] {
            let payload = ElixirEventPayload::Position(PointerPosition { x: 35.5, y: -2.25 });
            let encoded = encode_element_event_payload(&NodeId(42), kind, Some(&payload));
            let expected: Vec<u8> = [tag, 1]
                .into_iter()
                .chain(8u32.to_be_bytes())
                .chain(42u64.to_be_bytes())
                .chain(16u32.to_be_bytes())
                .chain(0x4041_c000_0000_0000u64.to_be_bytes())
                .chain(0xc002_0000_0000_0000u64.to_be_bytes())
                .collect();
            assert_eq!(encoded, expected);
        }
        assert_eq!(
            encode_element_event_payload(&NodeId(42), ElementEventKind::Press, None),
            [2u8, 0]
                .into_iter()
                .chain(8u32.to_be_bytes())
                .chain(42u64.to_be_bytes())
                .chain(0u32.to_be_bytes())
                .collect::<Vec<_>>()
        );
    }
}

#[cfg(test)]
mod drag_payload_tests {
    use super::*;
    use crate::events::{
        DragPayload, DragPhase, ElementEventKind, registry_builder::ElixirEventPayload,
    };
    use crate::tree::element::NodeId;

    #[test]
    fn drag_wire_payload_encodes_phase_and_two_big_endian_floats() {
        for (phase, tag) in [
            (DragPhase::Start, 0),
            (DragPhase::Move, 1),
            (DragPhase::End, 2),
            (DragPhase::Cancel, 3),
        ] {
            let payload = ElixirEventPayload::Drag(DragPayload {
                phase,
                x: 35.5,
                y: -2.25,
            });
            let encoded =
                encode_element_event_payload(&NodeId(42), ElementEventKind::Drag, Some(&payload));
            let expected: Vec<u8> = [20, 1]
                .into_iter()
                .chain(8u32.to_be_bytes())
                .chain(42u64.to_be_bytes())
                .chain(17u32.to_be_bytes())
                .chain([tag])
                .chain(0x4041_c000_0000_0000u64.to_be_bytes())
                .chain(0xc002_0000_0000_0000u64.to_be_bytes())
                .collect();
            assert_eq!(encoded, expected);
        }
    }
}
