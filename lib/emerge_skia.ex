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

      tree =
        el(
          [
            width(px(220)),
            height(px(80)),
            Emerge.UI.Background.color(0x3366FFFF),
            Emerge.UI.Border.rounded(10),
            padding(16),
            Emerge.UI.Font.color(0xFFFFFFFF),
            Emerge.UI.Font.size(24)
          ],
          text("Hello!")
        )

      {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)

      # Stop when done
      EmergeSkia.stop(renderer)

  ## Color Format

  Colors are 32-bit unsigned integers in RGBA format: `0xRRGGBBAA`

  - `0xFF0000FF` = Red (fully opaque)
  - `0x00FF00FF` = Green (fully opaque)
  - `0x0000FFFF` = Blue (fully opaque)
  - `0x00000080` = Black at 50% opacity
  """

  alias EmergeSkia.Native
  alias EmergeSkia.VideoTarget

  @type renderer :: reference()
  @type color :: non_neg_integer()
  @type video_target :: VideoTarget.t()

  @default_runtime_extensions [".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp"]
  @default_runtime_max_file_size 25_000_000
  @default_font_extensions [".ttf", ".otf", ".ttc"]
  @default_asset_timeout_ms 30_000

  @doc """
  Start a new renderer window.

  ## Options

  - `otp_app` - OTP application used to resolve logical assets from its `priv` dir (**required**)
  - `backend` - Backend selection (`:wayland` or `:drm`, default: `:wayland`)
  - `title` - Window title (default: "Emerge")
  - `width` - Window width in pixels (default: 800)
  - `height` - Window height in pixels (default: 600)
  - `drm_card` - DRM device path (default: `/dev/dri/card0`)
  - `hw_cursor` - Enable hardware cursor when available (default: true)
  - `input_log` - Log DRM input devices on startup (default: false)
  - `render_log` - Log DRM render/present diagnostics (default: false)
  - `assets` - Asset runtime policy options (optional)

  `assets` options:
  - `runtime_paths.enabled` (default: `false`)
  - `runtime_paths.allowlist` (default: `[]`)
  - `runtime_paths.follow_symlinks` (default: `false`)
  - `runtime_paths.max_file_size` (default: `25_000_000`)
  - `runtime_paths.extensions` (default image extension allowlist)
  - `fonts` (default: `[]`)

  Each `assets.fonts` entry supports:
  - `family` (required)
  - `source` (required, logical path under `<otp_app>/priv` or `%Emerge.Assets.Ref{}`)
  - `weight` (default: `400`)
  - `italic` (default: `false`)
  """
  @spec start(keyword()) :: {:ok, renderer()} | {:error, term()}
  def start(opts) when is_list(opts) do
    if Keyword.keyword?(opts) do
      opts = Keyword.new(opts)
      asset_config = normalize_asset_config!(opts)
      backend = Keyword.get(opts, :backend, :wayland)
      title = Keyword.get(opts, :title, "Emerge")
      width = Keyword.get(opts, :width, 800)
      height = Keyword.get(opts, :height, 600)
      drm_card = Keyword.get(opts, :drm_card)
      hw_cursor = Keyword.get(opts, :hw_cursor, true)
      input_log = Keyword.get(opts, :input_log, false)
      render_log = Keyword.get(opts, :render_log, false)

      if Keyword.has_key?(opts, :dispatch_mode) do
        raise ArgumentError,
              "dispatch_mode option has been removed; EmergeSkia now runs a single dispatch engine"
      end

      backend =
        case backend do
          value when is_atom(value) -> Atom.to_string(value)
          value when is_binary(value) -> value
          _ -> raise ArgumentError, "backend must be an atom or string"
        end

      drm_card = if is_nil(drm_card), do: nil, else: to_string(drm_card)

      case Native.start_opts(%{
             backend: backend,
             title: title,
             width: width,
             height: height,
             drm_card: drm_card,
             hw_cursor: hw_cursor,
             input_log: input_log,
             render_log: render_log
           }) do
        ref when is_reference(ref) ->
          case initialize_renderer_assets(ref, asset_config) do
            :ok ->
              {:ok, ref}

            {:error, reason} ->
              _ = Native.stop(ref)
              {:error, reason}
          end

        error ->
          {:error, error}
      end
    else
      raise ArgumentError,
            "EmergeSkia.start/1 expects a keyword list, for example: EmergeSkia.start(otp_app: :my_app, ...)"
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

  V1 supports fixed-size `:prime` targets only.
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
  @spec upload_tree(renderer(), Emerge.Element.t()) ::
          {Emerge.DiffState.t(), Emerge.Element.t()}
  def upload_tree(renderer, tree) do
    state = Emerge.diff_state_new()
    {full_bin, state, assigned} = Emerge.encode_full(state, tree)

    case Native.renderer_upload(renderer, full_bin) do
      :ok -> :ok
      {:ok, _} -> :ok
      {:error, reason} -> raise "renderer_upload failed: #{reason}"
    end

    {state, assigned}
  end

  @doc """
  Apply patches for a new tree, run layout, and render.

  Window dimensions come from the initial start config and are updated
  automatically when the window is resized (handled on the Rust side).
  """
  @spec patch_tree(renderer(), Emerge.DiffState.t(), Emerge.Element.t()) ::
          {Emerge.DiffState.t(), Emerge.Element.t()}
  def patch_tree(renderer, state, tree) do
    {patch_bin, state, assigned} = Emerge.diff_state_update(state, tree)

    case Native.renderer_patch(renderer, patch_bin) do
      :ok -> :ok
      {:ok, _} -> :ok
      {:error, reason} -> raise "renderer_patch failed: #{reason}"
    end

    {state, assigned}
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
    case File.read(path) do
      {:ok, data} -> normalize_native_ok(Native.load_font_nif(name, weight, italic, data))
      {:error, reason} -> {:error, reason}
    end
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

      pixels =
        EmergeSkia.render_to_pixels(
          el(
            [width(px(100)), height(px(100)), Emerge.UI.Background.color(0xFF0000FF)],
            none()
          ),
          otp_app: :my_app,
          width: 100,
          height: 100
        )

      # pixels is 100 * 100 * 4 = 40000 bytes
  """
  @spec render_to_pixels(Emerge.Element.t(), keyword()) :: binary()
  def render_to_pixels(tree, opts) when is_list(opts) do
    if Keyword.keyword?(opts) do
      opts = Keyword.new(opts)
      asset_config = normalize_asset_config!(opts)
      width = opts |> Keyword.fetch!(:width) |> normalize_positive_integer!(":width")
      height = opts |> Keyword.fetch!(:height) |> normalize_positive_integer!(":height")
      scale = opts |> Keyword.get(:scale, 1.0) |> normalize_positive_number!(":scale")

      asset_mode =
        opts
        |> Keyword.get(:asset_mode, :await)
        |> normalize_asset_mode!()

      asset_timeout_ms =
        opts
        |> Keyword.get(:asset_timeout_ms, @default_asset_timeout_ms)
        |> normalize_positive_integer!(":asset_timeout_ms")

      with :ok <- preload_font_assets(asset_config) do
        state = Emerge.diff_state_new()
        {full_bin, _state, _assigned} = Emerge.encode_full(state, tree)

        case Native.render_tree_to_pixels_nif(
               full_bin,
               width,
               height,
               scale,
               [asset_config.priv_dir],
               asset_config.runtime_enabled,
               asset_config.runtime_allowlist,
               asset_config.runtime_follow_symlinks,
               asset_config.runtime_max_file_size,
               asset_config.runtime_extensions,
               asset_mode,
               asset_timeout_ms
             ) do
          pixels when is_binary(pixels) ->
            pixels

          {:ok, pixels} when is_binary(pixels) ->
            pixels

          {:error, reason} ->
            raise "render_tree_to_pixels failed: #{reason}"

          other ->
            raise "render_tree_to_pixels returned unexpected result: #{inspect(other)}"
        end
      else
        {:error, reason} ->
          raise "render_tree_to_pixels failed: #{inspect(reason)}"
      end
    else
      raise ArgumentError,
            "EmergeSkia.render_to_pixels/2 expects a keyword list, for example: EmergeSkia.render_to_pixels(tree, otp_app: :my_app, width: 800, height: 600)"
    end
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
  Set the target process to receive input events.

  Input events are sent directly to the target process as
  `{:emerge_skia_event, event}` messages where event is one of:

  - `{:cursor_pos, {x, y}}`
  - `{:cursor_button, {button, action, mods, {x, y}}}`
  - `{:cursor_scroll, {{dx, dy}, {x, y}}}`
  - `{:key, {key, action, mods}}`
  - `{:codepoint, {char, mods}}`
  - `{:cursor_entered, entered}`
  - `{:resized, {width, height, scale}}`
  - `{:focused, focused}`

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

  defp initialize_renderer_assets(renderer, asset_config) do
    with :ok <- configure_assets_for_renderer(renderer, asset_config),
         :ok <- preload_font_assets(asset_config) do
      :ok
    end
  end

  defp configure_assets_for_renderer(renderer, asset_config) do
    case Native.configure_assets_nif(
           renderer,
           [asset_config.priv_dir],
           asset_config.runtime_enabled,
           asset_config.runtime_allowlist,
           asset_config.runtime_follow_symlinks,
           asset_config.runtime_max_file_size,
           asset_config.runtime_extensions
         ) do
      :ok -> :ok
      {:error, reason} -> {:error, {:configure_assets_failed, reason}}
      other -> {:error, {:configure_assets_failed, other}}
    end
  end

  defp preload_font_assets(%{fonts: []}), do: :ok

  defp preload_font_assets(%{fonts: fonts, priv_dir: priv_dir}) do
    Enum.reduce_while(fonts, :ok, fn font, :ok ->
      absolute_path = Path.join(priv_dir, font.source)

      case File.read(absolute_path) do
        {:ok, data} ->
          case normalize_native_ok(
                 Native.load_font_nif(font.family, font.weight, font.italic, data)
               ) do
            :ok ->
              {:cont, :ok}

            {:error, reason} ->
              {:halt,
               {:error,
                {:font_asset_load_failed,
                 %{font: font_key(font), source: font.source, reason: reason}}}}
          end

        {:error, reason} ->
          {:halt,
           {:error,
            {:font_asset_read_failed,
             %{font: font_key(font), source: font.source, path: absolute_path, reason: reason}}}}
      end
    end)
  end

  defp normalize_native_ok(:ok), do: :ok
  defp normalize_native_ok({:ok, _}), do: :ok
  defp normalize_native_ok({:error, reason}), do: {:error, reason}
  defp normalize_native_ok(other), do: {:error, {:unexpected_native_result, other}}

  defp normalize_asset_config!(opts) do
    otp_app =
      case Keyword.fetch(opts, :otp_app) do
        {:ok, value} when is_atom(value) ->
          value

        {:ok, value} ->
          raise ArgumentError,
                "otp_app must be an atom, got: #{inspect(value)}"

        :error ->
          raise ArgumentError,
                "missing required :otp_app option; use EmergeSkia.start(otp_app: :my_app, ...)"
      end

    assets_opts =
      opts
      |> Keyword.get(:assets, [])
      |> normalize_keyword_or_map!("assets")

    runtime_opts =
      assets_opts
      |> Keyword.get(:runtime_paths, [])
      |> normalize_keyword_or_map!("assets.runtime_paths")

    runtime_allowlist =
      runtime_opts
      |> Keyword.get(:allowlist, [])
      |> normalize_path_list!("assets.runtime_paths.allowlist")

    runtime_extensions =
      runtime_opts
      |> Keyword.get(:extensions, @default_runtime_extensions)
      |> normalize_string_list!("assets.runtime_paths.extensions")

    fonts =
      assets_opts
      |> Keyword.get(:fonts, [])
      |> normalize_fonts!()

    runtime_max_file_size =
      Keyword.get(runtime_opts, :max_file_size, @default_runtime_max_file_size)

    runtime_enabled = Keyword.get(runtime_opts, :enabled, false)
    runtime_follow_symlinks = Keyword.get(runtime_opts, :follow_symlinks, false)

    if not is_boolean(runtime_enabled) do
      raise ArgumentError, "assets.runtime_paths.enabled must be a boolean"
    end

    if not is_boolean(runtime_follow_symlinks) do
      raise ArgumentError, "assets.runtime_paths.follow_symlinks must be a boolean"
    end

    if not (is_integer(runtime_max_file_size) and runtime_max_file_size > 0) do
      raise ArgumentError, "assets.runtime_paths.max_file_size must be a positive integer"
    end

    %{
      otp_app: otp_app,
      priv_dir: otp_app_priv_dir!(otp_app),
      runtime_enabled: runtime_enabled,
      runtime_allowlist: runtime_allowlist,
      runtime_follow_symlinks: runtime_follow_symlinks,
      runtime_max_file_size: runtime_max_file_size,
      runtime_extensions: runtime_extensions,
      fonts: fonts
    }
  end

  defp normalize_fonts!(fonts) do
    entries = normalize_list!(fonts, "assets.fonts")

    normalized =
      Enum.map(entries, fn entry ->
        opts = normalize_keyword_or_map!(entry, "assets.fonts[]")

        family =
          opts
          |> Keyword.fetch!(:family)
          |> normalize_non_empty_string!("assets.fonts[].family")

        source =
          opts
          |> Keyword.fetch!(:source)
          |> normalize_font_source!()

        weight =
          opts
          |> Keyword.get(:weight, 400)
          |> normalize_font_weight!()

        italic =
          opts
          |> Keyword.get(:italic, false)
          |> normalize_boolean!("assets.fonts[].italic")

        extension = Path.extname(source) |> String.downcase()

        if extension not in @default_font_extensions do
          raise ArgumentError,
                "assets.fonts[].source extension must be one of #{inspect(@default_font_extensions)}, got: #{inspect(source)}"
        end

        %{
          family: family,
          source: source,
          weight: weight,
          italic: italic
        }
      end)

    ensure_unique_fonts!(normalized)
    normalized
  end

  defp normalize_path_list!(list, field_name) do
    strings = normalize_string_list!(list, field_name)
    Enum.map(strings, &Path.expand/1)
  end

  defp normalize_list!(list, _field_name) when is_list(list), do: list

  defp normalize_list!(value, field_name) do
    raise ArgumentError, "#{field_name} must be a list, got: #{inspect(value)}"
  end

  defp normalize_string_list!(list, field_name) do
    if not (is_list(list) and Enum.all?(list, &is_binary/1)) do
      raise ArgumentError, "#{field_name} must be a list of strings"
    end

    list
  end

  defp normalize_non_empty_string!(value, field_name) when is_binary(value) do
    case String.trim(value) do
      "" -> raise ArgumentError, "#{field_name} must not be empty"
      trimmed -> trimmed
    end
  end

  defp normalize_non_empty_string!(value, field_name) do
    raise ArgumentError, "#{field_name} must be a string, got: #{inspect(value)}"
  end

  defp normalize_boolean!(value, _field_name) when is_boolean(value), do: value

  defp normalize_boolean!(value, field_name) do
    raise ArgumentError, "#{field_name} must be a boolean, got: #{inspect(value)}"
  end

  defp normalize_font_weight!(weight) when is_integer(weight) and weight in 100..900, do: weight

  defp normalize_font_weight!(weight) do
    raise ArgumentError,
          "assets.fonts[].weight must be an integer between 100 and 900, got: #{inspect(weight)}"
  end

  defp normalize_font_source!(%Emerge.Assets.Ref{path: path}) when is_binary(path) do
    normalize_logical_source!(path)
  end

  defp normalize_font_source!(path) when is_binary(path) do
    normalize_logical_source!(path)
  end

  defp normalize_font_source!(other) do
    raise ArgumentError,
          "assets.fonts[].source must be a logical string path or %Emerge.Assets.Ref{}, got: #{inspect(other)}"
  end

  defp normalize_logical_source!(path) do
    normalized =
      path
      |> String.trim()
      |> String.trim_leading("/")

    if normalized == "" do
      raise ArgumentError, "assets.fonts[].source must not be empty"
    end

    if Enum.any?(Path.split(normalized), &(&1 == "..")) do
      raise ArgumentError,
            "assets.fonts[].source must be relative and may not contain '..': #{inspect(path)}"
    end

    normalized
  end

  defp ensure_unique_fonts!(fonts) do
    keys = Enum.map(fonts, &font_key/1)
    duplicates = keys -- Enum.uniq(keys)

    if duplicates != [] do
      duplicates = duplicates |> Enum.uniq() |> Enum.map(&inspect/1) |> Enum.join(", ")
      raise ArgumentError, "duplicate assets.fonts entries for variants: #{duplicates}"
    end
  end

  defp font_key(%{family: family, weight: weight, italic: italic}), do: {family, weight, italic}

  defp normalize_keyword_or_map!(value, field_name) do
    cond do
      is_map(value) ->
        Map.to_list(value)

      is_list(value) and Keyword.keyword?(value) ->
        Keyword.new(value)

      true ->
        raise ArgumentError, "#{field_name} must be a keyword list or map, got: #{inspect(value)}"
    end
  end

  defp normalize_positive_integer!(value, _field_name)
       when is_integer(value) and value > 0,
       do: value

  defp normalize_positive_integer!(value, field_name) do
    raise ArgumentError, "#{field_name} must be a positive integer, got: #{inspect(value)}"
  end

  defp normalize_positive_number!(value, _field_name)
       when is_integer(value) and value > 0,
       do: value / 1.0

  defp normalize_positive_number!(value, _field_name)
       when is_float(value) and value > 0.0,
       do: value

  defp normalize_positive_number!(value, field_name) do
    raise ArgumentError, "#{field_name} must be a positive number, got: #{inspect(value)}"
  end

  defp normalize_asset_mode!(:await), do: "await"
  defp normalize_asset_mode!(:snapshot), do: "snapshot"
  defp normalize_asset_mode!("await"), do: "await"
  defp normalize_asset_mode!("snapshot"), do: "snapshot"

  defp normalize_asset_mode!(value) do
    raise ArgumentError,
          ":asset_mode must be :await or :snapshot, got: #{inspect(value)}"
  end

  defp otp_app_priv_dir!(otp_app) do
    case :code.priv_dir(otp_app) do
      path when is_list(path) ->
        List.to_string(path)

      _ ->
        raise ArgumentError,
              "could not resolve priv dir for otp_app #{inspect(otp_app)}; ensure the application is part of your release"
    end
  end
end
