defmodule Emerge.Config.BinaryFrames do
  @moduledoc """
  Owned headless `VideoInterop.Frame` output with no lease. Use this explicit
  variant in `Emerge.Config.Headless.frames`; its fields are startup-only.
  BW1/Gray2 rows are packed MSB-first. `bw1_polarity` chooses the meaning of a set
  bit in BW1. `dither` is supported for raster BW1/Gray2 output, not every format
  or GPU renderer. BGRA8888 is premultiplied B, G, R, A byte order.

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
