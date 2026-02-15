defmodule EmergeSkia do
  @moduledoc """
  Minimal Skia renderer for Emerge layout engine.

  This library provides direct Skia rendering without the overhead of Scenic.
  It exposes a simple command-based API optimized for Emerge's needs:

  - Rectangles (solid and rounded)
  - Text rendering
  - Linear gradients
  - Clipping (for scroll containers)
  - Transform stack (save/restore)

  ## Example

      # Start renderer
      {:ok, renderer} = EmergeSkia.start("My App", 800, 600)

      # Render commands
      EmergeSkia.render(renderer, [
        {:rect, 0, 0, 800, 600, 0xFFFFFFFF},           # White background
        {:rounded_rect, 50, 50, 200, 100, 8, 0x3366FFFF}, # Blue rounded rect
        {:text, 60, 110, "Hello!", 24.0, 0xFFFFFFFF},  # White text
      ])

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

  @type renderer :: reference()
  @type color :: non_neg_integer()

  @doc """
  Start a new renderer window.

  ## Options

  - `backend` - Backend selection (`:wayland` or `:drm`, default: `:wayland`)
  - `title` - Window title (default: "Emerge")
  - `width` - Window width in pixels (default: 800)
  - `height` - Window height in pixels (default: 600)
  - `drm_card` - DRM device path (default: `/dev/dri/card0`)
  - `hw_cursor` - Enable hardware cursor when available (default: true)
  - `input_log` - Log DRM input devices on startup (default: false)
  - `render_log` - Log DRM render/present diagnostics (default: false)
  """
  @spec start(keyword()) :: {:ok, renderer()} | {:error, term()}
  def start(opts) when is_list(opts) do
    if Keyword.keyword?(opts) do
      opts = Keyword.new(opts)
      backend = Keyword.get(opts, :backend, :wayland)
      title = Keyword.get(opts, :title, "Emerge")
      width = Keyword.get(opts, :width, 800)
      height = Keyword.get(opts, :height, 600)
      drm_card = Keyword.get(opts, :drm_card)
      hw_cursor = Keyword.get(opts, :hw_cursor, true)
      input_log = Keyword.get(opts, :input_log, false)
      render_log = Keyword.get(opts, :render_log, false)

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
          :ok = configure_assets_for_renderer(ref)
          {:ok, ref}

        error ->
          {:error, error}
      end
    else
      start(List.to_string(opts), 800, 600)
    end
  end

  @spec start(String.t()) :: {:ok, renderer()} | {:error, term()}
  def start(title) when is_binary(title) do
    start(title, 800, 600)
  end

  @spec start() :: {:ok, renderer()} | {:error, term()}
  def start do
    start("Emerge", 800, 600)
  end

  @spec start(String.t(), non_neg_integer()) :: {:ok, renderer()} | {:error, term()}
  def start(title, width) when is_binary(title) and is_integer(width) do
    start(title, width, 600)
  end

  @spec start(String.t(), non_neg_integer(), non_neg_integer()) ::
          {:ok, renderer()} | {:error, term()}
  def start(title, width, height)
      when is_binary(title) and is_integer(width) and is_integer(height) do
    case Native.start(title, width, height) do
      ref when is_reference(ref) ->
        :ok = configure_assets_for_renderer(ref)
        {:ok, ref}

      error ->
        {:error, error}
    end
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
  Render a list of draw commands to the window.

  ## Commands

  - `{:clear, color}` - Clear with color
  - `{:rect, x, y, w, h, fill}` - Filled rectangle
  - `{:rounded_rect, x, y, w, h, radius, fill}` - Rounded rectangle
  - `{:border, x, y, w, h, radius, width, color}` - Rectangle border/stroke
  - `{:text, x, y, text, font_size, fill}` - Text at baseline position
  - `{:gradient, x, y, w, h, from, to, angle}` - Linear gradient rectangle
  - `{:push_clip, x, y, w, h}` - Push clipping rectangle (also saves state)
  - `:pop_clip` - Pop clipping (also restores state)
  - `{:translate, x, y}` - Translate subsequent drawing
  - `:save` - Save canvas state
  - `:restore` - Restore canvas state
  """
  @spec render(renderer(), list()) :: :ok
  def render(renderer, commands) do
    Native.render(renderer, commands)
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
      :ok = EmergeSkia.load_font_file("my-font", 400, false, "assets/fonts/MyFont-Regular.ttf")
      :ok = EmergeSkia.load_font_file("my-font", 700, false, "assets/fonts/MyFont-Bold.ttf")
      :ok = EmergeSkia.load_font_file("my-font", 400, true, "assets/fonts/MyFont-Italic.ttf")

      # Use in elements
      el([Font.family("my-font"), Font.size(16)], text("Hello"))
      el([Font.family("my-font"), Font.bold()], text("Bold text"))
  """
  @spec load_font_file(String.t(), non_neg_integer(), boolean(), Path.t()) ::
          :ok | {:error, term()}
  def load_font_file(name, weight, italic, path) do
    case File.read(path) do
      {:ok, data} -> Native.load_font_nif(name, weight, italic, data)
      {:error, reason} -> {:error, reason}
    end
  end

  # ===========================================================================
  # Raster Backend (Offscreen Rendering)
  # ===========================================================================

  @doc """
  Render commands to an RGBA pixel buffer (synchronous, no window).

  This is useful for testing, headless rendering, and generating images.
  Each call creates a fresh CPU surface, renders, and returns the pixels.

  Returns a binary containing RGBA pixel data (4 bytes per pixel, row-major order).
  The binary size is `width * height * 4` bytes.

  ## Example

      pixels = EmergeSkia.render_to_pixels(100, 100, [
        {:rect, 0, 0, 100, 100, 0xFF0000FF},  # Red background
        {:text, 10, 50, "Hi", 24.0, 0xFFFFFFFF}
      ])
      # pixels is 100 * 100 * 4 = 40000 bytes
  """
  @spec render_to_pixels(non_neg_integer(), non_neg_integer(), list()) :: binary()
  def render_to_pixels(width, height, commands) do
    Native.render_to_pixels(width, height, commands)
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

  defp configure_assets_for_renderer(renderer) do
    config = Emerge.Assets.Config.fetch()

    manifest_path =
      config
      |> get_in([:manifest, :path])
      |> Path.expand()

    runtime = Map.get(config, :runtime_paths, %{})

    case Native.configure_assets_nif(
           renderer,
           manifest_path,
           Map.get(runtime, :enabled, false),
           Map.get(runtime, :allowlist, []) |> Enum.map(&Path.expand/1),
           Map.get(runtime, :follow_symlinks, false),
           Map.get(runtime, :max_file_size, 25_000_000),
           Map.get(runtime, :extensions, [])
         ) do
      :ok -> :ok
      {:error, reason} -> raise "configure_assets_nif failed: #{inspect(reason)}"
      other -> raise "configure_assets_nif failed: #{inspect(other)}"
    end
  end
end
