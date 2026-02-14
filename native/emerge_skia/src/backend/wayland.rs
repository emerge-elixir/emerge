//! Wayland/X11 backend using winit and glutin.
//!
//! This backend creates a window using winit and renders to it via OpenGL/Skia.

use std::{
    ffi::CString,
    num::NonZeroU32,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Arc,
    },
};

use crossbeam_channel::Receiver;

use glutin::{
    config::{ConfigTemplateBuilder, GlConfig},
    context::{ContextApi, ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext},
    display::{GetGlDisplay, GlDisplay},
    prelude::GlSurface,
    surface::{Surface as GlutinSurface, SurfaceAttributesBuilder, WindowSurface},
};
use glutin_winit::DisplayBuilder;
use raw_window_handle::HasWindowHandle;
use skia_safe::gpu::gl::FramebufferInfo;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{EventLoop, EventLoopProxy},
    keyboard::{Key, NamedKey},
    window::{Window, WindowAttributes},
};

use crate::actors::{EventMsg, RenderMsg};
use crate::input::{
    InputEvent, ACTION_PRESS, ACTION_RELEASE, MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT,
};
use crate::renderer::{RenderState, Renderer};

// ============================================================================
// Configuration
// ============================================================================

#[derive(Clone, Debug)]
pub struct WaylandConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

impl Default for WaylandConfig {
    fn default() -> Self {
        Self {
            title: "Emerge".to_string(),
            width: 800,
            height: 600,
        }
    }
}

// ============================================================================
// Events
// ============================================================================

#[derive(Debug)]
pub enum UserEvent {
    Stop,
    Redraw,
}

// ============================================================================
// GL Environment
// ============================================================================

struct GlEnv {
    gl_surface: GlutinSurface<WindowSurface>,
    gl_context: PossiblyCurrentContext,
    window: Window,
}

// ============================================================================
// Application
// ============================================================================

struct App {
    env: Option<GlEnv>,
    renderer: Option<Renderer>,
    running: bool,
    running_flag: Arc<AtomicBool>,
    render_state: RenderState,
    render_rx: Receiver<RenderMsg>,
    event_tx: crossbeam_channel::Sender<EventMsg>,
    window_size: (u32, u32),
    current_mods: u8,
    cursor_pos: (f32, f32),
    is_focused: bool,
    is_occluded: bool,
    pending_redraw: bool,
}

impl App {
    fn can_present(&self) -> bool {
        self.running && self.is_focused && !self.is_occluded
    }

    fn queue_redraw(&mut self) {
        self.pending_redraw = true;
        if self.can_present() && let Some(env) = &self.env {
            env.window.request_redraw();
        }
    }

    fn flush_render_updates(&mut self) {
        if self.running && self.drain_render_commands() {
            self.queue_redraw();
        }
    }

    fn handle_resize(&mut self, physical_size: winit::dpi::PhysicalSize<u32>) {
        if !self.running {
            return;
        }

        let (w, h): (u32, u32) = physical_size.into();
        self.window_size = (w, h);

        if let (Some(env), Some(renderer)) = (self.env.as_mut(), self.renderer.as_mut()) {
            env.gl_surface.resize(
                &env.gl_context,
                NonZeroU32::new(w.max(1)).unwrap(),
                NonZeroU32::new(h.max(1)).unwrap(),
            );
            renderer.resize((w.max(1), h.max(1)));
            self.queue_redraw();
        }

        if let Some(env) = &self.env {
            let scale = env.window.scale_factor() as f32;
            let _ = self
                .event_tx
                .send(EventMsg::InputEvent(InputEvent::Resized {
                    width: w,
                    height: h,
                    scale_factor: scale,
                }));
        }
    }

    fn redraw(&mut self) {
        if let (Some(env), Some(renderer)) = (self.env.as_mut(), self.renderer.as_mut()) {
            renderer.render(&self.render_state);
            env.gl_surface
                .swap_buffers(&env.gl_context)
                .expect("swap_buffers failed");
        }
    }

