defmodule Emerge.Config.BinaryFrames do
  @moduledoc """
  Owned pixel data for headless output. Set this record as
  `Emerge.Config.Headless.frames`. The recipient does not need to release frames.

  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.Headless{
      target: self(),
      frames: %Emerge.Config.BinaryFrames{pixel_format: :bw1, dither: true}
    },
    renderer: %Emerge.Config.Renderer{api: :raster},
    assets: %Emerge.Config.Assets{otp_app: :my_app}
  }
  ```

  ## Fields

  - `pixel_format` — `:rgba8888` (default), `:bgra8888`, `:rgb888`, `:gray8`,
    `:gray2` or `:bw1`. RGBA/BGRA use four bytes per pixel; RGB uses three;
    Gray8 uses one. Gray2 packs four pixels per byte and BW1 packs eight,
    most-significant pixel first in each row. BGRA uses premultiplied alpha.
  - `bw1_polarity` — `:one_is_black` (default) or `:one_is_white`. This chooses
    what a set bit means in BW1 output; it does not affect other formats.
  - `dither` — `false` by default. Set `true` to improve grayscale approximation
    in raster BW1/Gray2 output. Other formats and GPU renderers do not support it.

  Read the frame's stride rather than assuming tightly packed rows.
  All fields require a new viewport to change.
  """
  defstruct pixel_format: :rgba8888,
            bw1_polarity: :one_is_black,
            dither: false

  @typedoc "Frame pixel encoding."
  @type pixel_format :: :rgba8888 | :bgra8888 | :rgb888 | :gray8 | :gray2 | :bw1
  @typedoc "Meaning of a set bit in BW1 output."
  @type polarity :: :one_is_black | :one_is_white
  @type t :: %__MODULE__{
          pixel_format: pixel_format(),
          bw1_polarity: polarity(),
          dither: boolean()
        }
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: t()

  @doc false
  def __schema__,
    do: [
      pixel_format: {:enum, [:rgba8888, :bgra8888, :rgb888, :gray8, :gray2, :bw1]},
      bw1_polarity: {:enum, [:one_is_black, :one_is_white]},
      dither: :boolean
    ]
end
