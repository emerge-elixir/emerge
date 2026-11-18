use super::*;
use crate::vk_window::capabilities::DrmNodeId;
use sctk::dmabuf::{DmabufFeedback, DmabufHandler, DmabufState};
use wayland_client::{globals::GlobalList, protocol::wl_buffer::WlBuffer};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
    zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
};
pub(super) struct Feedback {
    state: DmabufState,
    query: Option<ZwpLinuxDmabufFeedbackV1>,
    pub(super) node: Option<DrmNodeId>,
}
impl Feedback {
    pub(super) fn new(
        globals: &GlobalList,
        qh: &QueueHandle<WindowApp>,
        win: &Window,
        api: Requested,
    ) -> Result<Self, String> {
        let state = DmabufState::new(globals, qh);
        let query = if api == Requested::Vk && state.version().is_some_and(|v| v >= 4) {
            Some(
                state
                    .get_surface_feedback(win.wl_surface(), qh)
                    .map_err(err)?,
            )
        } else {
            None
        };
        Ok(Self {
            state,
            query,
            node: None,
        })
    }
    pub(super) fn pending(&self) -> bool {
        self.query.is_some()
    }
}
impl DmabufHandler for WindowApp {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.feedback.state
    }
    fn dmabuf_feedback(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ZwpLinuxDmabufFeedbackV1,
        f: DmabufFeedback,
    ) {
        let dev = f.main_device();
        self.feedback.node = Some(DrmNodeId {
            major: libc::major(dev),
            minor: libc::minor(dev),
        });
        if let Some(q) = self.feedback.query.take() {
            q.destroy()
        }
    }
    fn created(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ZwpLinuxBufferParamsV1,
        _: WlBuffer,
    ) {
    }
    fn failed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &ZwpLinuxBufferParamsV1) {}
    fn released(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlBuffer) {}
}
sctk::delegate_dmabuf!(WindowApp);
