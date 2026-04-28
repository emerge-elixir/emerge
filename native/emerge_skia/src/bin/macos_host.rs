#[cfg(all(feature = "macos", target_os = "macos"))]
mod app {
    use std::{
        cell::{Cell, RefCell},
        collections::{HashMap, HashSet},
        env, fs,
        io::{self, Read, Write},
        os::unix::net::{UnixListener, UnixStream},
        path::PathBuf,
        rc::{Rc, Weak},
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicU64, Ordering},
        },
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
    use emerge_skia::{
        assets::{self, AssetConfig},
        events::{
            ElementEventKind, HostEventRuntime, HostEventSink, TextInputCommandRequest,
            TextInputEditRequest, TextInputState,
        },
        input::InputEvent,
        keys::CanonicalKey,
        renderer::{
            CleanSubtreeCacheConfig, RenderFrame, RenderState, RendererCacheConfig, SceneRenderer,
        },
        services::{self, OffscreenRenderOptions},
        stats::{RendererStatsCollector, format_renderer_stats_log},
        tree::{
            animation::AnimationRuntime,
            deserialize,
            element::ElementTree,
            invalidation::{
                RefreshAvailability, RefreshDecision, TreeInvalidation, decide_refresh_action,
            },
            layout::{
                Constraint, LayoutOutput, layout_and_refresh_default,
                layout_and_refresh_default_with_animation, refresh,
            },
            patch,
        },
    };
    use objc2::{
        AnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class,
        msg_send,
        rc::{Allocated, Retained, autoreleasepool},
        runtime::{AnyObject, NSObjectProtocol, ProtocolObject, Sel},
    };
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSAutoresizingMaskOptions,
        NSBackingStoreType, NSEvent, NSEventMask, NSEventModifierFlags, NSEventTrackingRunLoopMode,
        NSEventType, NSImage, NSImageScaling, NSImageView, NSTextInputClient, NSTrackingArea,
        NSTrackingAreaOptions, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
    };
    use objc2_core_foundation::CGSize;
    use objc2_foundation::{
        NSArray, NSAttributedString, NSData, NSDate, NSDefaultRunLoopMode, NSNotification,
        NSObject, NSPoint, NSRange, NSRect, NSSize, NSString,
    };
    use objc2_metal::{
        MTLCommandBuffer, MTLCommandQueue, MTLCreateSystemDefaultDevice, MTLDevice, MTLDrawable,
        MTLPixelFormat,
    };
    use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};
    use skia_safe::{
        AlphaType, ColorType, EncodedImageFormat, ImageInfo, Surface,
        gpu::{self, SurfaceOrigin, backend_render_targets, mtl},
        surfaces,
    };

    const PROTOCOL_NAME: &str = "emerge_skia_macos";
    const PROTOCOL_VERSION: u16 = 8;

    const FRAME_INIT: u8 = 1;
    const FRAME_INIT_OK: u8 = 2;
    const FRAME_REQUEST: u8 = 3;
    const FRAME_REPLY: u8 = 4;
    const FRAME_NOTIFY: u8 = 5;
    const FRAME_ERROR: u8 = 6;

    const REQUEST_START_SESSION: u16 = 0x0010;
    const REQUEST_STOP_SESSION: u16 = 0x0011;
    const REQUEST_SESSION_RUNNING: u16 = 0x0012;
    const REQUEST_UPLOAD_TREE: u16 = 0x0013;
    const REQUEST_PATCH_TREE: u16 = 0x0014;
    const REQUEST_SHUTDOWN_HOST: u16 = 0x0015;
    const REQUEST_SET_INPUT_MASK: u16 = 0x0016;
    const REQUEST_MEASURE_TEXT: u16 = 0x0017;
    const REQUEST_LOAD_FONT: u16 = 0x0018;
    const REQUEST_CONFIGURE_ASSETS: u16 = 0x0019;
    const REQUEST_RENDER_TREE_TO_PIXELS: u16 = 0x001A;
    const REQUEST_RENDER_TREE_TO_PNG: u16 = 0x001B;

    const ASSET_MODE_AWAIT: u8 = 0;
    const ASSET_MODE_SNAPSHOT: u8 = 1;

    const NOTIFY_RESIZED: u16 = 0x0100;
    const NOTIFY_FOCUSED: u16 = 0x0101;
    const NOTIFY_CLOSE_REQUESTED: u16 = 0x0102;
    const NOTIFY_LOG: u16 = 0x0103;
    const NOTIFY_CURSOR_POS: u16 = 0x0104;
    const NOTIFY_CURSOR_BUTTON: u16 = 0x0105;
    const NOTIFY_CURSOR_SCROLL: u16 = 0x0106;
    const NOTIFY_CURSOR_ENTERED: u16 = 0x0107;
    const NOTIFY_KEY: u16 = 0x0108;
    const NOTIFY_TEXT_COMMIT: u16 = 0x0109;
    const NOTIFY_ELEMENT_EVENT: u16 = 0x010A;
    const NOTIFY_TEXT_PREEDIT: u16 = 0x010B;
    const NOTIFY_TEXT_PREEDIT_CLEAR: u16 = 0x010C;
    const NOTIFY_RUNNING: u16 = 0x010D;

    const RUNNING_HEARTBEAT_INTERVAL: Duration = Duration::from_millis(500);

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

    const LOG_LEVEL_DEBUG: u8 = 0;
    const LOG_LEVEL_INFO: u8 = 1;

    const ACTION_RELEASE: u8 = 0;
    const ACTION_PRESS: u8 = 1;
    const MOD_SHIFT: u8 = 0x01;
    const MOD_CTRL: u8 = 0x02;
    const MOD_ALT: u8 = 0x04;
    const MOD_META: u8 = 0x08;
    const BUTTON_LEFT: u8 = 1;
    const BUTTON_RIGHT: u8 = 2;
    const BUTTON_MIDDLE: u8 = 3;
    const MACOS_BACKEND_AUTO: u8 = 0;
    const MACOS_BACKEND_METAL: u8 = 1;
    const MACOS_BACKEND_RASTER: u8 = 2;

    pub fn run() -> Result<(), String> {
        let config = HostConfig::from_env_args()?;
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| "macos_host must run on the macOS process main thread".to_string())?;

        let host_id = host_id();
        let state = Arc::new(HostState::new(host_id));
        let (command_tx, command_rx) = unbounded();
        let (listener_started_tx, listener_started_rx) = std::sync::mpsc::channel();

        let socket_path = config.socket_path.clone();
        let state_for_listener = Arc::clone(&state);
        let command_tx_for_listener = command_tx.clone();

        thread::Builder::new()
            .name("emerge_skia_macos_host_listener".to_string())
            .spawn(move || {
                listener_thread(
                    socket_path,
                    state_for_listener,
                    command_tx_for_listener,
                    listener_started_tx,
                )
            })
            .map_err(|err| format!("failed to spawn macOS host listener thread: {err}"))?;

        if config.monitor_stdin {
            let command_tx_for_stdin = command_tx.clone();
            thread::Builder::new()
                .name("emerge_skia_macos_host_stdin".to_string())
                .spawn(move || stdin_monitor_thread(command_tx_for_stdin))
                .map_err(|err| format!("failed to spawn macOS stdin monitor thread: {err}"))?;
        }

        match listener_started_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => {}
            Ok(Err(reason)) => return Err(reason),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                return Err("timed out starting macOS host listener".to_string());
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("macOS host listener exited before startup completed".to_string());
            }
        }

        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        app.finishLaunching();

        host_event_loop(app, mtm, config, state, command_rx);
        Ok(())
    }

    struct HostConfig {
        socket_path: PathBuf,
        monitor_stdin: bool,
    }

    impl HostConfig {
        fn from_env_args() -> Result<Self, String> {
            let mut socket_path = None;
            let mut monitor_stdin = false;
            let mut args = env::args().skip(1);

            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--socket" => {
                        let value = args
                            .next()
                            .ok_or_else(|| "--socket requires a path argument".to_string())?;
                        socket_path = Some(PathBuf::from(value));
                    }
                    "--monitor-stdin" => monitor_stdin = true,
                    other => return Err(format!("unknown macOS host argument: {other}")),
                }
            }

            let socket_path =
                socket_path.ok_or_else(|| "macos_host requires --socket <path>".to_string())?;

            Ok(Self {
                socket_path,
                monitor_stdin,
            })
        }
    }

    struct HostState {
        running: AtomicBool,
        next_session_id: AtomicU64,
        host_id: u64,
        outbound_tx: Mutex<Option<Sender<Vec<u8>>>>,
        running_sessions: Mutex<HashSet<u64>>,
        session_stats: Mutex<HashMap<u64, HostSessionStats>>,
    }

    struct HostSessionStats {
        backend_label: &'static str,
        collector: Arc<RendererStatsCollector>,
    }

    impl HostState {
        fn new(host_id: u64) -> Self {
            Self {
                running: AtomicBool::new(true),
                next_session_id: AtomicU64::new(1),
                host_id,
                outbound_tx: Mutex::new(None),
                running_sessions: Mutex::new(HashSet::new()),
                session_stats: Mutex::new(HashMap::new()),
            }
        }

        fn is_running(&self) -> bool {
            self.running.load(Ordering::Acquire)
        }

        fn stop(&self) {
            self.running.store(false, Ordering::Release);
        }

        fn next_session_id(&self) -> u64 {
            self.next_session_id.fetch_add(1, Ordering::Relaxed)
        }

        fn set_outbound(&self, outbound_tx: Sender<Vec<u8>>) {
            if let Ok(mut slot) = self.outbound_tx.lock() {
                *slot = Some(outbound_tx);
            }
        }

        fn clear_outbound(&self) {
            if let Ok(mut slot) = self.outbound_tx.lock() {
                *slot = None;
            }
        }

        fn send_frame(&self, frame: Vec<u8>) {
            if let Ok(slot) = self.outbound_tx.lock()
                && let Some(outbound_tx) = slot.as_ref()
            {
                let _ = outbound_tx.send(frame);
            }
        }

        fn register_session(
            &self,
            session_id: u64,
            backend_label: &'static str,
            stats: Option<Arc<RendererStatsCollector>>,
        ) {
            if let Ok(mut sessions) = self.running_sessions.lock() {
                sessions.insert(session_id);
            }

            if let Some(collector) = stats
                && let Ok(mut session_stats) = self.session_stats.lock()
            {
                session_stats.insert(
                    session_id,
                    HostSessionStats {
                        backend_label,
                        collector,
                    },
                );
            }
        }

        fn unregister_session(&self, session_id: u64) {
            if let Ok(mut sessions) = self.running_sessions.lock() {
                sessions.remove(&session_id);
            }

            if let Ok(mut session_stats) = self.session_stats.lock() {
                session_stats.remove(&session_id);
            }
        }

        fn running_session_ids(&self) -> Vec<u64> {
            self.running_sessions
                .lock()
                .map(|sessions| sessions.iter().copied().collect())
                .unwrap_or_default()
        }

        fn renderer_stats_logs(&self) -> Vec<(u64, String)> {
            self.session_stats
                .lock()
                .map(|stats| {
                    stats
                        .iter()
                        .map(|(session_id, session_stats)| {
                            (
                                *session_id,
                                format!(
                                    "session_id={} {}",
                                    session_id,
                                    format_renderer_stats_log(
                                        session_stats.backend_label,
                                        &session_stats.collector.snapshot(),
                                    )
                                ),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
    }

    enum HostCommand {
        StartSession {
            title: String,
            width: u32,
            height: u32,
            scroll_line_pixels: f32,
            renderer_stats_log: bool,
            renderer_cache_config: RendererCacheConfig,
            macos_backend: RequestedMacosBackend,
            asset_config: StartSessionAssetConfig,
            reply_tx: std::sync::mpsc::Sender<HostReply>,
        },
        StopSession {
            session_id: u64,
            reply_tx: std::sync::mpsc::Sender<HostReply>,
        },
        SessionRunning {
            session_id: u64,
            reply_tx: std::sync::mpsc::Sender<HostReply>,
        },
        UploadTree {
            session_id: u64,
            bytes: Vec<u8>,
        },
        PatchTree {
            session_id: u64,
            bytes: Vec<u8>,
        },
        SetInputMask {
            session_id: u64,
            mask: u32,
            reply_tx: std::sync::mpsc::Sender<HostReply>,
        },
        MeasureText {
            text: String,
            font_size: f32,
            reply_tx: std::sync::mpsc::Sender<HostReply>,
        },
        LoadFont {
            family: String,
            weight: u16,
            italic: bool,
            data: Vec<u8>,
            reply_tx: std::sync::mpsc::Sender<HostReply>,
        },
        ConfigureAssets {
            session_id: u64,
            asset_config: AssetConfig,
            reply_tx: std::sync::mpsc::Sender<HostReply>,
        },
        RenderTreeToPixels {
            bytes: Vec<u8>,
            opts: OffscreenRenderOptions,
            reply_tx: std::sync::mpsc::Sender<HostReply>,
        },
        RenderTreeToPng {
            bytes: Vec<u8>,
            opts: OffscreenRenderOptions,
            reply_tx: std::sync::mpsc::Sender<HostReply>,
        },
        Shutdown {
            reply_tx: Option<std::sync::mpsc::Sender<HostReply>>,
        },
    }

    enum HostReply {
        StartSession {
            session_id: u64,
            macos_backend: SelectedMacosBackend,
        },
        StopSession,
        SessionRunning {
            running: bool,
        },
        UploadTree,
        PatchTree,
        SetInputMask,
        MeasureText {
            width: f32,
            line_height: f32,
            ascent: f32,
            descent: f32,
        },
        LoadFont,
        ConfigureAssets,
        RenderTreeToPixels {
            data: Vec<u8>,
        },
        RenderTreeToPng {
            data: Vec<u8>,
        },
        Shutdown,
        Error(String),
    }

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    struct HostFontSpec {
        family: String,
        path: String,
        weight: u16,
        italic: bool,
    }

    #[derive(Clone, Debug, Default)]
    struct StartSessionAssetConfig {
        sources: Vec<String>,
        runtime_enabled: bool,
        runtime_allowlist: Vec<String>,
        runtime_follow_symlinks: bool,
        runtime_max_file_size: u64,
        runtime_extensions: Vec<String>,
        fonts: Vec<HostFontSpec>,
    }

    struct HostInputViewIvars {
        session_id: Cell<u64>,
        ui_state: RefCell<Weak<RefCell<HostUiState>>>,
    }

    impl Default for HostInputViewIvars {
        fn default() -> Self {
            Self {
                session_id: Cell::new(0),
                ui_state: RefCell::new(Weak::new()),
            }
        }
    }

    struct HostWindowDelegateIvars {
        session_id: Cell<u64>,
        ui_state: RefCell<Weak<RefCell<HostUiState>>>,
    }

    impl Default for HostWindowDelegateIvars {
        fn default() -> Self {
            Self {
                session_id: Cell::new(0),
                ui_state: RefCell::new(Weak::new()),
            }
        }
    }

    define_class!(
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[ivars = HostWindowDelegateIvars]
        struct HostWindowDelegate;

        unsafe impl NSObjectProtocol for HostWindowDelegate {}

        unsafe impl NSWindowDelegate for HostWindowDelegate {
            #[unsafe(method(windowDidResize:))]
            fn window_did_resize(&self, _notification: &NSNotification) {
                host_window_delegate_did_resize(self);
            }
        }
    );

    define_class!(
        #[unsafe(super(NSView))]
        #[thread_kind = MainThreadOnly]
        #[ivars = HostInputViewIvars]
        struct HostInputView;

        impl HostInputView {
            #[unsafe(method_id(initWithFrame:))]
            fn init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
                let this = this.set_ivars(HostInputViewIvars::default());
                unsafe { msg_send![super(this), initWithFrame: frame] }
            }

            #[unsafe(method(acceptsFirstResponder))]
            fn accepts_first_responder(&self) -> bool {
                true
            }

            #[unsafe(method(viewDidMoveToWindow))]
            fn view_did_move_to_window(&self) {
                unsafe {
                    let () = msg_send![super(self), viewDidMoveToWindow];
                }
                host_input_view_make_first_responder(self);
            }

            #[unsafe(method(keyDown:))]
            fn key_down(&self, event: &NSEvent) {
                host_input_view_key_down(self, event);
            }

            #[unsafe(method(keyUp:))]
            fn key_up(&self, event: &NSEvent) {
                host_input_view_key_up(self, event);
            }

            #[unsafe(method(flagsChanged:))]
            fn flags_changed(&self, event: &NSEvent) {
                host_input_view_flags_changed(self, event);
            }

            #[unsafe(method(selectAll:))]
            unsafe fn select_all(&self, sender: Option<&AnyObject>) {
                let _ = sender;
                host_input_view_handle_text_input_command(self, TextInputCommandRequest::SelectAll);
            }

            #[unsafe(method(copy:))]
            unsafe fn copy(&self, sender: Option<&AnyObject>) {
                let _ = sender;
                host_input_view_handle_text_input_command(self, TextInputCommandRequest::Copy);
            }

            #[unsafe(method(cut:))]
            unsafe fn cut(&self, sender: Option<&AnyObject>) {
                let _ = sender;
                host_input_view_handle_text_input_command(self, TextInputCommandRequest::Cut);
            }

            #[unsafe(method(paste:))]
            unsafe fn paste(&self, sender: Option<&AnyObject>) {
                let _ = sender;
                host_input_view_handle_text_input_command(self, TextInputCommandRequest::Paste);
            }
        }

        unsafe impl NSObjectProtocol for HostInputView {}

        unsafe impl NSTextInputClient for HostInputView {
            #[unsafe(method(insertText:replacementRange:))]
            unsafe fn insert_text_replacement_range(
                &self,
                string: &AnyObject,
                replacement_range: NSRange,
            ) {
                host_input_view_insert_text(self, string, replacement_range);
            }

            #[unsafe(method(doCommandBySelector:))]
            unsafe fn do_command_by_selector(&self, selector: Sel) {
                host_input_view_do_command(self, selector);
            }

            #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
            unsafe fn set_marked_text_selected_range_replacement_range(
                &self,
                string: &AnyObject,
                selected_range: NSRange,
                replacement_range: NSRange,
            ) {
                host_input_view_set_marked_text(
                    self,
                    string,
                    selected_range,
                    replacement_range,
                );
            }

            #[unsafe(method(unmarkText))]
            fn unmark_text(&self) {
                host_input_view_unmark_text(self);
            }

            #[unsafe(method(selectedRange))]
            fn selected_range(&self) -> NSRange {
                host_input_view_selected_range(self)
            }

            #[unsafe(method(markedRange))]
            fn marked_range(&self) -> NSRange {
                host_input_view_marked_range(self)
            }

            #[unsafe(method(hasMarkedText))]
            fn has_marked_text(&self) -> bool {
                host_input_view_has_marked_text(self)
            }

            #[unsafe(method_id(attributedSubstringForProposedRange:actualRange:))]
            unsafe fn attributed_substring_for_proposed_range_actual_range(
                &self,
                range: NSRange,
                actual_range: *mut NSRange,
            ) -> Option<Retained<NSAttributedString>> {
                host_input_view_attributed_substring(self, range, actual_range)
            }

            #[unsafe(method_id(validAttributesForMarkedText))]
            fn valid_attributes_for_marked_text(&self) -> Retained<NSArray<NSString>> {
                NSArray::from_slice(&[])
            }

            #[unsafe(method(firstRectForCharacterRange:actualRange:))]
            unsafe fn first_rect_for_character_range_actual_range(
                &self,
                range: NSRange,
                actual_range: *mut NSRange,
            ) -> NSRect {
                host_input_view_first_rect_for_character_range(self, range, actual_range)
            }

            #[unsafe(method(characterIndexForPoint:))]
            fn character_index_for_point(&self, point: NSPoint) -> usize {
                host_input_view_character_index_for_point(self, point)
            }
        }
    );

    struct MacosHostEventSink {
        state: Arc<HostState>,
        session_id: u64,
    }

    impl HostEventSink for MacosHostEventSink {
        fn send_raw_input(&self, event: &InputEvent) {
            notify_input_event(&self.state, self.session_id, event);
        }

        fn send_element_event(
            &self,
            element_id: &emerge_skia::tree::element::NodeId,
            kind: ElementEventKind,
            payload: Option<&str>,
        ) {
            notify_element_event(&self.state, self.session_id, element_id, kind, payload);
        }
    }

    struct HostSession {
        window: Retained<NSWindow>,
        content_view: Retained<NSView>,
        _input_view: Retained<HostInputView>,
        _window_delegate: Retained<HostWindowDelegate>,
        surface: SessionSurface,
        renderer: SceneRenderer,
        tree: ElementTree,
        render_state: RenderState,
        logical_size: (u32, u32),
        scale_factor: f32,
        scroll_line_pixels: f32,
        dirty: bool,
        focused: bool,
        initial_notifications_sent: bool,
        initial_log_sent: bool,
        selected_backend: SelectedMacosBackend,
        _tracking_area: Retained<NSTrackingArea>,
        cursor_inside: bool,
        stats: Option<Arc<RendererStatsCollector>>,
        event_runtime: HostEventRuntime,
        animation_runtime: AnimationRuntime,
        latest_animation_sample_time: Option<std::time::Instant>,
        present: SessionPresentState,
    }

    #[derive(Default)]
    struct HostUiState {
        sessions: HashMap<u64, HostSession>,
        asset_config: AssetConfig,
        loaded_fonts: HashSet<HostFontSpec>,
    }

    fn configure_host_assets_for_start(
        ui_state: &mut HostUiState,
        asset_config: &StartSessionAssetConfig,
    ) -> Result<bool, String> {
        let config_changed = replace_asset_config(
            &mut ui_state.asset_config,
            AssetConfig {
                sources: asset_config.sources.clone(),
                runtime_enabled: asset_config.runtime_enabled,
                runtime_allowlist: asset_config.runtime_allowlist.clone(),
                runtime_follow_symlinks: asset_config.runtime_follow_symlinks,
                runtime_max_file_size: asset_config.runtime_max_file_size,
                runtime_extensions: asset_config.runtime_extensions.clone(),
            },
        );
        if config_changed {
            assets::configure(ui_state.asset_config.clone());
        }

        let fonts_changed = load_host_fonts(&mut ui_state.loaded_fonts, &asset_config.fonts)?;
        Ok(config_changed || fonts_changed)
    }

    fn configure_host_assets(ui_state: &mut HostUiState, asset_config: AssetConfig) -> bool {
        let changed = replace_asset_config(&mut ui_state.asset_config, asset_config);
        if changed {
            assets::configure(ui_state.asset_config.clone());
        }
        changed
    }

    fn replace_asset_config(existing: &mut AssetConfig, incoming: AssetConfig) -> bool {
        let changed = existing.sources != incoming.sources
            || existing.runtime_enabled != incoming.runtime_enabled
            || existing.runtime_allowlist != incoming.runtime_allowlist
            || existing.runtime_follow_symlinks != incoming.runtime_follow_symlinks
            || existing.runtime_max_file_size != incoming.runtime_max_file_size
            || existing.runtime_extensions != incoming.runtime_extensions;

        if changed {
            *existing = incoming;
        }

        changed
    }

    fn load_host_fonts(
        loaded_fonts: &mut HashSet<HostFontSpec>,
        fonts: &[HostFontSpec],
    ) -> Result<bool, String> {
        let mut changed = false;

        for font in fonts {
            if loaded_fonts.contains(font) {
                continue;
            }

            let data = fs::read(&font.path).map_err(|err| {
                format!(
                    "failed to read macOS font asset family={} path={}: {err}",
                    font.family, font.path
                )
            })?;

            services::load_font_bytes(&font.family, font.weight, font.italic, &data).map_err(
                |err| {
                    format!(
                        "failed to load macOS font asset family={} path={}: {err}",
                        font.family, font.path
                    )
                },
            )?;

            loaded_fonts.insert(font.clone());
            changed = true;
        }

        Ok(changed)
    }

    #[derive(Clone, Copy)]
    enum RequestedMacosBackend {
        Auto,
        Metal,
        Raster,
    }

    #[derive(Clone, Copy)]
    enum SelectedMacosBackend {
        Metal,
        Raster,
    }

    enum SessionSurface {
        Metal(MetalSurface),
        Raster(RasterLayerSurface),
    }

    struct MetalSurface {
        metal_layer: Retained<CAMetalLayer>,
        command_queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
        skia: gpu::DirectContext,
    }

    struct RasterLayerSurface {
        image_view: Retained<NSImageView>,
        surface: Surface,
        pixel_size: (u32, u32),
    }

    struct SessionPresentState {
        last_present_at: Option<std::time::Instant>,
        estimated_frame_interval: Duration,
        next_pulse_at: Option<std::time::Instant>,
    }

    impl SessionPresentState {
        fn new(initial_frame_interval: Duration) -> Self {
            Self {
                last_present_at: None,
                estimated_frame_interval: initial_frame_interval,
                next_pulse_at: None,
            }
        }

        fn observe_present(&mut self, presented_at: std::time::Instant) -> std::time::Instant {
            if let Some(last_present_at) = self.last_present_at {
                let observed = presented_at.saturating_duration_since(last_present_at);

                if observed >= Duration::from_millis(4) && observed <= Duration::from_millis(100) {
                    self.estimated_frame_interval = observed;
                }
            }

            self.last_present_at = Some(presented_at);
            let predicted_next = presented_at + self.estimated_frame_interval;
            self.next_pulse_at = Some(predicted_next);
            predicted_next
        }

        fn clear(&mut self) {
            self.next_pulse_at = None;
        }
    }

    impl HostInputView {
        fn new(
            mtm: MainThreadMarker,
            frame: NSRect,
            ui_state: &Rc<RefCell<HostUiState>>,
            session_id: u64,
        ) -> Retained<Self> {
            let view: Retained<Self> = unsafe { msg_send![Self::alloc(mtm), initWithFrame: frame] };
            view.ivars().session_id.set(session_id);
            *view.ivars().ui_state.borrow_mut() = Rc::downgrade(ui_state);
            view.as_super().setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            view
        }

        fn with_session_mut<R>(&self, f: impl FnOnce(&mut HostSession) -> R) -> Option<R> {
            let session_id = self.ivars().session_id.get();
            let ui_state = self.ivars().ui_state.borrow().upgrade()?;
            let mut ui_state = ui_state.borrow_mut();
            let session = ui_state.sessions.get_mut(&session_id)?;
            Some(f(session))
        }

        fn focused_text_state(&self) -> Option<TextInputState> {
            let session_id = self.ivars().session_id.get();
            let ui_state = self.ivars().ui_state.borrow().upgrade()?;
            let ui_state = ui_state.borrow();
            let session = ui_state.sessions.get(&session_id)?;
            session.event_runtime.focused_text_state()
        }
    }

    impl HostWindowDelegate {
        fn new(
            mtm: MainThreadMarker,
            ui_state: &Rc<RefCell<HostUiState>>,
            session_id: u64,
        ) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(HostWindowDelegateIvars::default());
            let delegate: Retained<Self> = unsafe { msg_send![super(this), init] };
            delegate.ivars().session_id.set(session_id);
            *delegate.ivars().ui_state.borrow_mut() = Rc::downgrade(ui_state);
            delegate
        }

        fn with_session_mut<R>(&self, f: impl FnOnce(&mut HostSession) -> R) -> Option<R> {
            let session_id = self.ivars().session_id.get();
            let ui_state = self.ivars().ui_state.borrow().upgrade()?;
            let mut ui_state = ui_state.borrow_mut();
            let session = ui_state.sessions.get_mut(&session_id)?;
            Some(f(session))
        }
    }

    fn host_window_delegate_did_resize(delegate: &HostWindowDelegate) {
        let _ = delegate.with_session_mut(|session| {
            if let Err(err) = sync_session_size(session, true) {
                eprintln!("macOS live resize sync failed: {err}");
            }
        });
    }

    fn host_input_view_make_first_responder(view: &HostInputView) {
        if let Some(window) = view.as_super().window() {
            let _ = window.makeFirstResponder(Some(view.as_super().as_super()));
        }
    }

    fn host_input_view_key_down(view: &HostInputView, event: &NSEvent) {
        let mods = modifier_bits(event.modifierFlags());
        let key = canonical_key_for_event(event);
        let _ = view.with_session_mut(|session| {
            let _ = handle_runtime_input(
                session,
                InputEvent::Key {
                    key,
                    action: ACTION_PRESS,
                    mods,
                },
            );
        });

        if should_interpret_key_event(key, mods) {
            let events = NSArray::from_slice(&[event]);
            view.as_super().as_super().interpretKeyEvents(&events);
        }
    }

    fn host_input_view_key_up(view: &HostInputView, event: &NSEvent) {
        let mods = modifier_bits(event.modifierFlags());
        let key = canonical_key_for_event(event);
        let _ = view.with_session_mut(|session| {
            let _ = handle_runtime_input(
                session,
                InputEvent::Key {
                    key,
                    action: ACTION_RELEASE,
                    mods,
                },
            );
        });
    }

    fn host_input_view_flags_changed(view: &HostInputView, event: &NSEvent) {
        let mods = modifier_bits(event.modifierFlags());
        let key = canonical_key_for_event(event);
        let action = modifier_action(event, mods, key);
        let _ = view.with_session_mut(|session| {
            let _ = handle_runtime_input(session, InputEvent::Key { key, action, mods });
        });
    }

    fn host_input_view_insert_text(
        view: &HostInputView,
        string: &AnyObject,
        replacement_range: NSRange,
    ) {
        let Some(text) = text_from_input_object(string) else {
            return;
        };

        let replacement_range = view.focused_text_state().and_then(|state| {
            state
                .appkit_replacement_char_range(replacement_range.location, replacement_range.length)
        });

        let _ = view.with_session_mut(|session| {
            session
                .event_runtime
                .prepare_text_input_replacement_range(replacement_range);
            let _ = handle_runtime_input(session, InputEvent::TextCommit { text, mods: 0 });
        });
    }

    fn host_input_view_do_command(view: &HostInputView, selector: Sel) {
        let Ok(name) = selector.name().to_str() else {
            return;
        };

        match text_input_selector_action(name) {
            Some(TextInputSelectorAction::Command(request)) => {
                host_input_view_handle_text_input_command(view, request)
            }
            Some(TextInputSelectorAction::Edit(request)) => {
                host_input_view_handle_text_input_edit(view, request)
            }
            None if should_ignore_text_input_selector(name) => {}
            None => {}
        }
    }

    fn host_input_view_handle_text_input_command(
        view: &HostInputView,
        request: TextInputCommandRequest,
    ) {
        let _ = view.with_session_mut(|session| {
            let _ = handle_runtime_text_input_command(session, request);
        });
    }

    fn host_input_view_handle_text_input_edit(view: &HostInputView, request: TextInputEditRequest) {
        let _ = view.with_session_mut(|session| {
            let _ = handle_runtime_text_input_edit(session, request);
        });
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) enum TextInputSelectorAction {
        Command(TextInputCommandRequest),
        Edit(TextInputEditRequest),
    }

    pub(crate) fn text_input_selector_action(name: &str) -> Option<TextInputSelectorAction> {
        match name {
            "selectAll:" => Some(TextInputSelectorAction::Command(
                TextInputCommandRequest::SelectAll,
            )),
            "copy:" => Some(TextInputSelectorAction::Command(
                TextInputCommandRequest::Copy,
            )),
            "cut:" => Some(TextInputSelectorAction::Command(
                TextInputCommandRequest::Cut,
            )),
            "paste:" => Some(TextInputSelectorAction::Command(
                TextInputCommandRequest::Paste,
            )),
            "moveWordForward:" | "moveWordRight:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::MoveWordRight {
                    extend_selection: false,
                },
            )),
            "moveWordBackward:" | "moveWordLeft:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::MoveWordLeft {
                    extend_selection: false,
                },
            )),
            "moveWordForwardAndModifySelection:" | "moveWordRightAndModifySelection:" => Some(
                TextInputSelectorAction::Edit(TextInputEditRequest::MoveWordRight {
                    extend_selection: true,
                }),
            ),
            "moveWordBackwardAndModifySelection:" | "moveWordLeftAndModifySelection:" => Some(
                TextInputSelectorAction::Edit(TextInputEditRequest::MoveWordLeft {
                    extend_selection: true,
                }),
            ),
            "moveToBeginningOfLine:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::MoveHome {
                    extend_selection: false,
                },
            )),
            "moveToEndOfLine:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::MoveEnd {
                    extend_selection: false,
                },
            )),
            "moveToBeginningOfLineAndModifySelection:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::MoveHome {
                    extend_selection: true,
                },
            )),
            "moveToEndOfLineAndModifySelection:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::MoveEnd {
                    extend_selection: true,
                },
            )),
            "moveToBeginningOfParagraph:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::MoveParagraphStart {
                    extend_selection: false,
                },
            )),
            "moveToEndOfParagraph:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::MoveParagraphEnd {
                    extend_selection: false,
                },
            )),
            "moveToBeginningOfParagraphAndModifySelection:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::MoveParagraphStart {
                    extend_selection: true,
                },
            )),
            "moveToEndOfParagraphAndModifySelection:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::MoveParagraphEnd {
                    extend_selection: true,
                },
            )),
            "moveToBeginningOfDocument:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::MoveDocumentStart {
                    extend_selection: false,
                },
            )),
            "moveToEndOfDocument:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::MoveDocumentEnd {
                    extend_selection: false,
                },
            )),
            "moveToBeginningOfDocumentAndModifySelection:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::MoveDocumentStart {
                    extend_selection: true,
                },
            )),
            "moveToEndOfDocumentAndModifySelection:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::MoveDocumentEnd {
                    extend_selection: true,
                },
            )),
            "deleteWordBackward:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::DeleteWordBackward,
            )),
            "deleteWordForward:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::DeleteWordForward,
            )),
            "deleteToBeginningOfLine:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::DeleteToHome,
            )),
            "deleteToEndOfLine:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::DeleteToEnd,
            )),
            "deleteToBeginningOfParagraph:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::DeleteToParagraphStart,
            )),
            "deleteToEndOfParagraph:" => Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::DeleteToParagraphEnd,
            )),
            _ => None,
        }
    }

    pub(crate) fn should_interpret_key_event(key: CanonicalKey, mods: u8) -> bool {
        if mods & MOD_CTRL != 0 {
            return false;
        }

        if mods & MOD_META != 0 && is_direct_command_shortcut_key(key) {
            return false;
        }

        true
    }

    fn is_direct_command_shortcut_key(key: CanonicalKey) -> bool {
        matches!(
            key,
            CanonicalKey::A | CanonicalKey::C | CanonicalKey::V | CanonicalKey::X
        )
    }

    pub(crate) fn should_ignore_text_input_selector(name: &str) -> bool {
        matches!(
            name,
            "moveLeft:"
                | "moveRight:"
                | "moveUp:"
                | "moveDown:"
                | "moveToBeginningOfLine:"
                | "moveToEndOfLine:"
                | "moveBackwardAndModifySelection:"
                | "moveForwardAndModifySelection:"
                | "moveUpAndModifySelection:"
                | "moveDownAndModifySelection:"
                | "moveToBeginningOfLineAndModifySelection:"
                | "moveToEndOfLineAndModifySelection:"
                | "deleteBackward:"
                | "deleteForward:"
                | "deleteBackwardByDecomposingPreviousCharacter:"
                | "insertNewline:"
                | "insertTab:"
                | "insertBacktab:"
        )
    }

    fn host_input_view_set_marked_text(
        view: &HostInputView,
        string: &AnyObject,
        selected_range: NSRange,
        replacement_range: NSRange,
    ) {
        let Some(text) = text_from_input_object(string) else {
            return;
        };
        let cursor = utf16_nsrange_to_char_range(&text, selected_range);
        let replacement_range = view.focused_text_state().and_then(|state| {
            state
                .appkit_replacement_char_range(replacement_range.location, replacement_range.length)
        });

        let _ = view.with_session_mut(|session| {
            session
                .event_runtime
                .prepare_text_input_replacement_range(replacement_range);
            let event = if text.is_empty() {
                InputEvent::TextPreeditClear
            } else {
                InputEvent::TextPreedit { text, cursor }
            };
            let _ = handle_runtime_input(session, event);
        });
    }

    fn host_input_view_unmark_text(view: &HostInputView) {
        let _ = view.with_session_mut(|session| {
            let _ = handle_runtime_input(session, InputEvent::TextPreeditClear);
        });
    }

    fn host_input_view_selected_range(view: &HostInputView) -> NSRange {
        view.focused_text_state()
            .map(|state| {
                let (location, length) = state.appkit_selected_range_utf16();
                NSRange::new(location, length)
            })
            .unwrap_or_else(invalid_text_range)
    }

    fn host_input_view_marked_range(view: &HostInputView) -> NSRange {
        view.focused_text_state()
            .and_then(|state| state.appkit_marked_range_utf16())
            .map(|(location, length)| NSRange::new(location, length))
            .unwrap_or_else(invalid_text_range)
    }

    fn host_input_view_has_marked_text(view: &HostInputView) -> bool {
        view.focused_text_state()
            .and_then(|state| state.appkit_marked_range_utf16())
            .is_some()
    }

    fn host_input_view_attributed_substring(
        view: &HostInputView,
        range: NSRange,
        actual_range: *mut NSRange,
    ) -> Option<Retained<NSAttributedString>> {
        let Some(state) = view.focused_text_state() else {
            unsafe {
                if !actual_range.is_null() {
                    *actual_range = invalid_text_range();
                }
            }
            return None;
        };

        let displayed = state.appkit_displayed_text();
        let total_len = displayed.encode_utf16().count();
        if range.location > total_len {
            unsafe {
                if !actual_range.is_null() {
                    *actual_range = invalid_text_range();
                }
            }
            return None;
        }

        let actual = NSRange::new(
            range.location,
            range.length.min(total_len.saturating_sub(range.location)),
        );
        unsafe {
            if !actual_range.is_null() {
                *actual_range = actual;
            }
        }

        let substring = state
            .appkit_substring_for_utf16_range(actual.location, actual.length)
            .unwrap_or_default();
        Some(NSAttributedString::from_nsstring(&NSString::from_str(
            &substring,
        )))
    }

    fn host_input_view_first_rect_for_character_range(
        view: &HostInputView,
        range: NSRange,
        actual_range: *mut NSRange,
    ) -> NSRect {
        unsafe {
            if !actual_range.is_null() {
                *actual_range = range;
            }
        }

        let Some(state) = view.focused_text_state() else {
            return zero_rect();
        };

        let Some((x, y, width, height)) =
            state.appkit_first_rect_for_utf16_range(range.location, range.length)
        else {
            return zero_rect();
        };
        let super_view = view.as_super();
        let scale_factor = super_view
            .window()
            .map(|window| window.backingScaleFactor() as f32)
            .unwrap_or(1.0);
        let (x, y, width, height) = view_rect_for_render_rect((x, y, width, height), scale_factor);

        let bounds = super_view.bounds();
        let cocoa_y = if super_view.isFlipped() {
            y as f64
        } else {
            bounds.size.height - (y + height) as f64
        };
        let local_rect = NSRect::new(
            NSPoint::new(x as f64, cocoa_y),
            NSSize::new(width as f64, height as f64),
        );
        let window_rect = super_view.convertRect_toView(local_rect, None);

        super_view
            .window()
            .map(|window| window.convertRectToScreen(window_rect))
            .unwrap_or(window_rect)
    }

    fn host_input_view_character_index_for_point(view: &HostInputView, point: NSPoint) -> usize {
        let Some(state) = view.focused_text_state() else {
            return 0;
        };
        let super_view = view.as_super();
        let Some(window) = super_view.window() else {
            return 0;
        };

        let window_point = window.convertPointFromScreen(point);
        let view_point = super_view.convertPoint_fromView(window_point, None);
        let bounds = super_view.bounds();
        let scale_factor = window.backingScaleFactor() as f32;
        let y = if super_view.isFlipped() {
            view_point.y as f32
        } else {
            (bounds.size.height - view_point.y) as f32
        };

        let (x, y) = render_point_for_view_point((view_point.x as f32, y), scale_factor);
        state.appkit_character_index_for_point_utf16(x, y)
    }

    fn text_from_input_object(object: &AnyObject) -> Option<String> {
        if let Some(string) = object.downcast_ref::<NSString>() {
            Some(string.to_string())
        } else {
            object
                .downcast_ref::<NSAttributedString>()
                .map(|string| string.string().to_string())
        }
    }

    fn invalid_text_range() -> NSRange {
        NSRange::new(usize::MAX, 0)
    }

    fn zero_rect() -> NSRect {
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0))
    }

    fn utf16_nsrange_to_char_range(text: &str, range: NSRange) -> Option<(u32, u32)> {
        if range.location == usize::MAX {
            return None;
        }

        let start = utf16_offset_to_char_index(text, range.location);
        let end = utf16_offset_to_char_index(text, range.location.saturating_add(range.length));
        Some((start, end))
    }

    fn utf16_offset_to_char_index(text: &str, utf16_offset: usize) -> u32 {
        let mut utf16_count = 0;
        let mut char_count = 0;

        for ch in text.chars() {
            let next = utf16_count + ch.len_utf16();
            if next > utf16_offset {
                break;
            }

            utf16_count = next;
            char_count += 1;
        }

        char_count
    }

    fn host_event_loop(
        app: Retained<NSApplication>,
        mtm: MainThreadMarker,
        config: HostConfig,
        state: Arc<HostState>,
        command_rx: Receiver<HostCommand>,
    ) {
        let distant_past = NSDate::distantPast();
        let ui_state = Rc::new(RefCell::new(HostUiState::default()));
        let (asset_tx, asset_rx) = bounded(256);
        assets::start(asset_tx, false);

        while state.is_running() {
            drain_commands(&app, mtm, &state, &command_rx, &ui_state);

            autoreleasepool(|_| {
                drain_pending_events_for_mode(&app, &ui_state, &distant_past, unsafe {
                    NSDefaultRunLoopMode
                });
                drain_pending_events_for_mode(&app, &ui_state, &distant_past, unsafe {
                    NSEventTrackingRunLoopMode
                });
            });

            app.updateWindows();
            sync_session_sizes(&state, &ui_state);
            tick_session_runtimes(&ui_state);
            tick_session_animations(&ui_state);
            handle_asset_updates(&ui_state, &asset_rx);
            draw_dirty_sessions(&ui_state);
            reap_closed_sessions(&state, &ui_state);
            thread::sleep(Duration::from_millis(10));
        }

        ui_state
            .borrow_mut()
            .sessions
            .drain()
            .for_each(|(_, session)| {
                session.window.close();
            });

        assets::stop();

        let _ = fs::remove_file(config.socket_path);
    }

    fn drain_pending_events_for_mode(
        app: &NSApplication,
        ui_state: &Rc<RefCell<HostUiState>>,
        distant_past: &NSDate,
        mode: &objc2_foundation::NSRunLoopMode,
    ) {
        while let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(
            NSEventMask::Any,
            Some(distant_past),
            mode,
            true,
        ) {
            dispatch_pointer_event(ui_state, &event);
            app.sendEvent(&event);
        }
    }

    fn drain_commands(
        app: &NSApplication,
        mtm: MainThreadMarker,
        state: &Arc<HostState>,
        command_rx: &Receiver<HostCommand>,
        ui_state: &Rc<RefCell<HostUiState>>,
    ) {
        loop {
            match command_rx.try_recv() {
                Ok(HostCommand::StartSession {
                    title,
                    width,
                    height,
                    scroll_line_pixels,
                    renderer_stats_log,
                    renderer_cache_config,
                    macos_backend,
                    asset_config,
                    reply_tx,
                }) => {
                    let session_id = state.next_session_id();

                    let assets_changed = {
                        let mut ui = ui_state.borrow_mut();
                        match configure_host_assets_for_start(&mut ui, &asset_config) {
                            Ok(changed) => changed,
                            Err(reason) => {
                                let _ = reply_tx.send(HostReply::Error(reason));
                                continue;
                            }
                        }
                    };

                    match create_session(CreateSessionRequest {
                        app,
                        mtm,
                        state,
                        session_id,
                        title: &title,
                        width,
                        height,
                        scroll_line_pixels,
                        renderer_stats_log,
                        renderer_cache_config,
                        requested_backend: macos_backend,
                        ui_state,
                    }) {
                        Ok((session, selected_backend)) => {
                            let stats = session.stats.clone();
                            ui_state.borrow_mut().sessions.insert(session_id, session);
                            state.register_session(
                                session_id,
                                selected_backend_stats_label(selected_backend),
                                stats,
                            );

                            if assets_changed {
                                ui_state.borrow_mut().sessions.values_mut().for_each(|session| {
                                    if let Err(err) = rerender_session(session) {
                                        eprintln!("macOS session rerender after asset update failed: {err}");
                                    }
                                });
                            }

                            let _ = reply_tx.send(HostReply::StartSession {
                                session_id,
                                macos_backend: selected_backend,
                            });
                        }
                        Err(reason) => {
                            let _ = reply_tx.send(HostReply::Error(reason));
                        }
                    }
                }
                Ok(HostCommand::StopSession {
                    session_id,
                    reply_tx,
                }) => {
                    if let Some(session) = ui_state.borrow_mut().sessions.remove(&session_id) {
                        session.window.close();
                    }
                    state.unregister_session(session_id);

                    let _ = reply_tx.send(HostReply::StopSession);
                }
                Ok(HostCommand::SessionRunning {
                    session_id,
                    reply_tx,
                }) => {
                    let running = ui_state.borrow().sessions.contains_key(&session_id);
                    let _ = reply_tx.send(HostReply::SessionRunning { running });
                }
                Ok(HostCommand::UploadTree { session_id, bytes }) => {
                    match ui_state.borrow_mut().sessions.get_mut(&session_id) {
                        Some(session) => {
                            if let Err(err) = upload_tree(session, &bytes) {
                                eprintln!(
                                    "macOS upload_tree failed for session {session_id}: {err}"
                                );
                            } else {
                                notify_log(
                                    state,
                                    session_id,
                                    LOG_LEVEL_DEBUG,
                                    "macos_host",
                                    "upload_tree applied",
                                );
                            }
                        }
                        None => {
                            eprintln!("macOS upload_tree for unknown session {session_id}");
                        }
                    }
                }
                Ok(HostCommand::PatchTree { session_id, bytes }) => {
                    match ui_state.borrow_mut().sessions.get_mut(&session_id) {
                        Some(session) => {
                            let patch_started_at = Instant::now();
                            let result = patch_tree(session, &bytes);

                            if let Some(stats) = session.stats.as_ref() {
                                stats.record_patch_tree_process(patch_started_at.elapsed());
                            }

                            if let Err(err) = result {
                                eprintln!(
                                    "macOS patch_tree failed for session {session_id}: {err}"
                                );
                            } else {
                                notify_log(
                                    state,
                                    session_id,
                                    LOG_LEVEL_DEBUG,
                                    "macos_host",
                                    "patch_tree applied",
                                );
                            }
                        }
                        None => {
                            eprintln!("macOS patch_tree for unknown session {session_id}");
                        }
                    }
                }
                Ok(HostCommand::SetInputMask {
                    session_id,
                    mask,
                    reply_tx,
                }) => {
                    let reply = match ui_state.borrow_mut().sessions.get_mut(&session_id) {
                        Some(session) => {
                            session.event_runtime.set_input_mask(mask);
                            HostReply::SetInputMask
                        }
                        None => HostReply::Error(format!("unknown session_id {session_id}")),
                    };

                    let _ = reply_tx.send(reply);
                }
                Ok(HostCommand::MeasureText {
                    text,
                    font_size,
                    reply_tx,
                }) => {
                    let (width, line_height, ascent, descent) =
                        services::measure_text(&text, font_size);
                    let _ = reply_tx.send(HostReply::MeasureText {
                        width,
                        line_height,
                        ascent,
                        descent,
                    });
                }
                Ok(HostCommand::LoadFont {
                    family,
                    weight,
                    italic,
                    data,
                    reply_tx,
                }) => {
                    let reply = match services::load_font_bytes(&family, weight, italic, &data) {
                        Ok(()) => HostReply::LoadFont,
                        Err(reason) => HostReply::Error(reason),
                    };

                    let _ = reply_tx.send(reply);
                }
                Ok(HostCommand::ConfigureAssets {
                    session_id,
                    asset_config,
                    reply_tx,
                }) => {
                    let reply = if ui_state.borrow().sessions.contains_key(&session_id) {
                        let assets_changed = {
                            let mut ui = ui_state.borrow_mut();
                            configure_host_assets(&mut ui, asset_config)
                        };

                        if assets_changed {
                            ui_state.borrow_mut().sessions.values_mut().for_each(|session| {
                                if let Err(err) = rerender_session(session) {
                                    eprintln!("macOS session rerender after asset update failed: {err}");
                                }
                            });
                        }

                        HostReply::ConfigureAssets
                    } else {
                        HostReply::Error(format!("unknown session_id {session_id}"))
                    };

                    let _ = reply_tx.send(reply);
                }
                Ok(HostCommand::RenderTreeToPixels {
                    bytes,
                    opts,
                    reply_tx,
                }) => {
                    let reply = match services::render_tree_to_pixels(&bytes, opts) {
                        Ok(data) => HostReply::RenderTreeToPixels { data },
                        Err(reason) => HostReply::Error(reason),
                    };

                    let _ = reply_tx.send(reply);
                }
                Ok(HostCommand::RenderTreeToPng {
                    bytes,
                    opts,
                    reply_tx,
                }) => {
                    let reply = match services::render_tree_to_png(&bytes, opts) {
                        Ok(data) => HostReply::RenderTreeToPng { data },
                        Err(reason) => HostReply::Error(reason),
                    };

                    let _ = reply_tx.send(reply);
                }
                Ok(HostCommand::Shutdown { reply_tx }) => {
                    state.stop();

                    if let Some(reply_tx) = reply_tx {
                        let _ = reply_tx.send(HostReply::Shutdown);
                    }

                    break;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    state.stop();
                    break;
                }
            }
        }
    }

    fn reap_closed_sessions(state: &Arc<HostState>, ui_state: &Rc<RefCell<HostUiState>>) {
        let mut ui_state = ui_state.borrow_mut();
        let closed_ids = ui_state
            .sessions
            .iter()
            .filter_map(|(id, session)| (!session.window.isVisible()).then_some(*id))
            .collect::<Vec<_>>();

        closed_ids.into_iter().for_each(|id| {
            notify_close_requested(state, id);
            ui_state.sessions.remove(&id);
            state.unregister_session(id);
        });
    }

    fn sync_session_size(session: &mut HostSession, draw_now: bool) -> Result<bool, String> {
        let metrics = session_metrics(&session.window, &session.content_view);

        if metrics.render_size == session.logical_size
            && (metrics.scale_factor - session.scale_factor).abs() <= f32::EPSILON
        {
            return Ok(false);
        }

        session.logical_size = metrics.render_size;
        session.scale_factor = metrics.scale_factor;
        resize_surface(session, &metrics);
        rerender_session(session)?;
        let _ = handle_runtime_input(
            session,
            InputEvent::Resized {
                width: session.logical_size.0,
                height: session.logical_size.1,
                scale_factor: session.scale_factor,
            },
        );

        if draw_now {
            draw_session(session)?;
        }

        Ok(true)
    }

    fn sync_session_sizes(state: &Arc<HostState>, ui_state: &Rc<RefCell<HostUiState>>) {
        ui_state
            .borrow_mut()
            .sessions
            .iter_mut()
            .for_each(|(session_id, session)| {
                if !session.initial_log_sent {
                    notify_log(
                        state,
                        *session_id,
                        LOG_LEVEL_INFO,
                        "macos_host",
                        &format!(
                            "session using macOS backend {}",
                            selected_backend_name(session.selected_backend)
                        ),
                    );
                    session.initial_log_sent = true;
                }

                if !session.initial_notifications_sent {
                    let _ = handle_runtime_input(
                        session,
                        InputEvent::Resized {
                            width: session.logical_size.0,
                            height: session.logical_size.1,
                            scale_factor: session.scale_factor,
                        },
                    );
                    let _ = handle_runtime_input(
                        session,
                        InputEvent::Focused {
                            focused: session.focused,
                        },
                    );
                    session.initial_notifications_sent = true;
                }

                if let Err(err) = sync_session_size(session, false) {
                    eprintln!("macOS session resize sync failed: {err}");
                }

                let focused = session.window.isKeyWindow();

                if focused != session.focused {
                    session.focused = focused;
                    let _ = handle_runtime_input(session, InputEvent::Focused { focused });
                }
            });
    }

    fn draw_dirty_sessions(ui_state: &Rc<RefCell<HostUiState>>) {
        ui_state
            .borrow_mut()
            .sessions
            .values_mut()
            .for_each(|session| {
                if session.dirty
                    && let Err(err) = draw_session(session)
                {
                    eprintln!("macOS session draw failed: {err}");
                }
            });
    }

    fn handle_asset_updates(
        ui_state: &Rc<RefCell<HostUiState>>,
        asset_rx: &Receiver<emerge_skia::actors::TreeMsg>,
    ) {
        let mut saw_update = false;

        while let Ok(message) = asset_rx.try_recv() {
            if matches!(message, emerge_skia::actors::TreeMsg::AssetStateChanged) {
                saw_update = true;
            }
        }

        if !saw_update {
            return;
        }

        ui_state
            .borrow_mut()
            .sessions
            .values_mut()
            .for_each(|session| {
                if let Err(err) = rerender_session(session) {
                    eprintln!("macOS asset rerender failed: {err}");
                }
            });
    }

    fn tick_session_runtimes(ui_state: &Rc<RefCell<HostUiState>>) {
        ui_state
            .borrow_mut()
            .sessions
            .values_mut()
            .for_each(|session| {
                session.event_runtime.handle_timers();

                let runtime_messages = session.event_runtime.drain_tree_messages();
                if runtime_messages.is_empty() {
                    return;
                }

                if let Err(err) = apply_tree_messages(session, runtime_messages)
                    .and_then(|invalidation| render_session_for_invalidation(session, invalidation))
                {
                    eprintln!("macOS runtime tick render failed: {err}");
                }
            });
    }

    fn tick_session_animations(ui_state: &Rc<RefCell<HostUiState>>) {
        let now = std::time::Instant::now();

        ui_state
            .borrow_mut()
            .sessions
            .values_mut()
            .for_each(|session| {
                let Some(predicted_next_present_at) = session.present.next_pulse_at else {
                    return;
                };

                if now < predicted_next_present_at {
                    return;
                }

                let Some(presented_at) = session.present.last_present_at else {
                    return;
                };

                if let Err(err) = apply_tree_messages(
                    session,
                    vec![emerge_skia::actors::TreeMsg::AnimationPulse {
                        presented_at,
                        predicted_next_present_at,
                        trace: None,
                    }],
                )
                .and_then(|invalidation| render_session_for_invalidation(session, invalidation))
                {
                    eprintln!("macOS animation tick render failed: {err}");
                }
            });
    }

    struct CreateSessionRequest<'a> {
        app: &'a NSApplication,
        mtm: MainThreadMarker,
        state: &'a Arc<HostState>,
        session_id: u64,
        title: &'a str,
        width: u32,
        height: u32,
        scroll_line_pixels: f32,
        renderer_stats_log: bool,
        renderer_cache_config: RendererCacheConfig,
        requested_backend: RequestedMacosBackend,
        ui_state: &'a Rc<RefCell<HostUiState>>,
    }

    fn create_session(
        request: CreateSessionRequest<'_>,
    ) -> Result<(HostSession, SelectedMacosBackend), String> {
        let CreateSessionRequest {
            app,
            mtm,
            state,
            session_id,
            title,
            width,
            height,
            scroll_line_pixels,
            renderer_stats_log,
            renderer_cache_config,
            requested_backend,
            ui_state,
        } = request;

        let window = create_window(app, mtm, title, width, height)?;
        let initial_content_view = window
            .contentView()
            .ok_or_else(|| "macOS window missing contentView".to_string())?;
        let input_view =
            HostInputView::new(mtm, initial_content_view.frame(), ui_state, session_id);
        let content_view = input_view.clone().into_super();
        let window_delegate = HostWindowDelegate::new(mtm, ui_state, session_id);
        window.setContentView(Some(&content_view));
        window.setDelegate(Some(ProtocolObject::from_ref(&*window_delegate)));
        let tracking_area = create_tracking_area(mtm, &content_view);
        content_view.addTrackingArea(&tracking_area);
        let metrics = session_metrics(&window, &content_view);
        let (surface, selected_backend) =
            create_session_surface(&content_view, mtm, &metrics, requested_backend)?;
        let _ = window.makeFirstResponder(Some(input_view.as_super().as_super()));
        let focused = window.isKeyWindow();
        let stats = renderer_stats_log.then(|| Arc::new(RendererStatsCollector::new()));
        let event_runtime = HostEventRuntime::new(
            true,
            scroll_line_pixels,
            false,
            Arc::new(MacosHostEventSink {
                state: Arc::clone(state),
                session_id,
            }),
            stats.clone(),
        );

        Ok((
            HostSession {
                window,
                content_view,
                _input_view: input_view,
                _window_delegate: window_delegate,
                surface,
                renderer: SceneRenderer::with_cache_config(renderer_cache_config),
                tree: ElementTree::new(),
                render_state: RenderState::default(),
                logical_size: metrics.render_size,
                scale_factor: metrics.scale_factor,
                scroll_line_pixels,
                dirty: true,
                focused,
                initial_notifications_sent: false,
                initial_log_sent: false,
                selected_backend,
                _tracking_area: tracking_area,
                cursor_inside: false,
                stats,
                event_runtime,
                animation_runtime: AnimationRuntime::default(),
                latest_animation_sample_time: None,
                present: SessionPresentState::new(Duration::from_millis(16)),
            },
            selected_backend,
        ))
    }

    fn create_window(
        app: &NSApplication,
        mtm: MainThreadMarker,
        title: &str,
        width: u32,
        height: u32,
    ) -> Result<Retained<NSWindow>, String> {
        let frame = NSRect::new(
            NSPoint::new(120.0, 120.0),
            NSSize::new(width as f64, height as f64),
        );
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;

        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };

        let title = NSString::from_str(title);
        unsafe {
            window.setReleasedWhenClosed(false);
        }
        window.setTitle(&title);
        window.setAcceptsMouseMovedEvents(true);
        window.center();
        window.makeKeyAndOrderFront(None);
        app.activate();
        Ok(window)
    }

    fn create_tracking_area(
        mtm: MainThreadMarker,
        content_view: &NSView,
    ) -> Retained<NSTrackingArea> {
        let _ = mtm;
        unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                content_view.bounds(),
                NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::MouseMoved
                    | NSTrackingAreaOptions::ActiveInKeyWindow
                    | NSTrackingAreaOptions::InVisibleRect
                    | NSTrackingAreaOptions::EnabledDuringMouseDrag,
                None,
                None,
            )
        }
    }

    fn create_session_surface(
        content_view: &NSView,
        mtm: MainThreadMarker,
        metrics: &SessionMetrics,
        requested_backend: RequestedMacosBackend,
    ) -> Result<(SessionSurface, SelectedMacosBackend), String> {
        match requested_backend {
            RequestedMacosBackend::Metal => create_metal_surface(content_view, metrics)
                .map(|surface| (SessionSurface::Metal(surface), SelectedMacosBackend::Metal)),
            RequestedMacosBackend::Raster => {
                create_raster_surface(content_view, mtm, metrics).map(|surface| {
                    (
                        SessionSurface::Raster(surface),
                        SelectedMacosBackend::Raster,
                    )
                })
            }
            RequestedMacosBackend::Auto => match create_metal_surface(content_view, metrics) {
                Ok(surface) => Ok((SessionSurface::Metal(surface), SelectedMacosBackend::Metal)),
                Err(reason) => {
                    eprintln!("macOS host falling back to raster presenter: {reason}");
                    create_raster_surface(content_view, mtm, metrics).map(|surface| {
                        (
                            SessionSurface::Raster(surface),
                            SelectedMacosBackend::Raster,
                        )
                    })
                }
            },
        }
    }

    fn create_metal_surface(
        content_view: &NSView,
        metrics: &SessionMetrics,
    ) -> Result<MetalSurface, String> {
        let device =
            default_metal_device().ok_or_else(|| "no Metal device available".to_string())?;
        let metal_layer = CAMetalLayer::new();
        metal_layer.setDevice(Some(&device));
        metal_layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        metal_layer.setPresentsWithTransaction(false);
        metal_layer.setFramebufferOnly(false);
        metal_layer.setDrawableSize(CGSize::new(
            metrics.render_size.0 as f64,
            metrics.render_size.1 as f64,
        ));
        metal_layer.setContentsScale(metrics.scale_factor as f64);

        content_view.setWantsLayer(true);
        content_view.setLayer(Some(&metal_layer.clone().into_super()));

        let command_queue = device
            .newCommandQueue()
            .ok_or_else(|| "unable to create Metal command queue".to_string())?;

        let backend = unsafe {
            mtl::BackendContext::new(
                Retained::as_ptr(&device) as mtl::Handle,
                Retained::as_ptr(&command_queue) as mtl::Handle,
            )
        };

        let skia = gpu::direct_contexts::make_metal(&backend, None)
            .ok_or_else(|| "failed to create Skia Metal direct context".to_string())?;

        Ok(MetalSurface {
            metal_layer,
            command_queue,
            skia,
        })
    }

    fn create_raster_surface(
        content_view: &NSView,
        mtm: MainThreadMarker,
        metrics: &SessionMetrics,
    ) -> Result<RasterLayerSurface, String> {
        let image_view = NSImageView::initWithFrame(NSImageView::alloc(mtm), content_view.frame());
        image_view.setEditable(false);
        image_view.setImageScaling(NSImageScaling::ScaleAxesIndependently);
        content_view.addSubview(&image_view.clone().into_super());

        Ok(RasterLayerSurface {
            image_view,
            surface: create_raster_skia_surface(metrics.render_size)?,
            pixel_size: metrics.render_size,
        })
    }

    fn create_raster_skia_surface(render_size: (u32, u32)) -> Result<Surface, String> {
        let info = ImageInfo::new(
            (render_size.0 as i32, render_size.1 as i32),
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        );

        surfaces::raster(&info, None, None)
            .ok_or_else(|| "failed to create raster fallback surface".to_string())
    }

    fn default_metal_device() -> Option<Retained<ProtocolObject<dyn MTLDevice>>> {
        MTLCreateSystemDefaultDevice()
            .or_else(|| objc2_metal::MTLCopyAllDevices().to_vec().into_iter().next())
    }

    fn resize_surface(session: &mut HostSession, metrics: &SessionMetrics) {
        match &mut session.surface {
            SessionSurface::Metal(surface) => {
                surface.metal_layer.setDrawableSize(CGSize::new(
                    metrics.render_size.0 as f64,
                    metrics.render_size.1 as f64,
                ));
                surface
                    .metal_layer
                    .setContentsScale(metrics.scale_factor as f64);
            }
            SessionSurface::Raster(surface) => {
                surface.image_view.setFrame(session.content_view.frame());
                surface.pixel_size = metrics.render_size;
                surface.surface = create_raster_skia_surface(metrics.render_size)
                    .expect("raster fallback surface should resize");
            }
        }
    }

    fn draw_metal_surface(
        surface: &mut MetalSurface,
        renderer: &mut SceneRenderer,
        render_state: &RenderState,
        stats: Option<&RendererStatsCollector>,
    ) -> Result<(), String> {
        let Some(drawable) = surface.metal_layer.nextDrawable() else {
            return Ok(());
        };

        let render_started_at = Instant::now();
        let size = surface.metal_layer.drawableSize();
        let (drawable_width, drawable_height) = (size.width.max(1.0), size.height.max(1.0));

        let texture_info =
            unsafe { mtl::TextureInfo::new(Retained::as_ptr(&drawable.texture()) as mtl::Handle) };

        let backend_render_target = backend_render_targets::make_mtl(
            (drawable_width as i32, drawable_height as i32),
            &texture_info,
        );

        let mut skia_surface = gpu::surfaces::wrap_backend_render_target(
            &mut surface.skia,
            &backend_render_target,
            SurfaceOrigin::TopLeft,
            ColorType::BGRA8888,
            None,
            None,
        )
        .ok_or_else(|| "failed to wrap CAMetalLayer drawable as Skia surface".to_string())?;

        let mut render_timings = {
            let mut frame = RenderFrame::new(&mut skia_surface, Some(&mut surface.skia));
            renderer.render(&mut frame, render_state)
        };

        let extra_flush_started_at = Instant::now();
        let extra_gpu_flush_started_at = Instant::now();
        surface.skia.flush(None);
        render_timings.gpu_flush += extra_gpu_flush_started_at.elapsed();

        let extra_submit_started_at = Instant::now();
        surface.skia.submit(gpu::SyncCpu::No);
        render_timings.submit += extra_submit_started_at.elapsed();
        render_timings.flush += extra_flush_started_at.elapsed();

        if let Some(stats) = stats {
            stats.record_render(render_started_at.elapsed());
            stats.record_render_draw(render_timings.draw);
            stats.record_render_flush(render_timings.flush);
            stats.record_render_gpu_flush(render_timings.gpu_flush);
            stats.record_render_submit(render_timings.submit);
            if let Some(renderer_cache) = render_timings.renderer_cache.as_deref() {
                stats.record_renderer_cache(*renderer_cache);
            }
        }

        drop(skia_surface);

        let present_submit_started_at = Instant::now();
        let command_buffer = surface
            .command_queue
            .commandBuffer()
            .ok_or_else(|| "unable to get Metal command buffer".to_string())?;

        let drawable: Retained<ProtocolObject<dyn MTLDrawable>> = (&drawable).into();
        command_buffer.presentDrawable(&drawable);
        command_buffer.commit();

        if let Some(stats) = stats {
            stats.record_present_submit(present_submit_started_at.elapsed());
        }

        Ok(())
    }

    fn draw_raster_surface(
        surface: &mut RasterLayerSurface,
        renderer: &mut SceneRenderer,
        render_state: &RenderState,
        stats: Option<&RendererStatsCollector>,
    ) -> Result<(), String> {
        let render_started_at = Instant::now();

        let render_timings = {
            let mut frame = RenderFrame::new(&mut surface.surface, None);
            renderer.render(&mut frame, render_state)
        };

        if let Some(stats) = stats {
            stats.record_render(render_started_at.elapsed());
            stats.record_render_draw(render_timings.draw);
            stats.record_render_flush(render_timings.flush);
            stats.record_render_gpu_flush(render_timings.gpu_flush);
            stats.record_render_submit(render_timings.submit);
            if let Some(renderer_cache) = render_timings.renderer_cache.as_deref() {
                stats.record_renderer_cache(*renderer_cache);
            }
        }

        let image = surface.surface.image_snapshot();
        let encoded = image
            .encode(
                None::<&mut gpu::DirectContext>,
                EncodedImageFormat::PNG,
                100,
            )
            .ok_or_else(|| "failed to encode raster fallback frame".to_string())?;
        let data = NSData::with_bytes(encoded.as_bytes());
        let ns_image = NSImage::initWithData(NSImage::alloc(), &data)
            .ok_or_else(|| "failed to create NSImage from raster fallback frame".to_string())?;

        surface.image_view.setImage(Some(&ns_image));
        Ok(())
    }

    fn draw_session(session: &mut HostSession) -> Result<(), String> {
        match &mut session.surface {
            SessionSurface::Metal(surface) => draw_metal_surface(
                surface,
                &mut session.renderer,
                &session.render_state,
                session.stats.as_deref(),
            )?,
            SessionSurface::Raster(surface) => draw_raster_surface(
                surface,
                &mut session.renderer,
                &session.render_state,
                session.stats.as_deref(),
            )?,
        }

        if let Some(stats) = session.stats.as_ref() {
            stats.record_frame_present();
        }

        let presented_at = std::time::Instant::now();
        let predicted_next_present_at = session.present.observe_present(presented_at);

        if let Some(stats) = session.stats.as_ref() {
            stats.record_display_interval(
                predicted_next_present_at.saturating_duration_since(presented_at),
            );
        }

        if !(session.render_state.animate || !session.animation_runtime.is_empty()) {
            session.present.clear();
        }

        session.dirty = false;
        Ok(())
    }

    fn upload_tree(session: &mut HostSession, bytes: &[u8]) -> Result<(), String> {
        let decoded = deserialize::decode_tree(bytes).map_err(|err| err.to_string())?;
        session.tree.replace_with_uploaded(decoded);
        render_session_for_invalidation(session, TreeInvalidation::Structure)
    }

    fn patch_tree(session: &mut HostSession, bytes: &[u8]) -> Result<(), String> {
        let patches = patch::decode_patches(bytes).map_err(|err| err.to_string())?;
        let invalidation =
            patch::apply_patches(&mut session.tree, patches).map_err(|err| err.to_string())?;
        render_session_for_invalidation(session, invalidation)
    }

    fn render_session_for_invalidation(
        session: &mut HostSession,
        invalidation: TreeInvalidation,
    ) -> Result<(), String> {
        match decide_refresh_action(invalidation, false, session_refresh_availability(session)) {
            RefreshDecision::Skip | RefreshDecision::UseCachedRebuild => Ok(()),
            RefreshDecision::RefreshOnly => refresh_session(session),
            RefreshDecision::Recompute => rerender_session(session),
        }
    }

    fn session_refresh_availability(session: &HostSession) -> RefreshAvailability {
        RefreshAvailability {
            has_cached_rebuild: false,
            has_root_frame: tree_has_root_frame(&session.tree),
        }
    }

    fn tree_has_root_frame(tree: &ElementTree) -> bool {
        tree.root_id()
            .and_then(|root_id| tree.get(&root_id).and_then(|element| element.layout.frame))
            .is_some()
    }

    fn install_layout_output(session: &mut HostSession, output: LayoutOutput) {
        session.render_state.scene = output.scene;
        session.render_state.render_version = session.render_state.render_version.wrapping_add(1);
        session.render_state.animate = output.animations_active;
        session.dirty = true;
        session.event_runtime.install_rebuild(output.event_rebuild);
    }

    fn refresh_session(session: &mut HostSession) -> Result<(), String> {
        let mut iterations = 0;

        loop {
            assets::ensure_tree_sources(&session.tree);
            let refresh_started_at = std::time::Instant::now();
            let output = refresh(&mut session.tree);

            if let Some(stats) = session.stats.as_ref() {
                stats.record_refresh(refresh_started_at.elapsed());
            }

            install_layout_output(session, output);
            let runtime_messages = session.event_runtime.drain_tree_messages();

            if runtime_messages.is_empty() {
                return Ok(());
            }

            let invalidation = apply_tree_messages(session, runtime_messages)?;
            iterations += 1;

            if invalidation.is_none() || iterations >= 8 {
                return Ok(());
            }

            match decide_refresh_action(invalidation, false, session_refresh_availability(session))
            {
                RefreshDecision::Skip | RefreshDecision::UseCachedRebuild => return Ok(()),
                RefreshDecision::RefreshOnly => {}
                RefreshDecision::Recompute => return rerender_session(session),
            }
        }
    }

    fn rerender_session(session: &mut HostSession) -> Result<(), String> {
        let mut iterations = 0;

        loop {
            assets::ensure_tree_sources(&session.tree);
            let constraint =
                Constraint::new(session.logical_size.0 as f32, session.logical_size.1 as f32);
            let sample_time = session
                .latest_animation_sample_time
                .unwrap_or_else(std::time::Instant::now);

            session
                .animation_runtime
                .sync_with_tree(&session.tree, sample_time);
            let _ = session
                .animation_runtime
                .prune_completed_exit_ghosts(&mut session.tree, Some(sample_time));
            session.tree.set_layout_cache_stats_enabled(
                session
                    .stats
                    .as_ref()
                    .is_some_and(|stats| stats.layout_cache_enabled()),
            );
            let layout_started_at = std::time::Instant::now();

            let output = if session.animation_runtime.is_empty() {
                layout_and_refresh_default(&mut session.tree, constraint, session.scale_factor)
            } else {
                layout_and_refresh_default_with_animation(
                    &mut session.tree,
                    constraint,
                    session.scale_factor,
                    &session.animation_runtime,
                    sample_time,
                )
            };

            if let Some(stats) = session.stats.as_ref() {
                stats.record_layout(layout_started_at.elapsed());
                stats.record_layout_cache(session.tree.layout_cache_stats());
            }

            let animations_active = output.animations_active;
            install_layout_output(session, output);

            if session.animation_runtime.is_empty() || !animations_active {
                session.latest_animation_sample_time = None;
            } else {
                session.latest_animation_sample_time = Some(sample_time);
            }

            let runtime_messages = session.event_runtime.drain_tree_messages();

            if runtime_messages.is_empty() {
                return Ok(());
            }

            let invalidation = apply_tree_messages(session, runtime_messages)?;
            iterations += 1;

            if invalidation.is_none() || iterations >= 8 {
                return Ok(());
            }

            if matches!(
                decide_refresh_action(invalidation, false, session_refresh_availability(session)),
                RefreshDecision::RefreshOnly
            ) {
                return refresh_session(session);
            }
        }
    }

    fn handle_runtime_input(session: &mut HostSession, event: InputEvent) -> Result<(), String> {
        session.event_runtime.handle_input(event);
        let runtime_messages = session.event_runtime.drain_tree_messages();

        if runtime_messages.is_empty() {
            return Ok(());
        }

        let invalidation = apply_tree_messages(session, runtime_messages)?;
        render_session_for_invalidation(session, invalidation)
    }

    fn handle_runtime_text_input_command(
        session: &mut HostSession,
        request: TextInputCommandRequest,
    ) -> Result<(), String> {
        if !session.event_runtime.handle_text_input_command(request) {
            return Ok(());
        }

        let runtime_messages = session.event_runtime.drain_tree_messages();

        if runtime_messages.is_empty() {
            return Ok(());
        }

        let invalidation = apply_tree_messages(session, runtime_messages)?;
        render_session_for_invalidation(session, invalidation)
    }

    fn handle_runtime_text_input_edit(
        session: &mut HostSession,
        request: TextInputEditRequest,
    ) -> Result<(), String> {
        if !session.event_runtime.handle_text_input_edit(request) {
            return Ok(());
        }

        let runtime_messages = session.event_runtime.drain_tree_messages();

        if runtime_messages.is_empty() {
            return Ok(());
        }

        let invalidation = apply_tree_messages(session, runtime_messages)?;
        render_session_for_invalidation(session, invalidation)
    }

    fn apply_tree_messages(
        session: &mut HostSession,
        messages: Vec<emerge_skia::actors::TreeMsg>,
    ) -> Result<TreeInvalidation, String> {
        let mut scroll_acc = HashMap::new();
        let mut thumb_drag_x_acc = HashMap::new();
        let mut thumb_drag_y_acc = HashMap::new();
        let mut hover_x_state = HashMap::new();
        let mut hover_y_state = HashMap::new();
        let mut mouse_over_state = HashMap::new();
        let mut mouse_down_state = HashMap::new();
        let mut focused_state = HashMap::new();
        let mut invalidation = TreeInvalidation::None;

        for message in flatten_tree_messages(messages) {
            match message {
                emerge_skia::actors::TreeMsg::Stop => {}
                emerge_skia::actors::TreeMsg::Batch(_) => unreachable!(),
                emerge_skia::actors::TreeMsg::UploadTree { bytes, .. } => {
                    let decoded =
                        deserialize::decode_tree(&bytes).map_err(|err| err.to_string())?;
                    session.tree.replace_with_uploaded(decoded);
                    invalidation.add(TreeInvalidation::Structure);
                }
                emerge_skia::actors::TreeMsg::PatchTree { bytes, .. } => {
                    let patches = patch::decode_patches(&bytes).map_err(|err| err.to_string())?;
                    invalidation.add(
                        patch::apply_patches(&mut session.tree, patches)
                            .map_err(|err| err.to_string())?,
                    );
                }
                emerge_skia::actors::TreeMsg::Resize {
                    width,
                    height,
                    scale,
                } => {
                    session.logical_size = (
                        width.max(1.0).round() as u32,
                        height.max(1.0).round() as u32,
                    );
                    session.scale_factor = scale;
                    let metrics = SessionMetrics {
                        render_size: session.logical_size,
                        scale_factor: session.scale_factor,
                    };
                    resize_surface(session, &metrics);
                    invalidation.add(TreeInvalidation::Measure);
                }
                emerge_skia::actors::TreeMsg::ScrollRequest { element_id, dx, dy } => {
                    let entry = scroll_acc.entry(element_id).or_insert((0.0, 0.0));
                    entry.0 += dx;
                    entry.1 += dy;
                }
                emerge_skia::actors::TreeMsg::ScrollbarThumbDragX { element_id, dx } => {
                    let entry = thumb_drag_x_acc.entry(element_id).or_insert(0.0);
                    *entry += dx;
                }
                emerge_skia::actors::TreeMsg::ScrollbarThumbDragY { element_id, dy } => {
                    let entry = thumb_drag_y_acc.entry(element_id).or_insert(0.0);
                    *entry += dy;
                }
                emerge_skia::actors::TreeMsg::SetScrollbarXHover {
                    element_id,
                    hovered,
                } => {
                    hover_x_state.insert(element_id, hovered);
                }
                emerge_skia::actors::TreeMsg::SetScrollbarYHover {
                    element_id,
                    hovered,
                } => {
                    hover_y_state.insert(element_id, hovered);
                }
                emerge_skia::actors::TreeMsg::SetMouseOverActive { element_id, active } => {
                    mouse_over_state.insert(element_id, active);
                }
                emerge_skia::actors::TreeMsg::SetMouseDownActive { element_id, active } => {
                    mouse_down_state.insert(element_id, active);
                }
                emerge_skia::actors::TreeMsg::SetFocusedActive { element_id, active } => {
                    focused_state.insert(element_id, active);
                }
                emerge_skia::actors::TreeMsg::SetTextInputContent {
                    element_id,
                    content,
                } => {
                    invalidation.add(session.tree.set_text_input_content(&element_id, content));
                }
                emerge_skia::actors::TreeMsg::SetTextInputRuntime {
                    element_id,
                    focused,
                    cursor,
                    selection_anchor,
                    preedit,
                    preedit_cursor,
                } => {
                    invalidation.add(session.tree.set_text_input_runtime(
                        &element_id,
                        focused,
                        cursor,
                        selection_anchor,
                        preedit,
                        preedit_cursor,
                    ));
                }
                emerge_skia::actors::TreeMsg::AnimationPulse {
                    presented_at,
                    predicted_next_present_at,
                    trace: _,
                } => {
                    session.latest_animation_sample_time =
                        Some(predicted_next_present_at.max(presented_at));
                    invalidation.add(TreeInvalidation::Measure);
                }
                emerge_skia::actors::TreeMsg::RebuildRegistry => {
                    invalidation.add(TreeInvalidation::Registry);
                }
                emerge_skia::actors::TreeMsg::AssetStateChanged => {
                    invalidation.add(TreeInvalidation::Measure);
                }
            }
        }

        for (id, (dx, dy)) in scroll_acc {
            invalidation.add(session.tree.apply_scroll(&id, dx, dy));
        }
        for (id, dx) in thumb_drag_x_acc {
            invalidation.add(session.tree.apply_scroll_x(&id, dx));
        }
        for (id, dy) in thumb_drag_y_acc {
            invalidation.add(session.tree.apply_scroll_y(&id, dy));
        }
        for (id, hovered) in hover_x_state {
            invalidation.add(session.tree.set_scrollbar_x_hover(&id, hovered));
        }
        for (id, hovered) in hover_y_state {
            invalidation.add(session.tree.set_scrollbar_y_hover(&id, hovered));
        }
        for (id, active) in mouse_over_state {
            invalidation.add(session.tree.set_mouse_over_active(&id, active));
        }
        for (id, active) in mouse_down_state {
            invalidation.add(session.tree.set_mouse_down_active(&id, active));
        }
        for (id, active) in focused_state {
            invalidation.add(session.tree.set_focused_active(&id, active));
        }

        Ok(invalidation)
    }

    fn flatten_tree_messages(
        messages: Vec<emerge_skia::actors::TreeMsg>,
    ) -> Vec<emerge_skia::actors::TreeMsg> {
        let mut flat = Vec::new();
        let mut stack = messages;

        while let Some(message) = stack.pop() {
            match message {
                emerge_skia::actors::TreeMsg::Batch(batch) => {
                    for msg in batch.into_iter().rev() {
                        stack.push(msg);
                    }
                }
                other => flat.push(other),
            }
        }

        flat.reverse();
        flat
    }

    struct SessionMetrics {
        render_size: (u32, u32),
        scale_factor: f32,
    }

    fn session_metrics(window: &NSWindow, content_view: &NSView) -> SessionMetrics {
        let frame = content_view.frame();
        let scale_factor = window.backingScaleFactor() as f32;
        let width = frame.size.width.max(1.0);
        let height = frame.size.height.max(1.0);
        let render_size =
            render_size_for_view((width.round() as u32, height.round() as u32), scale_factor);

        SessionMetrics {
            render_size,
            scale_factor: scale_factor.max(1.0),
        }
    }

    pub(super) fn render_size_for_view(view_size: (u32, u32), scale_factor: f32) -> (u32, u32) {
        let scale = scale_factor.max(1.0);
        let width = (view_size.0.max(1) as f32 * scale).round() as u32;
        let height = (view_size.1.max(1) as f32 * scale).round() as u32;
        (width.max(1), height.max(1))
    }

    pub(super) fn render_point_for_view_point(point: (f32, f32), scale_factor: f32) -> (f32, f32) {
        let scale = scale_factor.max(1.0);
        (point.0 * scale, point.1 * scale)
    }

    pub(super) fn view_rect_for_render_rect(
        rect: (f32, f32, f32, f32),
        scale_factor: f32,
    ) -> (f32, f32, f32, f32) {
        let scale = scale_factor.max(1.0);
        (
            rect.0 / scale,
            rect.1 / scale,
            rect.2 / scale,
            rect.3 / scale,
        )
    }

    fn notify_resized(
        state: &Arc<HostState>,
        session_id: u64,
        logical_size: (u32, u32),
        scale_factor: f32,
    ) {
        let mut payload = Vec::with_capacity(12);
        payload.extend_from_slice(&logical_size.0.to_be_bytes());
        payload.extend_from_slice(&logical_size.1.to_be_bytes());
        payload.extend_from_slice(&scale_factor.to_bits().to_be_bytes());

        state.send_frame(encode_frame(
            FRAME_NOTIFY,
            0,
            session_id,
            NOTIFY_RESIZED,
            &payload,
        ));
    }

    fn notify_focused(state: &Arc<HostState>, session_id: u64, focused: bool) {
        state.send_frame(encode_frame(
            FRAME_NOTIFY,
            0,
            session_id,
            NOTIFY_FOCUSED,
            &[if focused { 1 } else { 0 }],
        ));
    }

    fn notify_close_requested(state: &Arc<HostState>, session_id: u64) {
        state.send_frame(encode_frame(
            FRAME_NOTIFY,
            0,
            session_id,
            NOTIFY_CLOSE_REQUESTED,
            &[],
        ));
    }

    fn notify_running(state: &Arc<HostState>, session_id: u64) {
        state.send_frame(encode_frame(
            FRAME_NOTIFY,
            0,
            session_id,
            NOTIFY_RUNNING,
            &[],
        ));
    }

    fn notify_log(state: &Arc<HostState>, session_id: u64, level: u8, source: &str, message: &str) {
        let source = source.as_bytes();
        let message = message.as_bytes();
        let mut payload = Vec::with_capacity(1 + 4 + source.len() + 4 + message.len());
        payload.push(level);
        payload.extend_from_slice(&(source.len() as u32).to_be_bytes());
        payload.extend_from_slice(source);
        payload.extend_from_slice(&(message.len() as u32).to_be_bytes());
        payload.extend_from_slice(message);

        state.send_frame(encode_frame(
            FRAME_NOTIFY,
            0,
            session_id,
            NOTIFY_LOG,
            &payload,
        ));
    }

    fn notify_cursor_pos(state: &Arc<HostState>, session_id: u64, x: f32, y: f32) {
        let mut payload = Vec::with_capacity(8);
        payload.extend_from_slice(&x.to_bits().to_be_bytes());
        payload.extend_from_slice(&y.to_bits().to_be_bytes());

        state.send_frame(encode_frame(
            FRAME_NOTIFY,
            0,
            session_id,
            NOTIFY_CURSOR_POS,
            &payload,
        ));
    }

    fn notify_cursor_button(
        state: &Arc<HostState>,
        session_id: u64,
        button: u8,
        action: u8,
        mods: u8,
        x: f32,
        y: f32,
    ) {
        let mut payload = Vec::with_capacity(11);
        payload.push(button);
        payload.push(action);
        payload.push(mods);
        payload.extend_from_slice(&x.to_bits().to_be_bytes());
        payload.extend_from_slice(&y.to_bits().to_be_bytes());

        state.send_frame(encode_frame(
            FRAME_NOTIFY,
            0,
            session_id,
            NOTIFY_CURSOR_BUTTON,
            &payload,
        ));
    }

    fn notify_cursor_scroll(
        state: &Arc<HostState>,
        session_id: u64,
        dx: f32,
        dy: f32,
        x: f32,
        y: f32,
    ) {
        let mut payload = Vec::with_capacity(16);
        payload.extend_from_slice(&dx.to_bits().to_be_bytes());
        payload.extend_from_slice(&dy.to_bits().to_be_bytes());
        payload.extend_from_slice(&x.to_bits().to_be_bytes());
        payload.extend_from_slice(&y.to_bits().to_be_bytes());

        state.send_frame(encode_frame(
            FRAME_NOTIFY,
            0,
            session_id,
            NOTIFY_CURSOR_SCROLL,
            &payload,
        ));
    }

    fn notify_cursor_entered(state: &Arc<HostState>, session_id: u64, entered: bool) {
        state.send_frame(encode_frame(
            FRAME_NOTIFY,
            0,
            session_id,
            NOTIFY_CURSOR_ENTERED,
            &[if entered { 1 } else { 0 }],
        ));
    }

    fn notify_input_event(state: &Arc<HostState>, session_id: u64, event: &InputEvent) {
        match event {
            InputEvent::CursorPos { x, y } => notify_cursor_pos(state, session_id, *x, *y),
            InputEvent::CursorButton {
                button,
                action,
                mods,
                x,
                y,
            } => notify_cursor_button(
                state,
                session_id,
                encode_button(button),
                *action,
                *mods,
                *x,
                *y,
            ),
            InputEvent::CursorScroll { dx, dy, x, y }
            | InputEvent::CursorScrollLines { dx, dy, x, y } => {
                notify_cursor_scroll(state, session_id, *dx, *dy, *x, *y)
            }
            InputEvent::CursorEntered { entered } => {
                notify_cursor_entered(state, session_id, *entered)
            }
            InputEvent::Resized {
                width,
                height,
                scale_factor,
            } => notify_resized(state, session_id, (*width, *height), *scale_factor),
            InputEvent::Focused { focused } => notify_focused(state, session_id, *focused),
            InputEvent::Key { key, action, mods } => {
                notify_key(state, session_id, *key, *action, *mods)
            }
            InputEvent::TextCommit { text, mods } => {
                notify_text_commit(state, session_id, text, *mods)
            }
            InputEvent::TextPreedit { text, cursor } => {
                notify_text_preedit(state, session_id, text, *cursor)
            }
            InputEvent::TextPreeditClear => notify_text_preedit_clear(state, session_id),
            _ => {}
        }
    }

    fn notify_element_event(
        state: &Arc<HostState>,
        session_id: u64,
        element_id: &emerge_skia::tree::element::NodeId,
        kind: ElementEventKind,
        payload: Option<&str>,
    ) {
        let id_bytes = element_id.0.as_slice();
        let has_payload = if payload.is_some() { 1 } else { 0 };
        let payload = payload.unwrap_or("").as_bytes();
        let mut data = Vec::with_capacity(1 + 1 + 4 + id_bytes.len() + 4 + payload.len());
        data.push(encode_element_event_kind(kind));
        data.push(has_payload);
        data.extend_from_slice(&(id_bytes.len() as u32).to_be_bytes());
        data.extend_from_slice(id_bytes);
        data.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        data.extend_from_slice(payload);

        state.send_frame(encode_frame(
            FRAME_NOTIFY,
            0,
            session_id,
            NOTIFY_ELEMENT_EVENT,
            &data,
        ));
    }

    fn notify_key(
        state: &Arc<HostState>,
        session_id: u64,
        key: CanonicalKey,
        action: u8,
        mods: u8,
    ) {
        let key_name = key.atom_name().as_bytes();
        let mut payload = Vec::with_capacity(4 + key_name.len() + 2);
        payload.extend_from_slice(&(key_name.len() as u32).to_be_bytes());
        payload.extend_from_slice(key_name);
        payload.push(action);
        payload.push(mods);

        state.send_frame(encode_frame(
            FRAME_NOTIFY,
            0,
            session_id,
            NOTIFY_KEY,
            &payload,
        ));
    }

    fn notify_text_commit(state: &Arc<HostState>, session_id: u64, text: &str, mods: u8) {
        let text = text.as_bytes();
        let mut payload = Vec::with_capacity(4 + text.len() + 1);
        payload.extend_from_slice(&(text.len() as u32).to_be_bytes());
        payload.extend_from_slice(text);
        payload.push(mods);

        state.send_frame(encode_frame(
            FRAME_NOTIFY,
            0,
            session_id,
            NOTIFY_TEXT_COMMIT,
            &payload,
        ));
    }

    fn notify_text_preedit(
        state: &Arc<HostState>,
        session_id: u64,
        text: &str,
        cursor: Option<(u32, u32)>,
    ) {
        let text = text.as_bytes();
        let mut payload = Vec::with_capacity(4 + text.len() + 1 + 8);
        payload.extend_from_slice(&(text.len() as u32).to_be_bytes());
        payload.extend_from_slice(text);

        match cursor {
            Some((start, end)) => {
                payload.push(1);
                payload.extend_from_slice(&start.to_be_bytes());
                payload.extend_from_slice(&end.to_be_bytes());
            }
            None => payload.push(0),
        }

        state.send_frame(encode_frame(
            FRAME_NOTIFY,
            0,
            session_id,
            NOTIFY_TEXT_PREEDIT,
            &payload,
        ));
    }

    fn notify_text_preedit_clear(state: &Arc<HostState>, session_id: u64) {
        state.send_frame(encode_frame(
            FRAME_NOTIFY,
            0,
            session_id,
            NOTIFY_TEXT_PREEDIT_CLEAR,
            &[],
        ));
    }

    fn selected_backend_name(backend: SelectedMacosBackend) -> &'static str {
        match backend {
            SelectedMacosBackend::Metal => "metal",
            SelectedMacosBackend::Raster => "raster",
        }
    }

    fn selected_backend_stats_label(backend: SelectedMacosBackend) -> &'static str {
        match backend {
            SelectedMacosBackend::Metal => "macos-metal",
            SelectedMacosBackend::Raster => "macos-raster",
        }
    }

    fn encode_button(button: &str) -> u8 {
        match button {
            "left" => BUTTON_LEFT,
            "right" => BUTTON_RIGHT,
            "middle" => BUTTON_MIDDLE,
            _ => BUTTON_MIDDLE,
        }
    }

    fn decode_button_name(button: u8) -> &'static str {
        match button {
            BUTTON_LEFT => "left",
            BUTTON_RIGHT => "right",
            BUTTON_MIDDLE => "middle",
            _ => "middle",
        }
    }

    fn encode_element_event_kind(kind: ElementEventKind) -> u8 {
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
            ElementEventKind::Focus => ELEMENT_EVENT_FOCUS,
            ElementEventKind::Blur => ELEMENT_EVENT_BLUR,
            ElementEventKind::Change => ELEMENT_EVENT_CHANGE,
        }
    }

    fn dispatch_pointer_event(ui_state: &Rc<RefCell<HostUiState>>, event: &NSEvent) {
        let mut ui_state = ui_state.borrow_mut();
        let Some(session_id) =
            find_session_id_for_window_number(&ui_state.sessions, event.windowNumber())
        else {
            return;
        };

        let Some(session) = ui_state.sessions.get_mut(&session_id) else {
            return;
        };

        match event.r#type() {
            NSEventType::MouseMoved
            | NSEventType::LeftMouseDragged
            | NSEventType::RightMouseDragged
            | NSEventType::OtherMouseDragged => {
                let (x, y) =
                    event_point_in_view(event, &session.content_view, session.scale_factor);
                let _ = handle_runtime_input(session, InputEvent::CursorPos { x, y });
            }
            NSEventType::LeftMouseDown
            | NSEventType::LeftMouseUp
            | NSEventType::RightMouseDown
            | NSEventType::RightMouseUp
            | NSEventType::OtherMouseDown
            | NSEventType::OtherMouseUp => {
                let (x, y) =
                    event_point_in_view(event, &session.content_view, session.scale_factor);
                let _ = handle_runtime_input(
                    session,
                    InputEvent::CursorButton {
                        button: decode_button_name(button_tag(event)).to_string(),
                        action: button_action(event),
                        mods: modifier_bits(event.modifierFlags()),
                        x,
                        y,
                    },
                );
            }
            NSEventType::ScrollWheel => {
                let (x, y) =
                    event_point_in_view(event, &session.content_view, session.scale_factor);
                let (dx, dy) =
                    scroll_deltas(event, session.scale_factor, session.scroll_line_pixels);
                let _ = handle_runtime_input(session, InputEvent::CursorScroll { dx, dy, x, y });
            }
            NSEventType::MouseEntered => {
                if !session.cursor_inside {
                    session.cursor_inside = true;
                    let _ =
                        handle_runtime_input(session, InputEvent::CursorEntered { entered: true });
                }
            }
            NSEventType::MouseExited => {
                if session.cursor_inside {
                    session.cursor_inside = false;
                    let _ =
                        handle_runtime_input(session, InputEvent::CursorEntered { entered: false });
                }
            }
            _ => {}
        }
    }

    fn find_session_id_for_window_number(
        sessions: &HashMap<u64, HostSession>,
        window_number: objc2_foundation::NSInteger,
    ) -> Option<u64> {
        sessions.iter().find_map(|(session_id, session)| {
            (session.window.windowNumber() == window_number).then_some(*session_id)
        })
    }

    fn event_point_in_view(event: &NSEvent, view: &NSView, scale_factor: f32) -> (f32, f32) {
        let point = view.convertPoint_fromView(event.locationInWindow(), None);
        let bounds = view.bounds();
        let x = point.x as f32;
        let y = if view.isFlipped() {
            point.y as f32
        } else {
            (bounds.size.height - point.y) as f32
        };

        render_point_for_view_point((x, y), scale_factor)
    }

    fn button_tag(event: &NSEvent) -> u8 {
        match event.r#type() {
            NSEventType::LeftMouseDown | NSEventType::LeftMouseUp => BUTTON_LEFT,
            NSEventType::RightMouseDown | NSEventType::RightMouseUp => BUTTON_RIGHT,
            _ => match event.buttonNumber() {
                2 => BUTTON_MIDDLE,
                _ => BUTTON_MIDDLE,
            },
        }
    }

    fn button_action(event: &NSEvent) -> u8 {
        match event.r#type() {
            NSEventType::LeftMouseDown
            | NSEventType::RightMouseDown
            | NSEventType::OtherMouseDown => ACTION_PRESS,
            _ => ACTION_RELEASE,
        }
    }

    fn modifier_bits(flags: NSEventModifierFlags) -> u8 {
        let mut mods = 0;

        if flags.contains(NSEventModifierFlags::Shift) {
            mods |= MOD_SHIFT;
        }
        if flags.contains(NSEventModifierFlags::Control) {
            mods |= MOD_CTRL;
        }
        if flags.contains(NSEventModifierFlags::Option) {
            mods |= MOD_ALT;
        }
        if flags.contains(NSEventModifierFlags::Command) {
            mods |= MOD_META;
        }

        mods
    }

    fn scroll_deltas(event: &NSEvent, scale_factor: f32, scroll_line_pixels: f32) -> (f32, f32) {
        if event.hasPreciseScrollingDeltas() {
            precise_scroll_deltas(
                event.scrollingDeltaX() as f32,
                event.scrollingDeltaY() as f32,
                scale_factor,
            )
        } else {
            line_scroll_deltas(
                event.deltaX() as f32,
                event.deltaY() as f32,
                scroll_line_pixels,
            )
        }
    }

    pub(super) fn precise_scroll_deltas(dx: f32, dy: f32, scale_factor: f32) -> (f32, f32) {
        let scale = scale_factor.max(1.0);
        (dx * scale, dy * scale)
    }

    pub(super) fn line_scroll_deltas(dx: f32, dy: f32, scroll_line_pixels: f32) -> (f32, f32) {
        (dx * scroll_line_pixels, dy * scroll_line_pixels)
    }

    fn canonical_key_for_event(event: &NSEvent) -> CanonicalKey {
        match event.keyCode() {
            0 => CanonicalKey::A,
            1 => CanonicalKey::S,
            2 => CanonicalKey::D,
            3 => CanonicalKey::F,
            4 => CanonicalKey::H,
            5 => CanonicalKey::G,
            6 => CanonicalKey::Z,
            7 => CanonicalKey::X,
            8 => CanonicalKey::C,
            9 => CanonicalKey::V,
            11 => CanonicalKey::B,
            12 => CanonicalKey::Q,
            13 => CanonicalKey::W,
            14 => CanonicalKey::E,
            15 => CanonicalKey::R,
            16 => CanonicalKey::Y,
            17 => CanonicalKey::T,
            18 => CanonicalKey::Digit1,
            19 => CanonicalKey::Digit2,
            20 => CanonicalKey::Digit3,
            21 => CanonicalKey::Digit4,
            22 => CanonicalKey::Digit6,
            23 => CanonicalKey::Digit5,
            24 => CanonicalKey::Equal,
            25 => CanonicalKey::Digit9,
            26 => CanonicalKey::Digit7,
            27 => CanonicalKey::Minus,
            28 => CanonicalKey::Digit8,
            29 => CanonicalKey::Digit0,
            30 => CanonicalKey::RightBracket,
            31 => CanonicalKey::O,
            32 => CanonicalKey::U,
            33 => CanonicalKey::LeftBracket,
            34 => CanonicalKey::I,
            35 => CanonicalKey::P,
            37 => CanonicalKey::L,
            38 => CanonicalKey::J,
            39 => CanonicalKey::Apostrophe,
            40 => CanonicalKey::K,
            41 => CanonicalKey::Semicolon,
            42 => CanonicalKey::Backslash,
            43 => CanonicalKey::Comma,
            44 => CanonicalKey::Slash,
            45 => CanonicalKey::N,
            46 => CanonicalKey::M,
            47 => CanonicalKey::Period,
            49 => CanonicalKey::Space,
            36 => CanonicalKey::Enter,
            48 => CanonicalKey::Tab,
            51 => CanonicalKey::Backspace,
            53 => CanonicalKey::Escape,
            114 => CanonicalKey::Insert,
            117 => CanonicalKey::Delete,
            115 => CanonicalKey::Home,
            119 => CanonicalKey::End,
            116 => CanonicalKey::PageUp,
            121 => CanonicalKey::PageDown,
            123 => CanonicalKey::ArrowLeft,
            124 => CanonicalKey::ArrowRight,
            125 => CanonicalKey::ArrowDown,
            126 => CanonicalKey::ArrowUp,
            122 => CanonicalKey::F1,
            120 => CanonicalKey::F2,
            99 => CanonicalKey::F3,
            118 => CanonicalKey::F4,
            96 => CanonicalKey::F5,
            97 => CanonicalKey::F6,
            98 => CanonicalKey::F7,
            100 => CanonicalKey::F8,
            101 => CanonicalKey::F9,
            109 => CanonicalKey::F10,
            103 => CanonicalKey::F11,
            111 => CanonicalKey::F12,
            105 => CanonicalKey::F13,
            107 => CanonicalKey::F14,
            113 => CanonicalKey::F15,
            64 => CanonicalKey::F17,
            79 => CanonicalKey::F18,
            80 => CanonicalKey::F19,
            90 => CanonicalKey::F20,
            56 | 60 => CanonicalKey::Shift,
            59 | 62 => CanonicalKey::Control,
            58 => CanonicalKey::Alt,
            61 => CanonicalKey::AltGraph,
            55 | 54 => CanonicalKey::Super,
            57 => CanonicalKey::CapsLock,
            71 => CanonicalKey::NumLock,
            _ => event
                .charactersIgnoringModifiers()
                .and_then(|chars| chars.to_string().chars().next())
                .and_then(CanonicalKey::from_printable_char)
                .unwrap_or(CanonicalKey::Unknown),
        }
    }

    fn modifier_action(event: &NSEvent, mods: u8, key: CanonicalKey) -> u8 {
        let pressed = match key {
            CanonicalKey::Shift => mods & MOD_SHIFT != 0,
            CanonicalKey::Control => mods & MOD_CTRL != 0,
            CanonicalKey::Alt => mods & MOD_ALT != 0,
            CanonicalKey::AltGraph => mods & MOD_ALT != 0,
            CanonicalKey::Super => mods & MOD_META != 0,
            CanonicalKey::CapsLock => event
                .modifierFlags()
                .contains(NSEventModifierFlags::CapsLock),
            _ => false,
        };

        if pressed {
            ACTION_PRESS
        } else {
            ACTION_RELEASE
        }
    }

    #[cfg(test)]
    pub(crate) fn text_commit_from_key_and_text(
        key: CanonicalKey,
        mods: u8,
        text: &str,
    ) -> Option<String> {
        if mods & (MOD_CTRL | MOD_META) != 0 || suppress_text_commit_for_key(key) {
            return None;
        }

        let filtered: String = text
            .chars()
            .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\r' | '\t'))
            .collect();

        if filtered.is_empty() {
            None
        } else {
            Some(filtered)
        }
    }

    #[cfg(test)]
    pub(crate) fn suppress_text_commit_for_key(key: CanonicalKey) -> bool {
        matches!(
            key,
            CanonicalKey::Escape
                | CanonicalKey::Backspace
                | CanonicalKey::Delete
                | CanonicalKey::Insert
                | CanonicalKey::Home
                | CanonicalKey::End
                | CanonicalKey::PageUp
                | CanonicalKey::PageDown
                | CanonicalKey::ArrowLeft
                | CanonicalKey::ArrowRight
                | CanonicalKey::ArrowUp
                | CanonicalKey::ArrowDown
                | CanonicalKey::Shift
                | CanonicalKey::Control
                | CanonicalKey::Alt
                | CanonicalKey::AltGraph
                | CanonicalKey::Super
                | CanonicalKey::CapsLock
                | CanonicalKey::NumLock
                | CanonicalKey::F1
                | CanonicalKey::F2
                | CanonicalKey::F3
                | CanonicalKey::F4
                | CanonicalKey::F5
                | CanonicalKey::F6
                | CanonicalKey::F7
                | CanonicalKey::F8
                | CanonicalKey::F9
                | CanonicalKey::F10
                | CanonicalKey::F11
                | CanonicalKey::F12
                | CanonicalKey::F13
                | CanonicalKey::F14
                | CanonicalKey::F15
                | CanonicalKey::F17
                | CanonicalKey::F18
                | CanonicalKey::F19
                | CanonicalKey::F20
        )
    }

    fn listener_thread(
        socket_path: PathBuf,
        state: Arc<HostState>,
        command_tx: Sender<HostCommand>,
        startup_tx: std::sync::mpsc::Sender<Result<(), String>>,
    ) {
        let _ = fs::remove_file(&socket_path);

        let listener = match UnixListener::bind(&socket_path) {
            Ok(listener) => listener,
            Err(err) => {
                let _ = startup_tx.send(Err(format!(
                    "failed to bind macOS host socket {}: {err}",
                    socket_path.display()
                )));
                return;
            }
        };

        let _ = startup_tx.send(Ok(()));

        while state.is_running() {
            match listener.accept() {
                Ok((stream, _addr)) => {
                    let _ = handle_client(stream, &state, &command_tx);
                    state.clear_outbound();
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) if !state.is_running() => break,
                Err(err) => {
                    eprintln!("macOS host listener accept error: {err}");
                    break;
                }
            }
        }
    }

    fn handle_client(
        mut stream: UnixStream,
        state: &Arc<HostState>,
        command_tx: &Sender<HostCommand>,
    ) -> Result<(), String> {
        let write_stream = stream
            .try_clone()
            .map_err(|err| format!("failed to clone macOS host socket: {err}"))?;
        let (outbound_tx, outbound_rx) = unbounded::<Vec<u8>>();
        state.set_outbound(outbound_tx.clone());

        let writer_handle = thread::Builder::new()
            .name("emerge_skia_macos_host_writer".to_string())
            .spawn(move || writer_thread(write_stream, outbound_rx))
            .map_err(|err| format!("failed to spawn macOS host writer thread: {err}"))?;

        let heartbeat_state = Arc::clone(state);
        let heartbeat_running = Arc::new(AtomicBool::new(true));
        let heartbeat_running_for_thread = Arc::clone(&heartbeat_running);
        let heartbeat_handle = thread::Builder::new()
            .name("emerge_skia_macos_host_heartbeat".to_string())
            .spawn(move || running_heartbeat_thread(heartbeat_state, heartbeat_running_for_thread))
            .map_err(|err| format!("failed to spawn macOS host heartbeat thread: {err}"))?;

        let mut initialized = false;

        while state.is_running() {
            let request = match read_frame(&mut stream) {
                Ok(frame) => frame,
                Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(err) if err.kind() == io::ErrorKind::BrokenPipe => break,
                Err(err) => return Err(format!("failed reading macOS host request: {err}")),
            };

            let frame = match decode_frame(&request) {
                Ok(frame) => frame,
                Err(reason) => {
                    let _ = outbound_tx.send(encode_frame(FRAME_ERROR, 0, 0, 0, reason.as_bytes()));
                    continue;
                }
            };

            if !initialized {
                match handle_init_frame(frame, state) {
                    Ok(reply) => {
                        initialized = true;
                        let _ = outbound_tx.send(reply);
                    }
                    Err(reason) => {
                        let _ =
                            outbound_tx.send(encode_frame(FRAME_ERROR, 0, 0, 0, reason.as_bytes()));
                        break;
                    }
                }

                continue;
            }

            let reply = dispatch_request(frame, command_tx);
            let _ = outbound_tx.send(reply);
        }

        state.clear_outbound();
        drop(outbound_tx);
        heartbeat_running.store(false, Ordering::Release);
        let _ = writer_handle.join();
        let _ = heartbeat_handle.join();

        Ok(())
    }

    fn writer_thread(mut stream: UnixStream, outbound_rx: Receiver<Vec<u8>>) {
        while let Ok(frame) = outbound_rx.recv() {
            if write_frame(&mut stream, &frame).is_err() {
                break;
            }
        }
    }

    fn running_heartbeat_thread(state: Arc<HostState>, running: Arc<AtomicBool>) {
        let mut ticks = 0_u64;

        while state.is_running() && running.load(Ordering::Acquire) {
            state
                .running_session_ids()
                .into_iter()
                .for_each(|session_id| notify_running(&state, session_id));

            ticks = ticks.wrapping_add(1);

            if ticks % 10 == 0 {
                state
                    .renderer_stats_logs()
                    .into_iter()
                    .for_each(|(session_id, message)| {
                        notify_log(
                            &state,
                            session_id,
                            LOG_LEVEL_INFO,
                            "renderer_stats",
                            &message,
                        );
                    });
            }

            thread::sleep(RUNNING_HEARTBEAT_INTERVAL);
        }
    }

    fn handle_init_frame(frame: DecodedFrame, state: &Arc<HostState>) -> Result<Vec<u8>, String> {
        if frame.frame_type != FRAME_INIT {
            return Err("expected init frame before requests".to_string());
        }

        let (protocol_name, version) = decode_init_payload(&frame.payload)?;

        if protocol_name != PROTOCOL_NAME {
            return Err(format!("unexpected protocol name {protocol_name}"));
        }

        if version != PROTOCOL_VERSION {
            return Err(format!("unsupported macOS host protocol version {version}"));
        }

        Ok(encode_frame(
            FRAME_INIT_OK,
            0,
            0,
            0,
            &encode_init_ok_payload(state.host_id, std::process::id()),
        ))
    }

    fn dispatch_request(frame: DecodedFrame, command_tx: &Sender<HostCommand>) -> Vec<u8> {
        if frame.frame_type != FRAME_REQUEST {
            return encode_frame(
                FRAME_ERROR,
                frame.request_id,
                frame.session_id,
                frame.tag,
                b"expected request frame",
            );
        }

        let reply = match frame.tag {
            REQUEST_START_SESSION => {
                let Some((
                    title,
                    width,
                    height,
                    scroll_line_pixels,
                    renderer_stats_log,
                    renderer_cache_config,
                    macos_backend,
                    asset_config,
                )) = decode_start_session(&frame.payload)
                else {
                    return encode_frame(
                        FRAME_ERROR,
                        frame.request_id,
                        frame.session_id,
                        frame.tag,
                        b"invalid start_session payload",
                    );
                };

                roundtrip(command_tx, |reply_tx| HostCommand::StartSession {
                    title,
                    width,
                    height,
                    scroll_line_pixels,
                    renderer_stats_log,
                    renderer_cache_config,
                    macos_backend,
                    asset_config,
                    reply_tx,
                })
            }
            REQUEST_STOP_SESSION => roundtrip(command_tx, |reply_tx| HostCommand::StopSession {
                session_id: frame.session_id,
                reply_tx,
            }),
            REQUEST_SESSION_RUNNING => {
                roundtrip(command_tx, |reply_tx| HostCommand::SessionRunning {
                    session_id: frame.session_id,
                    reply_tx,
                })
            }
            REQUEST_UPLOAD_TREE => enqueue_immediate(
                command_tx,
                HostCommand::UploadTree {
                    session_id: frame.session_id,
                    bytes: frame.payload,
                },
                HostReply::UploadTree,
            ),
            REQUEST_PATCH_TREE => enqueue_immediate(
                command_tx,
                HostCommand::PatchTree {
                    session_id: frame.session_id,
                    bytes: frame.payload,
                },
                HostReply::PatchTree,
            ),
            REQUEST_SET_INPUT_MASK => {
                if frame.payload.len() != 4 {
                    return encode_frame(
                        FRAME_ERROR,
                        frame.request_id,
                        frame.session_id,
                        frame.tag,
                        b"invalid set_input_mask payload",
                    );
                }

                let mask = u32::from_be_bytes(frame.payload.as_slice().try_into().unwrap());
                roundtrip(command_tx, |reply_tx| HostCommand::SetInputMask {
                    session_id: frame.session_id,
                    mask,
                    reply_tx,
                })
            }
            REQUEST_MEASURE_TEXT => {
                let Some((text, font_size)) = decode_measure_text(&frame.payload) else {
                    return encode_frame(
                        FRAME_ERROR,
                        frame.request_id,
                        frame.session_id,
                        frame.tag,
                        b"invalid measure_text payload",
                    );
                };

                roundtrip(command_tx, |reply_tx| HostCommand::MeasureText {
                    text,
                    font_size,
                    reply_tx,
                })
            }
            REQUEST_LOAD_FONT => {
                let Some((family, weight, italic, data)) = decode_load_font(&frame.payload) else {
                    return encode_frame(
                        FRAME_ERROR,
                        frame.request_id,
                        frame.session_id,
                        frame.tag,
                        b"invalid load_font payload",
                    );
                };

                roundtrip(command_tx, |reply_tx| HostCommand::LoadFont {
                    family,
                    weight,
                    italic,
                    data,
                    reply_tx,
                })
            }
            REQUEST_CONFIGURE_ASSETS => {
                let mut cursor = 0;
                let Some(asset_config) = decode_asset_config(&frame.payload, &mut cursor) else {
                    return encode_frame(
                        FRAME_ERROR,
                        frame.request_id,
                        frame.session_id,
                        frame.tag,
                        b"invalid configure_assets payload",
                    );
                };

                if cursor != frame.payload.len() {
                    return encode_frame(
                        FRAME_ERROR,
                        frame.request_id,
                        frame.session_id,
                        frame.tag,
                        b"invalid configure_assets payload",
                    );
                }

                roundtrip(command_tx, |reply_tx| HostCommand::ConfigureAssets {
                    session_id: frame.session_id,
                    asset_config,
                    reply_tx,
                })
            }
            REQUEST_RENDER_TREE_TO_PIXELS => {
                let Some((bytes, opts)) = decode_offscreen_request(&frame.payload) else {
                    return encode_frame(
                        FRAME_ERROR,
                        frame.request_id,
                        frame.session_id,
                        frame.tag,
                        b"invalid render_tree_to_pixels payload",
                    );
                };

                roundtrip(command_tx, |reply_tx| HostCommand::RenderTreeToPixels {
                    bytes,
                    opts,
                    reply_tx,
                })
            }
            REQUEST_RENDER_TREE_TO_PNG => {
                let Some((bytes, opts)) = decode_offscreen_request(&frame.payload) else {
                    return encode_frame(
                        FRAME_ERROR,
                        frame.request_id,
                        frame.session_id,
                        frame.tag,
                        b"invalid render_tree_to_png payload",
                    );
                };

                roundtrip(command_tx, |reply_tx| HostCommand::RenderTreeToPng {
                    bytes,
                    opts,
                    reply_tx,
                })
            }
            REQUEST_SHUTDOWN_HOST => roundtrip(command_tx, |reply_tx| HostCommand::Shutdown {
                reply_tx: Some(reply_tx),
            }),
            _ => HostReply::Error(format!("unknown macOS host request tag {}", frame.tag)),
        };

        encode_reply(frame.request_id, frame.session_id, frame.tag, reply)
    }

    fn roundtrip<F>(command_tx: &Sender<HostCommand>, make_command: F) -> HostReply
    where
        F: FnOnce(std::sync::mpsc::Sender<HostReply>) -> HostCommand,
    {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();

        if command_tx.send(make_command(reply_tx)).is_err() {
            return HostReply::Error("macOS host command queue is closed".to_string());
        }

        match reply_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(reply) => reply,
            Err(_) => HostReply::Error("timed out waiting for macOS host reply".to_string()),
        }
    }

    fn enqueue_immediate(
        command_tx: &Sender<HostCommand>,
        command: HostCommand,
        ok: HostReply,
    ) -> HostReply {
        if command_tx.send(command).is_err() {
            HostReply::Error("macOS host command queue is closed".to_string())
        } else {
            ok
        }
    }

    fn stdin_monitor_thread(command_tx: Sender<HostCommand>) {
        let mut stdin = io::stdin();
        let mut buf = [0_u8; 64];

        loop {
            match stdin.read(&mut buf) {
                Ok(0) => {
                    let _ = command_tx.send(HostCommand::Shutdown { reply_tx: None });
                    break;
                }
                Ok(_) => continue,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    let _ = command_tx.send(HostCommand::Shutdown { reply_tx: None });
                    break;
                }
            }
        }
    }

    fn encode_reply(request_id: u32, session_id: u64, tag: u16, reply: HostReply) -> Vec<u8> {
        match reply {
            HostReply::StartSession {
                session_id,
                macos_backend,
            } => encode_frame(
                FRAME_REPLY,
                request_id,
                session_id,
                tag,
                &[encode_macos_backend(macos_backend)],
            ),
            HostReply::StopSession => encode_frame(FRAME_REPLY, request_id, session_id, tag, &[]),
            HostReply::SessionRunning { running } => encode_frame(
                FRAME_REPLY,
                request_id,
                session_id,
                tag,
                &[if running { 1 } else { 0 }],
            ),
            HostReply::UploadTree => encode_frame(FRAME_REPLY, request_id, session_id, tag, &[]),
            HostReply::PatchTree => encode_frame(FRAME_REPLY, request_id, session_id, tag, &[]),
            HostReply::SetInputMask => encode_frame(FRAME_REPLY, request_id, session_id, tag, &[]),
            HostReply::MeasureText {
                width,
                line_height,
                ascent,
                descent,
            } => encode_frame(
                FRAME_REPLY,
                request_id,
                session_id,
                tag,
                &encode_measure_text_reply(width, line_height, ascent, descent),
            ),
            HostReply::LoadFont => encode_frame(FRAME_REPLY, request_id, session_id, tag, &[]),
            HostReply::ConfigureAssets => {
                encode_frame(FRAME_REPLY, request_id, session_id, tag, &[])
            }
            HostReply::RenderTreeToPixels { data } => encode_frame(
                FRAME_REPLY,
                request_id,
                session_id,
                tag,
                &encode_blob_payload(&data),
            ),
            HostReply::RenderTreeToPng { data } => encode_frame(
                FRAME_REPLY,
                request_id,
                session_id,
                tag,
                &encode_blob_payload(&data),
            ),
            HostReply::Shutdown => encode_frame(FRAME_REPLY, request_id, session_id, tag, &[]),
            HostReply::Error(message) => {
                encode_frame(FRAME_ERROR, request_id, session_id, tag, message.as_bytes())
            }
        }
    }

    struct DecodedFrame {
        frame_type: u8,
        request_id: u32,
        session_id: u64,
        tag: u16,
        payload: Vec<u8>,
    }

    fn encode_frame(
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

    fn decode_frame(frame: &[u8]) -> Result<DecodedFrame, String> {
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

    fn encode_init_ok_payload(host_id: u64, host_pid: u32) -> Vec<u8> {
        let protocol_name = PROTOCOL_NAME.as_bytes();
        let mut out = Vec::with_capacity(2 + protocol_name.len() + 2 + 8 + 4);
        out.extend_from_slice(&(protocol_name.len() as u16).to_be_bytes());
        out.extend_from_slice(protocol_name);
        out.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
        out.extend_from_slice(&host_id.to_be_bytes());
        out.extend_from_slice(&host_pid.to_be_bytes());
        out
    }

    fn decode_init_payload(payload: &[u8]) -> Result<(String, u16), String> {
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

    fn decode_start_session(
        payload: &[u8],
    ) -> Option<(
        String,
        u32,
        u32,
        f32,
        bool,
        RendererCacheConfig,
        RequestedMacosBackend,
        StartSessionAssetConfig,
    )> {
        let mut cursor = 0;
        let title = decode_string(payload, &mut cursor)?;
        let width = decode_u32(payload, &mut cursor)?;
        let height = decode_u32(payload, &mut cursor)?;
        let scroll_line_pixels = decode_f32(payload, &mut cursor)?;
        let renderer_stats_log = decode_u8(payload, &mut cursor)? != 0;
        let renderer_cache_config = decode_renderer_cache_config(payload, &mut cursor)?;
        let backend = decode_requested_macos_backend(decode_u8(payload, &mut cursor)?)?;
        let asset_config = decode_asset_config(payload, &mut cursor)?;
        let fonts = decode_font_list(payload, &mut cursor)?;

        if cursor != payload.len() {
            return None;
        }

        Some((
            title,
            width,
            height,
            scroll_line_pixels,
            renderer_stats_log,
            renderer_cache_config,
            backend,
            StartSessionAssetConfig {
                sources: asset_config.sources,
                runtime_enabled: asset_config.runtime_enabled,
                runtime_allowlist: asset_config.runtime_allowlist,
                runtime_follow_symlinks: asset_config.runtime_follow_symlinks,
                runtime_max_file_size: asset_config.runtime_max_file_size,
                runtime_extensions: asset_config.runtime_extensions,
                fonts,
            },
        ))
    }

    fn decode_renderer_cache_config(
        payload: &[u8],
        cursor: &mut usize,
    ) -> Option<RendererCacheConfig> {
        let max_new_payloads_per_frame = decode_u32(payload, cursor)?;
        let max_entries = usize::try_from(decode_u64(payload, cursor)?).ok()?;
        let max_bytes = decode_u64(payload, cursor)?;
        let max_entry_bytes = decode_u64(payload, cursor)?;

        Some(RendererCacheConfig {
            max_new_payloads_per_frame,
            clean_subtree: CleanSubtreeCacheConfig {
                max_entries,
                max_bytes,
                max_entry_bytes,
            },
        })
    }

    fn decode_measure_text(payload: &[u8]) -> Option<(String, f32)> {
        let mut cursor = 0;
        let text = decode_string(payload, &mut cursor)?;
        let font_size = decode_f32(payload, &mut cursor)?;

        if cursor == payload.len() {
            Some((text, font_size))
        } else {
            None
        }
    }

    fn decode_load_font(payload: &[u8]) -> Option<(String, u16, bool, Vec<u8>)> {
        let mut cursor = 0;
        let family = decode_string(payload, &mut cursor)?;
        let weight = decode_u16(payload, &mut cursor)?;
        let italic = decode_u8(payload, &mut cursor)? != 0;
        let data = decode_blob(payload, &mut cursor)?;

        if cursor == payload.len() {
            Some((family, weight, italic, data))
        } else {
            None
        }
    }

    fn decode_asset_config(payload: &[u8], cursor: &mut usize) -> Option<AssetConfig> {
        Some(AssetConfig {
            sources: decode_string_list(payload, cursor)?,
            runtime_enabled: decode_u8(payload, cursor)? != 0,
            runtime_allowlist: decode_string_list(payload, cursor)?,
            runtime_follow_symlinks: decode_u8(payload, cursor)? != 0,
            runtime_max_file_size: decode_u64(payload, cursor)?,
            runtime_extensions: decode_string_list(payload, cursor)?,
        })
    }

    fn decode_offscreen_request(payload: &[u8]) -> Option<(Vec<u8>, OffscreenRenderOptions)> {
        let mut cursor = 0;
        let bytes = decode_blob(payload, &mut cursor)?;
        let width = decode_u32(payload, &mut cursor)?;
        let height = decode_u32(payload, &mut cursor)?;
        let scale = decode_f32(payload, &mut cursor)?;
        let asset_mode = match decode_u8(payload, &mut cursor)? {
            ASSET_MODE_AWAIT => "await".to_string(),
            ASSET_MODE_SNAPSHOT => "snapshot".to_string(),
            _ => return None,
        };
        let asset_timeout_ms = decode_u32(payload, &mut cursor)? as u64;
        let asset_config = decode_asset_config(payload, &mut cursor)?;

        if cursor == payload.len() {
            Some((
                bytes,
                OffscreenRenderOptions {
                    width,
                    height,
                    scale,
                    asset_mode,
                    asset_timeout_ms,
                    asset_config,
                },
            ))
        } else {
            None
        }
    }

    fn decode_font_list(payload: &[u8], cursor: &mut usize) -> Option<Vec<HostFontSpec>> {
        let count = decode_u32(payload, cursor)? as usize;
        let mut fonts = Vec::with_capacity(count);

        for _ in 0..count {
            fonts.push(HostFontSpec {
                family: decode_string(payload, cursor)?,
                path: decode_string(payload, cursor)?,
                weight: decode_u16(payload, cursor)?,
                italic: decode_u8(payload, cursor)? != 0,
            });
        }

        Some(fonts)
    }

    fn decode_string_list(payload: &[u8], cursor: &mut usize) -> Option<Vec<String>> {
        let count = decode_u32(payload, cursor)? as usize;
        let mut items = Vec::with_capacity(count);

        for _ in 0..count {
            items.push(decode_string(payload, cursor)?);
        }

        Some(items)
    }

    fn decode_string(payload: &[u8], cursor: &mut usize) -> Option<String> {
        let len = decode_u32(payload, cursor)? as usize;
        let end = (*cursor).checked_add(len)?;
        let bytes = payload.get(*cursor..end)?;
        *cursor = end;
        String::from_utf8(bytes.to_vec()).ok()
    }

    fn decode_blob(payload: &[u8], cursor: &mut usize) -> Option<Vec<u8>> {
        let len = decode_u32(payload, cursor)? as usize;
        let end = (*cursor).checked_add(len)?;
        let bytes = payload.get(*cursor..end)?;
        *cursor = end;
        Some(bytes.to_vec())
    }

    fn decode_u8(payload: &[u8], cursor: &mut usize) -> Option<u8> {
        let value = *payload.get(*cursor)?;
        *cursor += 1;
        Some(value)
    }

    fn decode_u16(payload: &[u8], cursor: &mut usize) -> Option<u16> {
        let end = (*cursor).checked_add(2)?;
        let bytes = payload.get(*cursor..end)?;
        *cursor = end;
        Some(u16::from_be_bytes(bytes.try_into().ok()?))
    }

    fn decode_u32(payload: &[u8], cursor: &mut usize) -> Option<u32> {
        let end = (*cursor).checked_add(4)?;
        let bytes = payload.get(*cursor..end)?;
        *cursor = end;
        Some(u32::from_be_bytes(bytes.try_into().ok()?))
    }

    fn decode_u64(payload: &[u8], cursor: &mut usize) -> Option<u64> {
        let end = (*cursor).checked_add(8)?;
        let bytes = payload.get(*cursor..end)?;
        *cursor = end;
        Some(u64::from_be_bytes(bytes.try_into().ok()?))
    }

    fn decode_f32(payload: &[u8], cursor: &mut usize) -> Option<f32> {
        let end = (*cursor).checked_add(4)?;
        let bytes = payload.get(*cursor..end)?;
        *cursor = end;
        Some(f32::from_bits(u32::from_be_bytes(bytes.try_into().ok()?)))
    }

    fn read_frame(stream: &mut UnixStream) -> io::Result<Vec<u8>> {
        let mut len_buf = [0_u8; 4];
        stream.read_exact(&mut len_buf)?;
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut payload = vec![0_u8; len];
        stream.read_exact(&mut payload)?;
        Ok(payload)
    }

    fn write_frame(stream: &mut UnixStream, payload: &[u8]) -> io::Result<()> {
        stream.write_all(&(payload.len() as u32).to_be_bytes())?;
        stream.write_all(payload)?;
        stream.flush()
    }

    fn host_id() -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        now ^ (std::process::id() as u64)
    }

    fn decode_requested_macos_backend(tag: u8) -> Option<RequestedMacosBackend> {
        match tag {
            MACOS_BACKEND_AUTO => Some(RequestedMacosBackend::Auto),
            MACOS_BACKEND_METAL => Some(RequestedMacosBackend::Metal),
            MACOS_BACKEND_RASTER => Some(RequestedMacosBackend::Raster),
            _ => None,
        }
    }

    fn encode_macos_backend(backend: SelectedMacosBackend) -> u8 {
        match backend {
            SelectedMacosBackend::Metal => MACOS_BACKEND_METAL,
            SelectedMacosBackend::Raster => MACOS_BACKEND_RASTER,
        }
    }

    fn encode_measure_text_reply(
        width: f32,
        line_height: f32,
        ascent: f32,
        descent: f32,
    ) -> Vec<u8> {
        let mut out = Vec::with_capacity(16);
        out.extend_from_slice(&width.to_be_bytes());
        out.extend_from_slice(&line_height.to_be_bytes());
        out.extend_from_slice(&ascent.to_be_bytes());
        out.extend_from_slice(&descent.to_be_bytes());
        out
    }

    fn encode_blob_payload(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + data.len());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(data);
        out
    }
}

