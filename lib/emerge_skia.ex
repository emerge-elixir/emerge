defmodule EmergeSkia do
  @moduledoc """
  Minimal Skia renderer for the Emerge layout engine.

  This library renders retained Emerge trees through the native Rust layout,
  event, and Skia pipeline.

  ## Example

      # Start renderer
      {:ok, renderer} =
        EmergeSkia.start(
          otp_app: :my_app,
          title: "My App",
          width: 800,
          height: 600
        )

      import Emerge.UI
      import Emerge.UI.Color
      import Emerge.UI.Size
      import Emerge.UI.Space

      tree =
        el(
          [
            width(px(220)),
            height(px(80)),
            Emerge.UI.Background.color(color(:sky, 500)),
            Emerge.UI.Border.rounded(10),
            padding(16),
            Emerge.UI.Font.color(color(:white)),
            Emerge.UI.Font.size(24)
          ],
          text("Hello!")
        )

      {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)

      # Stop when done
      EmergeSkia.stop(renderer)

  ## Color Format

  For `Emerge.UI` styling, prefer `Emerge.UI.Color.color/1..3`,
  `Emerge.UI.Color.color_rgb/3`, and `Emerge.UI.Color.color_rgba/4`.

  `EmergeSkia.rgb/3` and `EmergeSkia.rgba/4` are still available when you need
  packed 32-bit unsigned integers in RGBA format: `0xRRGGBBAA`

  - `0xFF0000FF` = Red (fully opaque)
  - `0x00FF00FF` = Green (fully opaque)
  - `0x0000FFFF` = Blue (fully opaque)
  - `0x00000080` = Black at 50% opacity

  When you register an input target with `set_input_target/2`, Wayland close
  requests are delivered separately as
  `{:emerge_skia_close, :window_close_requested}`. This lifecycle message
  bypasses the input mask so higher-level runtimes can shut down promptly.
  """

  alias EmergeSkia.Assets
  alias EmergeSkia.HeadlessPrimeSession
  alias EmergeSkia.Macos.Renderer
  alias EmergeSkia.Native
  alias EmergeSkia.Options
  alias EmergeSkia.Transport
  alias EmergeSkia.TreeRenderer
  alias EmergeSkia.VideoConsumerSession
  alias EmergeSkia.VideoTarget

  @type renderer :: reference() | Renderer.t() | HeadlessPrimeSession.t()
  @type color :: non_neg_integer()
  @type video_target :: VideoTarget.t()
  @type video_target_info :: %{
          required(:renderer_epoch) => non_neg_integer(),
          required(:target_id) => binary(),
          required(:target_incarnation) => non_neg_integer(),
          required(:active_stream_id) => non_neg_integer() | nil
        }

  @doc """
  Start a new renderer session.

  ## Options

  - `otp_app` - OTP application used to resolve logical assets from its `priv` dir (**required**)
  - `backend` - Backend selection (`:wayland`, `:drm`, `:macos`, or `:headless`). Defaults to `:wayland` for Linux desktop builds, `:macos` on Darwin, and `:drm` for Nerves-style builds. Window/device backends must be present in `config :emerge, compiled_backends: [...]`.
  - `rendering_api` - Rendering API selection (`:auto`, `:opengl`, `:raster`, `:metal`, `:vulkan`, or configured raster/auto forms). Defaults to `:auto`. Explicit Vulkan requires a matching native feature and never falls back; `:auto` remains OpenGL-first. DRM Vulkan additionally fails closed at its output-allocation capability seam until a target probe proves one supported KMS/Vulkan direction. The deprecated `backend_renderer` option and `:gl` value remain accepted. `:raster` is equivalent to `[raster: [present: :auto]]`; Wayland/DRM can force raster presentation with `[raster: [present: :gpu_upload | :cpu]]` or configure auto fallback with `[auto: [raster: [present: ...]]]`. Headless binary `:auto` tries offscreen EGL/OpenGL on Linux, then falls back to raster. Headless PRIME never falls back to raster.
  - `title` - Window title (default: "Emerge")
  - `width` - Window width in pixels (default: 800)
  - `height` - Window height in pixels (default: 600)
  - `scroll_line_pixels` - Pixel distance used for each discrete mouse-wheel line step (default: `30.0`)
  - `drm_card` - KMS/modeset DRM primary-node path (default: `/dev/dri/card0`)
  - `vulkan_drm_node` - Explicit primary/render DRM node used only for exact Vulkan physical-device selection. Required for `backend: :drm, rendering_api: :vulkan`; it is never inferred from `drm_card`.
  - `hw_cursor` - Enable hardware cursor when available (default: true)
  - `drm_cursor` - Optional DRM-only cursor overrides for `default`, `text`, and `pointer`
  - `input_log` - Log DRM input devices on startup (default: false)
  - `render_log` - Log native backend render/present diagnostics, including Wayland present
    and event-runtime traces. On Wayland, also writes an out-of-band watchdog file to
    `/tmp/emerge-wayland-watchdog-<pid>.log` (default: false)
  - `close_signal_log` - Log detailed Wayland window-close diagnostics to stderr (default: false)
  - `stats` - Enable renderer stats collection without periodic logging (default: false)
  - `renderer_stats_log` - Enable renderer stats collection and log all current stat families every 5 seconds, including frame rate, split render timings, split patch-to-present pipeline timing, layout-cache counters, and renderer-cache counters. Slow Wayland render frames also include a scene primitive summary and per-frame renderer-cache counters. Individual DRM GPU timer samples require the separate verbose `render_log` option; their aggregate remains in the periodic stats log. (default: false)
  - `renderer_animation_log` - Log detailed Wayland animation cadence traces. This is intentionally separate from `renderer_stats_log` because continuous animations can produce very noisy frame-by-frame logs. (default: false)
  - `renderer_cache` - Renderer cache limits (optional)
  - `assets` - Asset runtime policy options (optional)

  Native renderer logs are delivered to the process that starts the renderer as
  `{:emerge_skia_log, level, source, message}` messages. Call
  `set_log_target/2` to redirect them.

  `assets` options:
  - `runtime_paths.enabled` (default: `false`)
  - `runtime_paths.allowlist` (default: `[]`)
  - `runtime_paths.follow_symlinks` (default: `false`)
  - `runtime_paths.max_file_size` (default: `25_000_000`)
  - `runtime_paths.extensions` (default image/SVG extension allowlist)
  - `fonts` (default: `[]`)

  `headless` options, used with `backend: :headless`:
  - `target` - Process pid that receives frame messages (required for binary output;
    optional/deprecated for PRIME output, which may start disconnected)
  - `mode` - `:binary` (default) or `:prime` for Linux headless dma-buf output
  - `pixel_format` - `:rgba8888` (default), `:rgb888`, `:gray8`, `:gray4`, `:gray2`, or `:bw1`
  - `bw1_polarity` - `:one_is_black` (default) or `:one_is_white`
  - `target_fps` - Requested animation cadence for retained headless output (optional)
  - `frame_message` - Message tag atom/string (default: `:emerge_skia_frame`)
  - `prime.max_in_flight` - Maximum unreleased PRIME frames (default: `2`)
  - `prime.on_backpressure` - `:drop_new` (default)

  Headless frames are delivered as `{message_tag, frame}` where `frame` is a
  key/value list. Binary frames include `"mode"`, `"sequence"`, `"width"`,
  `"height"`, `"scale"`, `"pixel_format"`, `"stride_bytes"`, `"data"`, and
  `"timestamp_native"`. PRIME frames include a canonical
  `%VideoInterop.Frame{}` under `"dmabuf"`. Its managed lease supports safe
  fan-out with `VideoInterop.retain/2`; release each holder with
  `VideoInterop.release/1`. Direct Emerge connections consume these holders
  without exposing lease messages to applications.

  `renderer_cache` options:
  - `enabled` (default: `true`, GPU backends only)
  - `max_new_payloads_per_frame` (default: `16`)
  - `paint_layer.max_entries` (default: `512`)
  - `paint_layer.max_bytes` (default: `671_088_640`)
  - `paint_layer.max_entry_bytes` (default: `268_435_456`)
  - `paint_layer.min_visible_before_store` (default: `1`)
  - `paint_layer.max_stale_frames` (default: `120`)

  Set a renderer-cache limit to `0` to prevent new stores for that dimension.

  Each `assets.fonts` entry supports:
  - `family` (required)
  - `source` (required, logical path under `<otp_app>/priv` or `%Emerge.Assets.Ref{}`)
  - `weight` (default: `400`)
  - `italic` (default: `false`)

  Each `drm_cursor` entry supports:
  - `source` (required, `.png` or `.svg`; logical path under `<otp_app>/priv`, `%Emerge.Assets.Ref{}`, or an absolute runtime path allowed by `assets.runtime_paths`)
  - `hotspot` (required `{x, y}` tuple; integers and floats are allowed)

  DRM cursor overrides are applied only on the `:drm` backend. Missing icons fall back to
  the built-in `mocu-black-right` theme.

  The DRM backend explicitly requests OpenGL ES 2; no GLES compatibility option is needed.
  GPU timer profiling and PRIME video import are enabled only when their required GL/EGL
  extensions are available. Missing optional capabilities do not stop ordinary DRM UI
  rendering, while unsupported PRIME video target operations return an error.

  Compile-time backend selection is configured separately with
  `config :emerge, compiled_backends: [...]`. If omitted, desktop builds assume
  `[:wayland]` and Nerves-style builds assume `[:drm]`.
  """
  @spec start(keyword()) :: {:ok, renderer()} | {:error, term()}
  def start(opts) when is_list(opts) do
    opts = Options.normalize_start_keyword_opts!(opts)
    asset_config = Assets.normalize_asset_config!(opts)

    native_opts =
      opts
      |> Options.build_start_native_opts!()
      |> Map.merge(Assets.native_start_asset_config(asset_config))
      |> Map.put(:drm_cursor, Assets.normalize_drm_cursor_overrides!(opts))

    if native_opts.drm_cursor != [] and String.downcase(native_opts.backend) == "wayland" do
      raise ArgumentError, "drm_cursor is only supported with backend: :drm"
    end

    native_opts.backend
    |> Transport.for_backend()
    |> apply(:start_session, [native_opts, asset_config])
  end

  @spec start(String.t()) :: no_return()
  def start(_title) do
    raise ArgumentError,
          "EmergeSkia.start/1 with title is no longer supported; use EmergeSkia.start(otp_app: :my_app, title: \"...\")"
  end

  @spec start() :: no_return()
  def start do
    raise ArgumentError,
          "EmergeSkia.start/0 requires explicit otp_app; use EmergeSkia.start(otp_app: :my_app)"
  end

  @spec start(String.t(), non_neg_integer()) :: no_return()
  def start(_title, _width) do
    raise ArgumentError,
          "EmergeSkia.start/2 is no longer supported; use EmergeSkia.start(otp_app: :my_app, title: \"...\", width: ...)"
  end

  @spec start(String.t(), non_neg_integer(), non_neg_integer()) :: no_return()
  def start(_title, _width, _height) do
    raise ArgumentError,
          "EmergeSkia.start/3 is no longer supported; use EmergeSkia.start(otp_app: :my_app, title: \"...\", width: ..., height: ...)"
  end

  @doc """
  Stop the renderer and close the window.
  """
  @spec stop(renderer()) :: :ok | {:error, term()}
  def stop(renderer) do
    renderer
    |> Transport.for_renderer()
    |> apply(:stop_session, [renderer])
  end

  @doc """
  Check if the renderer window is still open.
  """
  @spec running?(renderer()) :: boolean()
  def running?(renderer) do
    renderer
    |> Transport.for_renderer()
    |> apply(:session_running?, [renderer])
  end

  @doc """
  Create a renderer-owned video target.

  V1 supports fixed-size `:prime` targets only on Prime-capable backends
  (`:wayland` and `:drm`).
  """
  @spec video_target(renderer(), keyword()) :: {:ok, video_target()} | {:error, term()}
  def video_target(%Renderer{}, _opts) do
    {:error, "video targets are not supported on the macOS backend for now"}
  end

  def video_target(renderer, opts) when is_list(opts) do
    opts = Keyword.new(opts)
    id = Keyword.get_lazy(opts, :id, fn -> "video-#{System.unique_integer([:positive])}" end)
    width = Keyword.fetch!(opts, :width)
    height = Keyword.fetch!(opts, :height)
    mode = Keyword.get(opts, :mode, :prime)

    if !is_binary(id) do
      raise ArgumentError, "video target id must be a binary"
    end

    if !is_integer(width) or width <= 0 do
      raise ArgumentError, "video target width must be a positive integer"
    end

    if !is_integer(height) or height <= 0 do
      raise ArgumentError, "video target height must be a positive integer"
    end

    if mode != :prime do
      raise ArgumentError, "video target mode must be :prime in v1"
    end

    renderer = EmergeSkia.Transport.Native.native_renderer(renderer)

    case Native.video_target_new(renderer, id, width, height, Atom.to_string(mode)) do
      {:ok, ref} when is_reference(ref) ->
        {:ok, %VideoTarget{id: id, width: width, height: height, mode: mode, ref: ref}}

      ref when is_reference(ref) ->
        {:ok, %VideoTarget{id: id, width: width, height: height, mode: mode, ref: ref}}

      {:error, reason} ->
        {:error, reason}
    end
  end

  @doc """
  Return the target's exact renderer, incarnation, and active stream identity.

  This is a read-only query. A stale target or renderer returns an error.
  """
  @spec video_target_info(video_target()) :: {:ok, video_target_info()} | {:error, term()}
  def video_target_info(%VideoTarget{ref: ref}) do
    Native.video_target_info(ref)
  end

  @doc """
  Transfer a canonical frame to an opened video consumer session.

  This is a consuming call. On every normal return the caller must not release
  the supplied frame. Known caller-owned rejections are released by
  `VideoInterop.consume/2`; native claims are retired by the renderer.
  """
  @spec submit_video_frame(VideoConsumerSession.t(), VideoInterop.Frame.t()) ::
          :ok | {:error, term()}
  def submit_video_frame(%VideoConsumerSession{} = session, %VideoInterop.Frame{} = frame) do
    VideoInterop.consume(session, frame)
  end

  @doc """
  Submit a generic DRM PRIME descriptor to a video target.

  The descriptor map contains `:width`, `:height`, `:format`, `:objects`,
  `:planes`, `:keepalive`, and `:owner_pid`. It may additionally contain the
  required explicit-sync shape `:acquire_fence_fd` set to a borrowed fd or
  `nil`; maps without that key retain legacy implicit synchronization. Objects
  contain `:fd` and an optional `:modifier`; planes contain `:object_index`,
  `:offset`, and `:pitch`. The renderer duplicates every borrowed fd during this
  call and sends
  `{:keepalive, keepalive}` to `owner_pid` after GPU retirement. If the target
  is absent from the current render scene, the frame is released immediately
  without waking or redrawing the backend. NV12 requires two planes; ABGR8888
  requires one plane with at least four bytes per pixel.
  """
  @deprecated "Open a VideoInterop consumer session and use submit_video_frame/2"
  @spec submit_prime(video_target(), map()) :: :ok | {:error, term()}
  def submit_prime(%VideoTarget{mode: :prime, ref: ref}, desc) when is_map(desc) do
    desc =
      Map.update!(desc, :objects, fn objects ->
        Enum.map(objects, &Map.put_new(&1, :modifier, nil))
      end)

    Native.video_target_submit_prime(ref, desc)
    |> normalize_native_ok()
  end

  @doc """
  Upload a full EMRG tree, run layout, and render.

  Window dimensions come from the initial start config and are updated
  automatically when the window is resized (handled on the Rust side).
  """
  @spec upload_tree(renderer(), Emerge.tree()) ::
          {Emerge.Engine.diff_state(), Emerge.tree()}
  def upload_tree(renderer, tree) do
    TreeRenderer.upload_tree(renderer, tree)
  end

  @doc """
  Apply patches for a new tree, run layout, and render.

  Window dimensions come from the initial start config and are updated
  automatically when the window is resized (handled on the Rust side).
  """
  @spec patch_tree(renderer(), Emerge.Engine.diff_state(), Emerge.tree()) ::
          {Emerge.Engine.diff_state(), Emerge.tree()}
  def patch_tree(renderer, state, tree) do
    TreeRenderer.patch_tree(renderer, state, tree)
  end

  @doc """
  Measure text dimensions for layout purposes.

  Returns `{width, line_height, ascent, descent}` where:
  - `width` - Horizontal extent of the text
  - `line_height` - Total line height (ascent + descent)
  - `ascent` - Distance from baseline to top (positive)
  - `descent` - Distance from baseline to bottom (positive)
  """
  @spec measure_text(String.t(), number()) :: {float(), float(), float(), float()}
  def measure_text(text, font_size) do
    Transport.default().measure_text(text, font_size / 1.0)
  end

  # ===========================================================================
  # Font Loading
  # ===========================================================================

  @doc """
  Load a font from a file path.

  The font is registered by name and can be used with `Font.family/1` in elements.
  Load different variants (bold, italic) with separate calls using appropriate weight/italic params.

  ## Parameters
  - `name` - Font family name to register (e.g., "my-font")
  - `weight` - Font weight (100-900, 400=normal, 700=bold)
  - `italic` - Whether this is an italic variant
  - `path` - Path to the TTF font file

  ## Example

      # Load font variants
      :ok = EmergeSkia.load_font_file("my-font", 400, false, "priv/fonts/MyFont-Regular.ttf")
      :ok = EmergeSkia.load_font_file("my-font", 700, false, "priv/fonts/MyFont-Bold.ttf")
      :ok = EmergeSkia.load_font_file("my-font", 400, true, "priv/fonts/MyFont-Italic.ttf")

      # Use in elements
      el([Font.family("my-font"), Font.size(16)], text("Hello"))
      el([Font.family("my-font"), Font.bold()], text("Bold text"))
  """
  @spec load_font_file(String.t(), non_neg_integer(), boolean(), Path.t()) ::
          :ok | {:error, term()}
  def load_font_file(name, weight, italic, path) do
    Assets.load_font_file(name, weight, italic, path)
  end

  # ===========================================================================
  # Screenshot capture
  # ===========================================================================

  @doc """
  Return pixels from the renderer's latest already-presented frame.

  This API captures retained renderer state. It no longer accepts an Emerge tree;
  pass a renderer handle returned by `start/1`.
  """
  @spec render_to_pixels(renderer(), keyword()) :: {:ok, binary()} | {:error, term()}
  def render_to_pixels(renderer, opts \\ [])

  def render_to_pixels(%Renderer{} = renderer, opts) when is_list(opts) do
    capture_pixels(renderer, opts)
  end

  def render_to_pixels(%HeadlessPrimeSession{} = renderer, opts) when is_list(opts) do
    capture_pixels(renderer, opts)
  end

  def render_to_pixels(renderer, opts) when is_reference(renderer) and is_list(opts) do
    capture_pixels(renderer, opts)
  end

  def render_to_pixels(_tree, opts) when is_list(opts) do
    raise ArgumentError,
          "EmergeSkia.render_to_pixels/2 now expects a renderer handle; one-shot tree rendering was removed. Start a renderer, upload the tree, then call EmergeSkia.render_to_pixels(renderer, opts)."
  end

  @doc """
  Return an encoded PNG from the renderer's latest already-presented frame.

  This API captures retained renderer state. It no longer accepts an Emerge tree;
  pass a renderer handle returned by `start/1`.
  """
  @spec render_to_png(renderer(), keyword()) :: {:ok, binary()} | {:error, term()}
  def render_to_png(renderer, opts \\ [])

  def render_to_png(%Renderer{} = renderer, opts) when is_list(opts) do
    capture_png(renderer, opts)
  end

  def render_to_png(%HeadlessPrimeSession{} = renderer, opts) when is_list(opts) do
    capture_png(renderer, opts)
  end

  def render_to_png(renderer, opts) when is_reference(renderer) and is_list(opts) do
    capture_png(renderer, opts)
  end

  def render_to_png(_tree, opts) when is_list(opts) do
    raise ArgumentError,
          "EmergeSkia.render_to_png/2 now expects a renderer handle; one-shot tree rendering was removed. Start a renderer, upload the tree, then call EmergeSkia.render_to_png(renderer, opts)."
  end

  defp capture_pixels(renderer, opts) do
    opts = Options.normalize_screenshot_opts!(opts)

    renderer
    |> Transport.for_renderer()
    |> apply(:capture_pixels, [renderer, opts])
  end

  defp capture_png(renderer, opts) do
    opts = Options.normalize_screenshot_opts!(opts)

    renderer
    |> Transport.for_renderer()
    |> apply(:capture_png, [renderer, opts])
  end

  @doc """
  Convert RGB values to a color integer.

  ## Examples

      iex> EmergeSkia.rgb(255, 0, 0)
      0xFF0000FF

      iex> EmergeSkia.rgb(0, 255, 0)
      0x00FF00FF
  """
  @spec rgb(0..255, 0..255, 0..255) :: color()
  def rgb(r, g, b) do
    rgba(r, g, b, 255)
  end

  @doc """
  Convert RGBA values to a color integer.

  ## Examples

      iex> EmergeSkia.rgba(255, 0, 0, 128)
      0xFF000080

      iex> EmergeSkia.rgba(0, 0, 0, 255)
      0x000000FF
  """
  @spec rgba(0..255, 0..255, 0..255, 0..255) :: color()
  def rgba(r, g, b, a) do
    import Bitwise
    r <<< 24 ||| g <<< 16 ||| b <<< 8 ||| a
  end

  # ===========================================================================
  # Input Handling
  # ===========================================================================

  # Input mask constants
  @input_mask_key 0x01
  @input_mask_codepoint 0x02
  @input_mask_cursor_pos 0x04
  @input_mask_cursor_button 0x08
  @input_mask_cursor_scroll 0x10
  @input_mask_cursor_enter 0x20
  @input_mask_resize 0x40
  @input_mask_focus 0x80
  @input_mask_all 0xFF

  @doc """
  Returns the input mask for key events.
  """
  @spec input_mask_key() :: non_neg_integer()
  def input_mask_key, do: @input_mask_key

  @doc """
  Returns the input mask for text input events.
  """
  @spec input_mask_codepoint() :: non_neg_integer()
  def input_mask_codepoint, do: @input_mask_codepoint

  @doc """
  Returns the input mask for cursor position events.
  """
  @spec input_mask_cursor_pos() :: non_neg_integer()
  def input_mask_cursor_pos, do: @input_mask_cursor_pos

  @doc """
  Returns the input mask for cursor button events.
  """
  @spec input_mask_cursor_button() :: non_neg_integer()
  def input_mask_cursor_button, do: @input_mask_cursor_button

  @doc """
  Returns the input mask for cursor scroll events.
  """
  @spec input_mask_cursor_scroll() :: non_neg_integer()
  def input_mask_cursor_scroll, do: @input_mask_cursor_scroll

  @doc """
  Returns the input mask for cursor enter/exit events.
  """
  @spec input_mask_cursor_enter() :: non_neg_integer()
  def input_mask_cursor_enter, do: @input_mask_cursor_enter

  @doc """
  Returns the input mask for window resize events.
  """
  @spec input_mask_resize() :: non_neg_integer()
  def input_mask_resize, do: @input_mask_resize

  @doc """
  Returns the input mask for window focus events.
  """
  @spec input_mask_focus() :: non_neg_integer()
  def input_mask_focus, do: @input_mask_focus

  @doc """
  Returns the input mask for all events.
  """
  @spec input_mask_all() :: non_neg_integer()
  def input_mask_all, do: @input_mask_all

  @doc """
  Set the input event mask to filter which events are sent.

  Wayland close notifications are always delivered to the input target as
  `{:emerge_skia_close, :window_close_requested}` and are not filtered by this
  mask.

  ## Example

      # Only capture mouse button and key events
      import Bitwise
      mask = EmergeSkia.input_mask_cursor_button() ||| EmergeSkia.input_mask_key()
      EmergeSkia.set_input_mask(renderer, mask)
  """
  @spec set_input_mask(renderer(), non_neg_integer()) :: :ok
  def set_input_mask(renderer, mask) do
    renderer
    |> Transport.for_renderer()
    |> apply(:set_input_mask, [renderer, mask])
  end

  @doc """
  Set the target process to receive renderer events.

  Events are sent directly to the target process as
  `{:emerge_skia_event, event}` messages.

  Raw input event payloads include:

  - `{:cursor_pos, {x, y}}`
  - `{:cursor_button, {button, action, mods, {x, y}}}`
  - `{:cursor_scroll, {{dx, dy}, {x, y}}}`
  - `{:key, {key, action, mods}}`
  - `{:codepoint, {char, mods}}`
  - `{:text_commit, {text, mods}}`
  - `{:text_preedit, {text, cursor}}`
  - `:text_preedit_clear`
  - `{:cursor_entered, entered}`
  - `{:resized, {width, height, scale}}`
  - `{:focused, focused}`

  On Wayland, close notifications are sent separately as:

  - `{:emerge_skia_close, :window_close_requested}`

  This lifecycle message bypasses the input mask so close requests are still
  delivered when other raw input categories are disabled.

  On DRM, raw `{:cursor_pos, {x, y}}` delivery is latest-wins under load so
  pointer motion does not stall rendering. Button, scroll, key, and text events
  remain ordered.

  Element event payloads include:

  - `{id_bin, event_type}`
  - `{id_bin, event_type, payload}`

  where `id_bin` is an opaque element id and `event_type` is an atom such as
  `:press`, `:click`, `:swipe_up`, `:swipe_down`, `:swipe_left`,
  `:swipe_right`, `:change`, `:key_down`, `:key_up`, or `:key_press`.

  Text-input `:change` payloads are binaries. Slider `:change` payloads are
  floats.

  Routed `:key_down`, `:key_up`, and `:key_press` payloads currently carry an
  opaque binding route id used by higher-level runtimes.

  Higher-level runtimes should route element events with
  `Emerge.Engine.lookup_event/3` or `Emerge.Engine.dispatch_event/3`/`4`.

  Where:
  - `button` is an atom like `:left`, `:right`, `:middle`
  - `action` is 0 for release, 1 for press
  - `mods` is a list of modifier atoms like `[:shift, :ctrl]`
  - `key` is a canonical atom like `:escape`, `:enter`, `:a`, `:digit_1`, `:arrow_left`, or `:plus`

  Raw key events stay layout-independent. Text-producing input is delivered separately
  through text commit/preedit events. For example, `Shift+=` reports raw key `:equal`
  with `[:shift]` and still commits the text `"+"`.

  ## Example

      EmergeSkia.set_input_target(renderer, self())

      receive do
        {:emerge_skia_event, {:cursor_button, {button, 1, _mods, {x, y}}}} ->
          IO.puts("Clicked \#{button} at \#{x}, \#{y}")

        {:emerge_skia_event, {:key, {key, 1, _mods}}} ->
          IO.puts("Key pressed: \#{key}")
      end
  """
  @spec set_input_target(renderer(), pid() | nil) :: :ok
  def set_input_target(renderer, pid) do
    renderer
    |> Transport.for_renderer()
    |> apply(:set_input_target, [renderer, pid])
  end

  @doc """
  Set the target process to receive native renderer log messages.

  Native logs are sent directly to the target process as
  `{:emerge_skia_log, level, source, message}` messages.
  """
  @spec set_log_target(renderer(), pid() | nil) :: :ok
  def set_log_target(renderer, pid) do
    renderer
    |> Transport.for_renderer()
    |> apply(:set_log_target, [renderer, pid])
  end

  @doc """
  Fetch renderer stats.

  Stats collection is disabled by default. Start the renderer with `stats: true`
  or `renderer_stats_log: true` to collect renderer stats. Use `:take` to read
  and reset the current stats window. On DRM, `:take` starts the next stats window
  while asynchronous GPU samples finish in the closed window and may return a
  draining error; retry it to receive the exact closed snapshot.
  """
  @spec stats(renderer(), Native.stats_command()) ::
          {:ok, Native.stats_snapshot()} | {:error, term()}
  def stats(renderer, command \\ :peek) do
    renderer
    |> Transport.for_renderer()
    |> apply(:stats, [renderer, command])
  end

  @doc """
  Fetch normalized renderer/backend information for a running renderer.

  Explicit Vulkan renderers include `:vulkan_device`, retained from the physical device that
  actually won startup selection. When a backend selected Vulkan through an exact DRM node, the
  nested `:drm_node` contains that opened path, match field, and major/minor identity. The KMS card
  is intentionally not reported as the Vulkan node on split-device systems. Non-Vulkan renderers
  return `vulkan_device: nil`.
  """
  @spec renderer_info(renderer()) :: {:ok, Native.renderer_info()} | {:error, term()}
  def renderer_info(renderer) do
    renderer
    |> Transport.for_renderer()
    |> apply(:renderer_info, [renderer])
  end

  defp normalize_native_ok({:ok, _}), do: :ok
  defp normalize_native_ok({:error, reason}), do: {:error, reason}
end