    fn send_input_event(&self, event: InputEvent) {
        let _ = self.event_tx.send(EventMsg::InputEvent(event));
    }

    fn drain_render_commands(&mut self) -> bool {
        let mut updated = false;
        while let Ok(msg) = self.render_rx.try_recv() {
            match msg {
                RenderMsg::Commands { commands, version } => {
                    self.render_state.commands = commands;
                    self.render_state.render_version = version;
                    updated = true;
                }
                RenderMsg::Stop => {
                    self.running = false;
                }
                RenderMsg::CursorUpdate { .. } => {}
            }
        }
        updated
    }

    fn mouse_button_name(button: MouseButton) -> &'static str {
        match button {
            MouseButton::Left => "left",
            MouseButton::Right => "right",
            MouseButton::Middle => "middle",
            MouseButton::Back => "back",
            MouseButton::Forward => "forward",
            MouseButton::Other(_) => "other",
        }
    }

    fn key_name(key: &Key) -> String {
        match key {
            Key::Named(named) => match named {
                NamedKey::Escape => "escape".to_string(),
                NamedKey::Backspace => "backspace".to_string(),
                NamedKey::Tab => "tab".to_string(),
                NamedKey::Enter => "enter".to_string(),
                NamedKey::Space => "space".to_string(),
                NamedKey::Delete => "delete".to_string(),
                NamedKey::Insert => "insert".to_string(),
                NamedKey::Home => "home".to_string(),
                NamedKey::End => "end".to_string(),
                NamedKey::PageUp => "page_up".to_string(),
                NamedKey::PageDown => "page_down".to_string(),
                NamedKey::ArrowUp => "up".to_string(),
                NamedKey::ArrowDown => "down".to_string(),
                NamedKey::ArrowLeft => "left".to_string(),
                NamedKey::ArrowRight => "right".to_string(),
                NamedKey::F1 => "f1".to_string(),
                NamedKey::F2 => "f2".to_string(),
                NamedKey::F3 => "f3".to_string(),
                NamedKey::F4 => "f4".to_string(),
                NamedKey::F5 => "f5".to_string(),
                NamedKey::F6 => "f6".to_string(),
                NamedKey::F7 => "f7".to_string(),
                NamedKey::F8 => "f8".to_string(),
                NamedKey::F9 => "f9".to_string(),
                NamedKey::F10 => "f10".to_string(),
                NamedKey::F11 => "f11".to_string(),
                NamedKey::F12 => "f12".to_string(),
                NamedKey::Shift => "shift".to_string(),
                NamedKey::Control => "control".to_string(),
                NamedKey::Alt => "alt".to_string(),
                NamedKey::Super => "super".to_string(),
                NamedKey::CapsLock => "caps_lock".to_string(),
                NamedKey::NumLock => "num_lock".to_string(),
                NamedKey::ScrollLock => "scroll_lock".to_string(),
                NamedKey::PrintScreen => "print_screen".to_string(),
                NamedKey::Pause => "pause".to_string(),
                _ => format!("{:?}", named).to_lowercase(),
            },
            Key::Character(c) => c.to_string(),
            Key::Unidentified(_) => "unknown".to_string(),
            Key::Dead(_) => "dead".to_string(),
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {}

    fn user_event(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Stop => {
                self.running = false;
                self.running_flag.store(false, Ordering::Relaxed);
                event_loop.exit();
            }
            UserEvent::Redraw => {
                self.flush_render_updates();
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        self.flush_render_updates();
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.running = false;
                self.running_flag.store(false, Ordering::Relaxed);
                event_loop.exit();
            }
            WindowEvent::Resized(physical_size) => {
                self.handle_resize(physical_size);
            }
            WindowEvent::RedrawRequested => {
                if self.can_present() {
                    self.pending_redraw = false;
                    self.redraw();
                } else if self.running {
                    self.pending_redraw = true;
                }
            }

            // Mouse cursor movement
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x as f32, position.y as f32);
                self.cursor_pos = (x, y);
                self.send_input_event(InputEvent::CursorPos { x, y });
            }

            // Mouse button press/release
            WindowEvent::MouseInput { state, button, .. } => {
                let action = match state {
                    ElementState::Pressed => ACTION_PRESS,
                    ElementState::Released => ACTION_RELEASE,
                };
                let (x, y) = self.cursor_pos;
                self.send_input_event(InputEvent::CursorButton {
                    button: Self::mouse_button_name(button).to_string(),
                    action,
                    mods: self.current_mods,
                    x,
                    y,
                });
            }

            // Mouse scroll wheel
            WindowEvent::MouseWheel { delta, .. } => {
                let (cursor_x, cursor_y) = self.cursor_pos;
                let event = match delta {
                    winit::event::MouseScrollDelta::LineDelta(dx, dy) => {
                        InputEvent::CursorScrollLines {
                            dx,
                            dy,
                            x: cursor_x,
                            y: cursor_y,
                        }
                    }
                    winit::event::MouseScrollDelta::PixelDelta(pos) => InputEvent::CursorScroll {
                        dx: pos.x as f32,
                        dy: pos.y as f32,
                        x: cursor_x,
                        y: cursor_y,
                    },
                };
                self.send_input_event(event);
            }

            // Cursor entered/exited window
            WindowEvent::CursorEntered { .. } => {
                self.send_input_event(InputEvent::CursorEntered { entered: true });
            }
            WindowEvent::CursorLeft { .. } => {
                self.send_input_event(InputEvent::CursorEntered { entered: false });
            }

            // Keyboard key press/release
            WindowEvent::KeyboardInput { event, .. } => {
                let action = match event.state {
                    ElementState::Pressed => ACTION_PRESS,
                    ElementState::Released => ACTION_RELEASE,
                };
                let key_name = Self::key_name(&event.logical_key);
                self.send_input_event(InputEvent::Key {
                    key: key_name,
                    action,
                    mods: self.current_mods,
                });
            }

            // Modifier state changed
            WindowEvent::ModifiersChanged(mods) => {
                let state = mods.state();
                self.current_mods = 0;
                if state.shift_key() {
                    self.current_mods |= MOD_SHIFT;
                }
                if state.control_key() {
                    self.current_mods |= MOD_CTRL;
                }
                if state.alt_key() {
                    self.current_mods |= MOD_ALT;
                }
                if state.super_key() {
                    self.current_mods |= MOD_META;
                }
            }

            // Window focus changed
            WindowEvent::Focused(focused) => {
                self.is_focused = focused;
                self.send_input_event(InputEvent::Focused { focused });
                if self.can_present() && self.pending_redraw {
                    self.queue_redraw();
                }
            }

            WindowEvent::Occluded(occluded) => {
                self.is_occluded = occluded;
                if self.can_present() && self.pending_redraw {
                    self.queue_redraw();
                }
            }

            _ => {}
        }
    }

    // user_event handled earlier
}