#[cfg(all(feature = "macos", target_os = "macos"))]
fn main() {
    if let Err(reason) = app::run() {
        eprintln!("macOS host failed: {reason}");
        std::process::exit(1);
    }
}

#[cfg(not(all(feature = "macos", target_os = "macos")))]
fn main() {
    eprintln!("macos_host can only run on macOS");
    std::process::exit(1);
}

#[cfg(all(test, feature = "macos", target_os = "macos"))]
mod tests {
    use super::app::{
        TextInputSelectorAction, line_scroll_deltas, precise_scroll_deltas,
        render_point_for_view_point, render_size_for_view, should_ignore_text_input_selector,
        should_interpret_key_event, suppress_text_commit_for_key, text_commit_from_key_and_text,
        text_input_selector_action, view_rect_for_render_rect,
    };
    use emerge_skia::events::{TextInputCommandRequest, TextInputEditRequest};
    use emerge_skia::keys::CanonicalKey;

    #[test]
    fn text_commit_suppresses_arrow_keys() {
        assert!(suppress_text_commit_for_key(CanonicalKey::ArrowLeft));
        assert_eq!(
            text_commit_from_key_and_text(CanonicalKey::ArrowLeft, 0, "\u{f702}"),
            None
        );
    }

    #[test]
    fn text_commit_preserves_printable_text() {
        assert_eq!(
            text_commit_from_key_and_text(CanonicalKey::A, 0, "a"),
            Some("a".to_string())
        );
        assert_eq!(
            text_commit_from_key_and_text(CanonicalKey::Space, 0, " "),
            Some(" ".to_string())
        );
    }

