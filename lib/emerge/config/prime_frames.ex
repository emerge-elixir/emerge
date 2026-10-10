defmodule Emerge.Config.PrimeFrames do
  @moduledoc """
  Leased headless DMA-BUF frames. The recipient must release leases.

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
