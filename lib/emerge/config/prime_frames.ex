defmodule Emerge.Config.PrimeFrames do
  @moduledoc """
  Linux DMA-BUF frames for a headless GPU renderer. Set this record as
  `Emerge.Config.Headless.frames`; CPU raster does not support this output.

  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.Headless{
      target: self(),
      frames: %Emerge.Config.PrimeFrames{device: "/dev/dri/renderD128"}
    },
    renderer: %Emerge.Config.Renderer{api: :opengl},
    assets: %Emerge.Config.Assets{otp_app: :my_app}
  }
  ```

  ## Fields

  - `device` — absolute path to the export DRM device. Defaults to `nil` for
    automatic device selection. The selected device must support the GPU/output
    combination. This is separate from the Vulkan renderer's `device` setting.
  - `max_in_flight` — positive number of output buffers available at once;
    defaults to `2`. Increasing it allows the recipient to hold more frames,
    at the cost of memory.
  - `on_backpressure` — currently only `:drop_new` (default). When every buffer
    is held by a recipient, new frames are dropped rather than growing the pool.

  Each `VideoInterop.Frame` has a lease. Release it through `VideoInterop` when
  finished; holding leases prevents their buffers from being reused.
  All fields require a new viewport to change.
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