// ============================================================================
// Window and Renderer Creation
// ============================================================================

fn create_window_and_renderer(
    event_loop: &EventLoop<UserEvent>,
    config: &WaylandConfig,
) -> Result<(GlEnv, Renderer), String> {
    let window_attributes = WindowAttributes::default()
        .with_title(&config.title)
        .with_inner_size(LogicalSize::new(config.width, config.height));

    let template = ConfigTemplateBuilder::new()
        .with_alpha_size(8)
        .with_transparency(true);

    let display_builder = DisplayBuilder::new().with_window_attributes(Some(window_attributes));
    let (window, gl_config) = display_builder
        .build(event_loop, template, |configs| {
            configs
                .reduce(|accum, cfg| {
                    if cfg.num_samples() < accum.num_samples() {
                        cfg
                    } else {
                        accum
                    }
                })
                .unwrap()
        })
        .map_err(|err| format!("failed to build display: {err}"))?;

    let window = window.ok_or_else(|| "could not create window".to_string())?;
    let window_handle = window
        .window_handle()
        .map_err(|err| format!("failed to get window handle: {err}"))?;
    let raw_window_handle = window_handle.as_raw();

    let context_attributes = ContextAttributesBuilder::new().build(Some(raw_window_handle));
    let fallback_context_attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::Gles(None))
        .build(Some(raw_window_handle));

    let not_current_gl_context = unsafe {
        gl_config
            .display()
            .create_context(&gl_config, &context_attributes)
            .unwrap_or_else(|_| {
                gl_config
                    .display()
                    .create_context(&gl_config, &fallback_context_attributes)
                    .expect("failed to create GL/GLES context")
            })
    };

    let (width, height): (u32, u32) = window.inner_size().into();
    let attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
        raw_window_handle,
        NonZeroU32::new(width.max(1)).unwrap(),
        NonZeroU32::new(height.max(1)).unwrap(),
    );

    let gl_surface = unsafe {
        gl_config
            .display()
            .create_window_surface(&gl_config, &attrs)
            .map_err(|err| format!("could not create GL window surface: {err}"))?
    };

    let gl_context = not_current_gl_context
        .make_current(&gl_surface)
        .map_err(|err| format!("could not make GL context current: {err}"))?;

    gl::load_with(|s| {
        gl_config
            .display()
            .get_proc_address(CString::new(s).unwrap().as_c_str())
    });

    let interface = skia_safe::gpu::gl::Interface::new_load_with(|name| {
        if name == "eglGetCurrentDisplay" {
            return std::ptr::null();
        }
        gl_config
            .display()
            .get_proc_address(CString::new(name).unwrap().as_c_str())
    })
    .ok_or_else(|| "could not create Skia GL interface".to_string())?;

    let gr_context = skia_safe::gpu::direct_contexts::make_gl(interface, None)
        .ok_or_else(|| "make_gl failed: could not create Skia direct context".to_string())?;

    let fb_info = {
        let mut fboid: i32 = 0;
        unsafe { gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut fboid) };

        FramebufferInfo {
            fboid: fboid as u32,
            format: skia_safe::gpu::gl::Format::RGBA8.into(),
            ..Default::default()
        }
    };

    let num_samples = gl_config.num_samples() as usize;
    let stencil_size = gl_config.stencil_size() as usize;

    let renderer = Renderer::new_gl(
        (width, height),
        fb_info,
        gr_context,
        num_samples,
        stencil_size,
    );

    let env = GlEnv {
        gl_surface,
        gl_context,
        window,
    };

    Ok((env, renderer))
}

