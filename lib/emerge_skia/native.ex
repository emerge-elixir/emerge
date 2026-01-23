defmodule EmergeSkia.Native do
  @moduledoc """
  NIF bindings for the Skia renderer.
  """

  use Rustler,
    otp_app: :emerge_skia,
    crate: "emerge_skia",
    path: "native/emerge_skia"

  @doc """
  Start the Skia renderer with a window.

  Returns a renderer resource that can be used with other functions.
  """
  @spec start(String.t(), non_neg_integer(), non_neg_integer()) :: {:ok, reference()} | {:error, term()}
  def start(_title, _width, _height), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Stop the renderer and close the window.
  """
  @spec stop(reference()) :: :ok
  def stop(_renderer), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Render a list of draw commands.

  Commands are tagged tuples like:
  - `{:rect, x, y, w, h, fill_color}`
  - `{:rounded_rect, x, y, w, h, radius, fill_color}`
  - `{:border, x, y, w, h, radius, width, color}`
  - `{:text, x, y, text, font_size, fill_color}`
  - `{:gradient, x, y, w, h, from_color, to_color, angle}`
  - `{:push_clip, x, y, w, h}`
  - `:pop_clip`
  - `{:translate, x, y}`
  - `:save`
  - `:restore`
  """
  @spec render(reference(), list()) :: :ok
  def render(_renderer, _commands), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Measure text dimensions.

  Returns `{width, line_height, ascent, descent}`.
  """
  @spec measure_text(String.t(), float()) :: {float(), float(), float(), float()}
  def measure_text(_text, _font_size), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Check if the renderer is still running.
  """
  @spec is_running(reference()) :: boolean()
  def is_running(_renderer), do: :erlang.nif_error(:nif_not_loaded)

  # ===========================================================================
  # Raster Backend
  # ===========================================================================

  @doc """
  Render commands to an RGBA pixel buffer (synchronous, no window).

  Returns a binary containing RGBA pixel data (4 bytes per pixel, row-major).
  The binary size is `width * height * 4` bytes.

  Useful for testing, headless rendering, and image generation.
  """
  @spec render_to_pixels(non_neg_integer(), non_neg_integer(), list()) :: binary()
  def render_to_pixels(_width, _height, _commands), do: :erlang.nif_error(:nif_not_loaded)

  # ===========================================================================
  # Input Handling
  # ===========================================================================

  @doc """
  Set the input event mask to filter which events are sent.

  Mask bits:
  - 0x01: Key events
  - 0x02: Codepoint (text input) events
  - 0x04: Cursor position events
  - 0x08: Cursor button events
  - 0x10: Cursor scroll events
  - 0x20: Cursor enter/exit events
  - 0x40: Resize events
  - 0x80: Focus events
  - 0xFF: All events
  """
  @spec set_input_mask(reference(), non_neg_integer()) :: :ok
  def set_input_mask(_renderer, _mask), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Set the target process to receive input events.

  Input events are sent directly to the target process as
  `{:emerge_skia_event, event}` messages.
  """
  @spec set_input_target(reference(), pid() | nil) :: :ok
  def set_input_target(_renderer, _pid), do: :erlang.nif_error(:nif_not_loaded)
end
