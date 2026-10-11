defmodule Emerge.Config.PrimeFrames do
  @moduledoc """
  Leased headless Linux DMA-BUF frames. Use this explicit variant in
  `Emerge.Config.Headless.frames` with a compatible GPU build/device, not CPU
  raster. The recipient must retire the `VideoInterop.Frame` lease before its
  buffer becomes reusable.

  `device` selects the export DRM node; `nil` leaves backend selection automatic.
  `max_in_flight` bounds pool slots. `on_backpressure: :drop_new` drops new output
  when all slots are held rather than allocating an unbounded pool. These fields
  are startup-only; the live cap is `Emerge.Config.Renderer.target_fps`.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `device` | `nil` |
  | `max_in_flight` | `2` |
  | `on_backpressure` | `:drop_new` |
  """
  defstruct device: nil,
            max_in_flight: 2,
            on_backpressure: :drop_new

  @typedoc "Drop a new frame when all output leases are in flight."
  @type backpressure :: :drop_new
  @type t :: %__MODULE__{
          device: String.t() | nil,
          max_in_flight: pos_integer(),
          on_backpressure: backpressure()
        }
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: t()

  @doc false
  def __schema__,
    do: [
      device: {:optional, :path},
      max_in_flight: :positive_integer,
      on_backpressure: {:enum, [:drop_new]}
    ]
end
