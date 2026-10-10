defmodule Emerge.Config.BinaryFrames do
  @moduledoc """
  Owned headless frames. Dithering requires supported grayscale raster output.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `pixel_format` | `:rgba8888` |
  | `bw1_polarity` | `:one_is_black` |
  | `dither` | `false` |
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