// ============================================================================
// Public API
// ============================================================================

/// Run the Wayland/X11 backend event loop.
///
/// This function spawns in the current thread and blocks until the window is closed
/// or `running_flag` is set to false.
pub fn run(
    config: WaylandConfig,
    running_flag: Arc<AtomicBool>,
    event_tx: crossbeam_channel::Sender<EventMsg>,
    render_rx: Receiver<RenderMsg>,
    proxy_tx: Sender<EventLoopProxy<UserEvent>>,
) {
    // Allow running on non-main thread (required for NIF)
    #[cfg(target_os = "linux")]
    let el = {
        use winit::platform::x11::EventLoopBuilderExtX11;
        EventLoop::<UserEvent>::with_user_event()
            .with_any_thread(true)
            .build()
            .expect("Failed to create event loop")
    };

    #[cfg(not(target_os = "linux"))]
    let el = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("Failed to create event loop");

    let proxy = el.create_proxy();
    let _ = proxy_tx.send(proxy);

    let (env, renderer) = match create_window_and_renderer(&el, &config) {
        Ok(values) => values,
        Err(err) => {
            eprintln!("Failed to initialize renderer: {err}");
            running_flag.store(false, Ordering::Relaxed);
            return;
        }
    };

    let size = env.window.inner_size();

    let mut app = App {
        env: Some(env),
        renderer: Some(renderer),
        running: true,
        running_flag,
        render_state: RenderState::default(),
        render_rx,
        event_tx,
        window_size: (size.width, size.height),
        current_mods: 0,
        cursor_pos: (0.0, 0.0),
        is_focused: true,
        is_occluded: false,
        pending_redraw: false,
    };

    app.redraw();
    el.run_app(&mut app).expect("run_app failed");
}
