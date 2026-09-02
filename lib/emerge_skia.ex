defmodule EmergeSkia do
  @moduledoc """
  Low-level renderer API used by Emerge viewports.

  Most applications should define a viewport with `use Emerge`. Use
  `EmergeSkia` directly when integrating the renderer without the viewport
  process or when using renderer-specific APIs such as capture, statistics, or
  video targets.

  ## Example

      {:ok, renderer} =
        EmergeSkia.start(
          otp_app: :my_app,
          backend: :wayland,
          title: "My App",
          width: 800,
          height: 600
        )

      {_state, _assigned_tree} = EmergeSkia.upload_tree(renderer, tree)
      :ok = EmergeSkia.stop(renderer)

  `start/1` documents renderer and output options. `renderer_info/1` reports the
  backend and rendering API actually selected. `stats/2` and the capture
  functions provide on-demand diagnostics for a running renderer.

  For UI colors, prefer `Emerge.UI.Color`. `rgb/3` and `rgba/4` return packed
  unsigned `0xRRGGBBAA` values for low-level interfaces.

  When an input target is registered with `set_input_target/2`, Wayland close
  requests arrive as `{:emerge_skia_close, :window_close_requested}` and are
  not filtered by the input mask.
  """

  alias EmergeSkia.Assets
  alias EmergeSkia.HeadlessPrimeSession
  alias EmergeSkia.Macos.Renderer
  alias EmergeSkia.Native
  alias EmergeSkia.Options
  alias EmergeSkia.Transport
  alias EmergeSkia.TreeRenderer

  @typedoc "Renderer handle returned by `start/1`."
  @type renderer :: reference() | struct()
  @type color :: non_neg_integer()

  @doc """
  Starts a renderer session.

  `otp_app` is required and identifies the application whose `priv/` directory
  contains logical assets.

      {:ok, renderer} =
        EmergeSkia.start(
          otp_app: :my_app,
          backend: :wayland,
          rendering_api: :auto,
          title: "My App",
          width: 800,
          height: 600
        )

  ## Backend and rendering API

  | Backend/mode | Rendering APIs | Fallback | Capture | Video |
  |---|---|---|---|---|
  | macOS | `:auto`, `:metal`, `:raster` | `:auto` falls back from Metal to raster | No | No |
  | Wayland | `:auto`, `:opengl`, `:raster`, experimental `:vulkan` | `:auto` falls back from OpenGL to raster | Yes | OpenGL/Vulkan when supported |
  | DRM | `:auto`, `:opengl`, `:raster`, experimental `:vulkan` | `:auto` falls back from OpenGL to raster GPU upload | Yes | OpenGL/Vulkan when supported |
  | headless binary | `:auto`, `:opengl`, `:raster` | `:auto` falls back from OpenGL to raster | Yes | No |
  | headless PRIME | `:auto`, `:opengl`, experimental `:vulkan` | None | No | Produces ABGR8888 DMA-BUF frames |

  Explicit Vulkan never falls back. DRM Vulkan requires
  `vulkan_drm_node` in addition to the KMS `drm_card`.

  Raster presentation may be selected explicitly:

      rendering_api: [raster: [present: :cpu]]
      rendering_api: [raster: [present: :gpu_upload]]
      rendering_api: [auto: [raster: [present: :cpu]]]

  Wayland supports `:cpu` and `:gpu_upload`. DRM supports `:gpu_upload`.

  Runtime backends must be compiled into the NIF:

      config :emerge,
        compiled_backends: [:wayland, :drm],
        compiled_vulkan_backends: []

  Headless does not appear in `compiled_backends`. Add `:headless` to
  `compiled_vulkan_backends` only for experimental headless Vulkan PRIME.

  ## Options

  | Option | Default | Description |
  |---|---|---|
  | `otp_app` | required | Application used to resolve logical assets |
  | `backend` | platform default | `:macos`, `:wayland`, `:drm`, or `:headless` |
  | `rendering_api` | `:auto` | Renderer selection described above |
  | `title` | `"Emerge"` | Window title |
  | `width`, `height` | `800`, `600` | Initial window size or fixed headless size |
  | `scroll_line_pixels` | `30.0` | Pixels for one discrete wheel step |
  | `drm_card` | `/dev/dri/card0` | KMS primary-node path |
  | `vulkan_drm_node` | none | Required Vulkan device node for DRM Vulkan |
  | `drm_startup_retries` | `40` | DRM startup retry count |
  | `drm_retry_interval_ms` | `250` | Delay between DRM retries |
  | `drm_force_gpu_finish` | `false` | OpenGL-only DRM diagnostic |
  | `hw_cursor` | `true` | Use a DRM hardware cursor when available |
  | `drm_cursor` | built-in theme | DRM cursor source and hotspot overrides |
  | `input_log` | `false` | Log DRM input-device discovery |
  | `render_log` | `false` | Log detailed renderer and presentation messages |
  | `close_signal_log` | `false` | Log detailed Wayland close handling |
  | `stats` | `false` | Collect renderer stats |
  | `renderer_stats_log` | `false` | Collect and log renderer stats every five seconds |
  | `renderer_animation_log` | `false` | Log Wayland animation cadence |
  | `renderer_cache` | enabled on GPU routes | Paint-layer cache settings |
  | `assets` | restrictive defaults | Asset paths, fonts, decode, and raster cache settings |
  | `headless` | binary defaults | Headless output settings |

  `backend_renderer` and `:gl` remain deprecated aliases. `macos_backend` and
  `dispatch_mode` were removed. See [Migrating to 0.4](0-4.html).

  Asset options are documented in [Use assets](use_assets.html).

  ## Headless options

  Both binary and PRIME modes require a live local `target` PID. The renderer
  sends frames directly to that process.

  | Option | Default | Description |
  |---|---|---|
  | `mode` | `:binary` | `:binary` or `:prime` |
  | `target` | none | Required live local frame recipient PID |
  | `pixel_format` | `:rgba8888` | Binary output format |
  | `bw1_polarity` | `:one_is_black` | `:one_is_black` or `:one_is_white` |
  | `dither` | `false` | Atkinson dithering for raster BW1/Gray2 |
  | `target_fps` | none | Positive requested cadence |
  | `frame_message` | `:emerge_skia_frame` | Frame tuple tag |
  | `prime.drm_node` | none | Optional absolute DRM allocation node |
  | `prime.max_in_flight` | `2` | Maximum unreleased PRIME frames |
  | `prime.on_backpressure` | `:drop_new` | PRIME backpressure policy |

  The target receives `{message_tag, %VideoInterop.Frame{}}`. The default tag is
  `:emerge_skia_frame`; custom tags are delivered as strings. Binary mode uses
  `%VideoInterop.Binary{}` storage with no lease. PRIME mode uses DMA-BUF storage
  with a lease that must be released after the recipient finishes with it.

  Stable binary formats are:

  | Format | Stride | Data |
  |---|---:|---|
  | `:rgba8888` | `width * 4` | Premultiplied RGBA |
  | `:rgb888` | `width * 3` | Premultiplied RGB components with alpha omitted |
  | `:bw1` | `ceil(width / 8)` | Eight MSB-first pixels per byte |
  | `:gray2` | `ceil(width / 4)` | Four MSB-first two-bit pixels per byte |

  BW1 and Gray2 rows are packed independently. Unused low bits in the final byte
  of each row are zero. Gray2 values are `0..3` from black to white. Gray4 is
  unavailable; Gray8 is not a stable 0.4 output contract.

  Grayscale output is composited over white before quantization. Dithering is
  available only with headless raster binary BW1/Gray2. Text, SVG/vector
  coverage, borders, and crisp generated UI are protected from error diffusion.

  Validate each received frame before consuming it:

      :ok = VideoInterop.validate(frame)

  ## Renderer cache options

  | Option | Default |
  |---|---:|
  | `enabled` | `true`; explicit raster defaults to `false` |
  | `max_new_payloads_per_frame` | `16` |
  | `paint_layer.max_entries` | `512` |
  | `paint_layer.max_bytes` | `671_088_640` |
  | `paint_layer.max_entry_bytes` | `268_435_456` |
  | `paint_layer.min_visible_before_store` | `1` |
  | `paint_layer.max_stale_frames` | `120` |

  Set an entry or byte limit to zero to prevent new stores for that limit.

  ## DRM cursor options

  `drm_cursor` accepts `default`, `text`, and `pointer` entries:

      drm_cursor: [
        pointer: [source: "cursors/pointer.svg", hotspot: {3, 2}]
      ]

  `source` must be a `.png` or `.svg` logical path, `%Emerge.Assets.Ref{}`, or
  an allowed absolute runtime path. `hotspot` is a pair of non-negative numbers.

  Native logs are delivered to the process that starts the renderer as
  `{:emerge_skia_log, level, source, message}`. Use `set_log_target/2` to change
  the destination.
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
  Stop the renderer and close its window or output session.

  Most sessions return `:ok`. A headless PRIME session returns
  `{:error, reason}` when it cannot complete ownership-safe shutdown. Do not
  treat an error as successful cleanup; stop native video use and cold-restart
  before loading replacement native code.
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

  @doc false
  @spec submit_video_frame(renderer(), atom(), VideoInterop.Frame.t()) ::
          :ok | {:error, term()}
  def submit_video_frame(%Renderer{}, _target, _frame),
    do: {:error, :video_submission_unsupported}

  def submit_video_frame(renderer, target, %VideoInterop.Frame{} = frame) when is_atom(target) do
    renderer = EmergeSkia.Transport.Native.native_renderer(renderer)

    case Native.video_frame_submit(renderer, Atom.to_string(target), frame) do
      {:ok, _receipt} ->
        :ok

      {:error, {receipt, reason}} when receipt in [:caller_owned, :transferred] ->
        {:error, {receipt, reason}}

      {:error, reason} ->
        {:error, {:caller_owned, reason}}
    end
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

      {:ok, pixels} =
        EmergeSkia.render_to_pixels(renderer,
          region: {0, 0, 320, 240},
          pixel_format: :rgba8888,
          timeout: 5_000
        )

  Supported options are `region: {x, y, width, height}`,
  `pixel_format: :rgba8888 | :rgb888`, `timeout`, `scale: 1.0`, and
  `background: :transparent`. macOS and headless PRIME do not support capture.
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

      {:ok, png} = EmergeSkia.render_to_png(renderer, png: [compression: :default])
      File.write!("capture.png", png)

  PNG capture accepts the same region, timeout, scale, and background options
  as `render_to_pixels/2`, plus `png: [compression: :default]`. macOS and
  headless PRIME do not support capture.
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
  or `renderer_stats_log: true` to collect renderer stats.

      {:ok, snapshot} = EmergeSkia.stats(renderer, :peek)
      {:ok, closed_window} = EmergeSkia.stats(renderer, :take)

  Commands are:

  - `:peek` reads the current window without resetting it;
  - `:take` returns and resets the current window;
  - `:reset` discards the current window;
  - `{:configure, %{enabled: boolean}}` changes collection at runtime.

  `:take` reads and resets the current window. On DRM it may return a draining
  error while asynchronous GPU samples finish; retry `:take` to receive the
  exact closed snapshot. The 0.4 stats schema version is `25`.
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

      {:ok, info} = EmergeSkia.renderer_info(renderer)
      info.rendering_api.selected
      info.capabilities.screenshot

  Explicit Vulkan renderers include the physical device that won startup
  selection. When selection used an exact DRM node, `vulkan_device.drm_node`
  contains its path, match field, and major/minor identity. The KMS card is not
  reported as the Vulkan node on split-device systems. Non-Vulkan renderers
  return `vulkan_device: nil`.

  Use `capabilities` to check capture, raster presentation, and PRIME video
  support before starting an optional operation.
  """
  @spec renderer_info(renderer()) :: {:ok, Native.renderer_info()} | {:error, term()}
  def renderer_info(renderer) do
    renderer
    |> Transport.for_renderer()
    |> apply(:renderer_info, [renderer])
  end
end
