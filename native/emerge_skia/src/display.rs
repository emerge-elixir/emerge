//! Platform-independent discovery records exposed by the NIF.

#[derive(Debug, rustler::NifMap)]
pub(crate) struct DrmMode {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f64,
    pub preferred: bool,
    pub interlaced: bool,
}

#[derive(Debug, rustler::NifMap)]
pub(crate) struct DrmOutput {
    pub name: String,
    pub connector_id: u32,
    pub connected: bool,
    pub modes: Vec<DrmMode>,
}
