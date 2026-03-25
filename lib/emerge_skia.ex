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
  """

  alias EmergeSkia.Assets
  alias EmergeSkia.Native
  alias EmergeSkia.Options
  alias EmergeSkia.TreeRenderer
  alias EmergeSkia.VideoTarget

  @type renderer :: reference()
  @type color :: non_neg_integer()
  @type video_target :: VideoTarget.t()

  @default_asset_timeout_ms 30_000

  @doc """
  Start a new renderer window.

  ## Options

  - `otp_app` - OTP application used to resolve logical assets from its `priv` dir (**required**)
  - `backend` - Backend selection (`:wayland` or `:drm`). Defaults to `:wayland` for desktop builds and `:drm` for Nerves-style builds. The requested backend must also be present in `config :emerge, compiled_backends: [...]`.
  - `title` - Window title (default: "Emerge")
  - `width` - Window width in pixels (default: 800)
  - `height` - Window height in pixels (default: 600)
  - `drm_card` - DRM device path (default: `/dev/dri/card0`)
  - `hw_cursor` - Enable hardware cursor when available (default: true)
  - `input_log` - Log DRM input devices on startup (default: false)
  - `render_log` - Log DRM render/present diagnostics (default: false)
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

  Each `assets.fonts` entry supports:
  - `family` (required)
  - `source` (required, logical path under `<otp_app>/priv` or `%Emerge.Assets.Ref{}`)
  - `weight` (default: `400`)
  - `italic` (default: `false`)

  Compile-time backend selection is configured separately with
  `config :emerge, compiled_backends: [...]`. If omitted, desktop builds assume
  `[:wayland]` and Nerves-style builds assume `[:drm]`.
  """
  @spec start(keyword()) :: {:ok, renderer()} | {:error, term()}
  def start(opts) when is_list(opts) do
    opts = Options.normalize_start_keyword_opts!(opts)
    asset_config = Assets.normalize_asset_config!(opts)

    case Native.start_opts(Options.build_start_native_opts!(opts)) do
      ref when is_reference(ref) ->
        case Assets.initialize_renderer_assets(ref, asset_config) do
          :ok ->
            {:ok, ref}

          {:error, reason} ->
            _ = Native.stop(ref)
            {:error, reason}
        end

      error ->
        {:error, error}
    end
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
  @spec stop(renderer()) :: :ok
  def stop(renderer) do
    Native.stop(renderer)
  end

  @doc """
  Check if the renderer window is still open.
  """
  @spec running?(renderer()) :: boolean()
  def running?(renderer) do
    Native.is_running(renderer)
  end

  @doc """
  Create a renderer-owned video target.

  V1 supports fixed-size `:prime` targets only on Prime-capable backends
  (`:wayland` and `:drm`).
  """
  @spec video_target(renderer(), keyword()) :: {:ok, video_target()} | {:error, term()}
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
  Submit a DRM Prime descriptor to a video target.
  """
  @spec submit_prime(video_target(), map()) :: :ok | {:error, term()}
  def submit_prime(%VideoTarget{mode: :prime, ref: ref}, desc) when is_map(desc) do
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
    Native.measure_text(text, font_size / 1.0)
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
  # Raster Backend (Offscreen Rendering)
  # ===========================================================================

  @doc """
  Render a tree to an RGBA pixel buffer (synchronous, no window).

  This is useful for testing, headless rendering, and image generation.
  Each call creates a fresh CPU surface, runs layout, renders the tree, and
  returns the pixels.

  ## Options

  - `otp_app` - OTP application used to resolve logical assets from its `priv` dir (**required**)
  - `width` - Output width in pixels (**required**)
  - `height` - Output height in pixels (**required**)
  - `scale` - Layout scale factor (default: `1.0`)
  - `assets` - Asset runtime policy options (same shape as `start/1`)
  - `asset_mode` - `:await` to block for asset resolution, or `:snapshot` to capture the current placeholder state (default: `:await`)
  - `asset_timeout_ms` - Maximum wait time for `asset_mode: :await` (default: `#{@default_asset_timeout_ms}`)

  Returns a binary containing RGBA pixel data (4 bytes per pixel, row-major order).
  The binary size is `width * height * 4` bytes.

  ## Example

      import Emerge.UI
      import Emerge.UI.Color
      import Emerge.UI.Size

      pixels =
        EmergeSkia.render_to_pixels(
          el(
            [width(px(100)), height(px(100)), Emerge.UI.Background.color(color(:red, 500))],
            none()
          ),
          otp_app: :my_app,
          width: 100,
          height: 100
        )

      # pixels is 100 * 100 * 4 = 40000 bytes
  """
  @spec render_to_pixels(Emerge.tree(), keyword()) :: binary()
  def render_to_pixels(tree, opts) when is_list(opts) do
    TreeRenderer.render_to_pixels(tree, opts, @default_asset_timeout_ms)
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

  ## Example

      # Only capture mouse button and key events
      import Bitwise
      mask = EmergeSkia.input_mask_cursor_button() ||| EmergeSkia.input_mask_key()
      EmergeSkia.set_input_mask(renderer, mask)
  """
  @spec set_input_mask(renderer(), non_neg_integer()) :: :ok
  def set_input_mask(renderer, mask) do
    Native.set_input_mask(renderer, mask)
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

  On DRM, raw `{:cursor_pos, {x, y}}` delivery is latest-wins under load so
  pointer motion does not stall rendering. Button, scroll, key, and text events
  remain ordered.

  Element event payloads include:

  - `{id_bin, event_type}`
  - `{id_bin, event_type, payload}`

  where `id_bin` is an opaque element id and `event_type` is an atom such as
  `:press`, `:click`, or `:change`.

  Higher-level runtimes should route element events with
  `Emerge.Engine.lookup_event/3` or `Emerge.Engine.dispatch_event/3`/`4`.

  Where:
  - `button` is an atom like `:left`, `:right`, `:middle`
  - `action` is 0 for release, 1 for press
  - `mods` is a list of modifier atoms like `[:shift, :ctrl]`
  - `key` is an atom like `:escape`, `:enter`, or a character atom like `:a`

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
    Native.set_input_target(renderer, pid)
  end

  @doc """
  Set the target process to receive native renderer log messages.

  Native logs are sent directly to the target process as
  `{:emerge_skia_log, level, source, message}` messages.
  """
  @spec set_log_target(renderer(), pid() | nil) :: :ok
  def set_log_target(renderer, pid) do
    Native.set_log_target(renderer, pid)
  end

  defp normalize_native_ok(:ok), do: :ok
  defp normalize_native_ok({:ok, _}), do: :ok
  defp normalize_native_ok({:error, reason}), do: {:error, reason}
end
