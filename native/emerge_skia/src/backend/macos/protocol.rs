pub const PROTOCOL_NAME: &str = "emerge_skia_macos";
pub const PROTOCOL_VERSION: u16 = 8;

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
