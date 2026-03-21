use smithay_client_toolkit::{
    compositor::CompositorState,
    shell::{WaylandSurface, xdg::window::Window},
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, globals::GlobalList, protocol::wl_surface,
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{Event as FractionalScaleEvent, WpFractionalScaleV1},
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};

use super::runtime::WaylandApp;

#[derive(Clone)]
pub(super) struct FractionalScaleData {
    pub(super) surface: wl_surface::WlSurface,
}

pub(super) struct ProtocolHandles {
    // These protocol objects must stay alive for the lifetime of the surface.
    _compositor_state: CompositorState,
    _viewporter: Option<WpViewporter>,
    pub(super) viewport: Option<WpViewport>,
    _fractional_scale: Option<WpFractionalScaleV1>,
}

impl ProtocolHandles {
    pub(super) fn new(
        globals: &GlobalList,
        qh: &QueueHandle<WaylandApp>,
        compositor_state: CompositorState,
        window: &Window,
    ) -> Self {
        let viewporter = globals.bind(qh, 1..=1, ()).ok();
        let viewport = viewporter
            .as_ref()
            .map(|viewporter: &WpViewporter| viewporter.get_viewport(window.wl_surface(), qh, ()));
        let fractional_scale_manager = globals.bind(qh, 1..=1, ()).ok();
        let fractional_scale =
            fractional_scale_manager
                .as_ref()
                .and_then(|manager: &WpFractionalScaleManagerV1| {
                    viewport.as_ref().map(|_| {
                        manager.get_fractional_scale(
                            window.wl_surface(),
                            qh,
                            FractionalScaleData {
                                surface: window.wl_surface().clone(),
                            },
                        )
                    })
                });

        Self {
            _compositor_state: compositor_state,
            _viewporter: viewporter,
            viewport,
            _fractional_scale: fractional_scale,
        }
    }
}

impl Dispatch<WpViewporter, ()> for WaylandApp {
    fn event(
        _: &mut Self,
        _: &WpViewporter,
        _: <WpViewporter as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        unreachable!("wp_viewporter::Event is empty in version 1")
    }
}

impl Dispatch<WpViewport, ()> for WaylandApp {
    fn event(
        _: &mut Self,
        _: &WpViewport,
        _: <WpViewport as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        unreachable!("wp_viewport::Event is empty in version 1")
    }
}

impl Dispatch<WpFractionalScaleManagerV1, ()> for WaylandApp {
    fn event(
        _: &mut Self,
        _: &WpFractionalScaleManagerV1,
        _: <WpFractionalScaleManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        unreachable!("wp_fractional_scale_manager_v1 has no events")
    }
}

impl Dispatch<WpFractionalScaleV1, FractionalScaleData> for WaylandApp {
    fn event(
        state: &mut Self,
        _: &WpFractionalScaleV1,
        event: <WpFractionalScaleV1 as Proxy>::Event,
        data: &FractionalScaleData,
        conn: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if data.surface != *state.window.wl_surface() {
            return;
        }

        let FractionalScaleEvent::PreferredScale { scale } = event else {
            return;
        };

        state
            .geometry
            .set_preferred_fractional_scale(Some(scale as f32 / 120.0));
        state.reconfigure_surface_geometry(conn);
    }
}