    #[test]
    fn text_commit_blocks_meta_and_ctrl_shortcuts() {
        assert_eq!(
            text_commit_from_key_and_text(CanonicalKey::V, 0x02, "v"),
            None
        );
        assert_eq!(
            text_commit_from_key_and_text(CanonicalKey::V, 0x08, "v"),
            None
        );
    }

    #[test]
    fn selector_mapping_routes_commands_and_phase_two_editors() {
        assert_eq!(
            text_input_selector_action("selectAll:"),
            Some(TextInputSelectorAction::Command(
                TextInputCommandRequest::SelectAll,
            ))
        );
        assert_eq!(
            text_input_selector_action("copy:"),
            Some(TextInputSelectorAction::Command(
                TextInputCommandRequest::Copy
            ))
        );
        assert_eq!(
            text_input_selector_action("moveWordLeft:"),
            Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::MoveWordLeft {
                    extend_selection: false,
                },
            ))
        );
        assert_eq!(
            text_input_selector_action("moveToBeginningOfDocumentAndModifySelection:"),
            Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::MoveDocumentStart {
                    extend_selection: true,
                },
            ))
        );
        assert_eq!(
            text_input_selector_action("deleteWordBackward:"),
            Some(TextInputSelectorAction::Edit(
                TextInputEditRequest::DeleteWordBackward,
            ))
        );
    }

    #[test]
    fn selector_mapping_ignores_raw_key_owned_commands() {
        assert!(should_ignore_text_input_selector("moveLeft:"));
        assert!(should_ignore_text_input_selector("deleteBackward:"));
        assert!(should_ignore_text_input_selector("insertNewline:"));
        assert!(!should_ignore_text_input_selector("selectAll:"));
    }

    #[test]
    fn interpret_gate_allows_meta_navigation_but_not_direct_shortcuts() {
        assert!(should_interpret_key_event(CanonicalKey::ArrowLeft, 0x08));
        assert!(should_interpret_key_event(CanonicalKey::Backspace, 0x08));
        assert!(!should_interpret_key_event(CanonicalKey::C, 0x08));
        assert!(!should_interpret_key_event(CanonicalKey::V, 0x08));
        assert!(!should_interpret_key_event(CanonicalKey::ArrowLeft, 0x02));
    }

    #[test]
    fn render_size_scales_view_points_to_drawable_pixels() {
        assert_eq!(render_size_for_view((640, 420), 2.0), (1280, 840));
        assert_eq!(render_size_for_view((401, 301), 1.5), (602, 452));
    }

    #[test]
    fn render_point_scales_view_coordinates_to_render_coordinates() {
        assert_eq!(render_point_for_view_point((12.5, 30.0), 2.0), (25.0, 60.0));
        assert_eq!(render_point_for_view_point((20.0, 40.0), 1.5), (30.0, 60.0));
    }

    #[test]
    fn view_rect_scales_render_coordinates_back_to_view_coordinates() {
        assert_eq!(
            view_rect_for_render_rect((200.0, 100.0, 80.0, 40.0), 2.0),
            (100.0, 50.0, 40.0, 20.0)
        );
    }

    #[test]
    fn precise_scroll_deltas_scale_into_render_pixels() {
        assert_eq!(precise_scroll_deltas(3.0, -4.0, 2.0), (6.0, -8.0));
        assert_eq!(precise_scroll_deltas(3.0, -4.0, 1.0), (3.0, -4.0));
    }

    #[test]
    fn line_scroll_deltas_use_configured_line_distance() {
        assert_eq!(line_scroll_deltas(1.0, -2.0, 45.0), (45.0, -90.0));
    }
}
