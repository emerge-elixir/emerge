defmodule Emerge.Config.Backlight do
  @moduledoc """
  DRM backlight binding and requested brightness for explicit OpenGL or CPU raster.
  `device: :auto` requires a proven, unambiguous connector association; `nil` opts
  out, and an absolute sysfs device path binds explicitly. Device selection is
  startup-only. Missing automatic association permits KMS-only output unless
  brightness is requested; ambiguity or unavailable explicit devices report errors.

  `brightness` is a live, finite `0..1` request, quantized to the device's integer
  range. `nil` restores the original requested brightness captured before darkening
  and preserved across viewport renderer recovery, not `actual_brightness` readback.
  Changes while off retain intent without illumination. Zero is not display-off;
  use `Emerge.Config.DRM.display` for power. Clean stop leaves a managed lamp dark.
  Reservations protect against other Emerge owners, not external sysfs writers.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `device` | `:auto` |
  | `brightness` | `nil` |
  """
  defstruct device: :auto,
            brightness: nil

  @typedoc "Automatic association, no management, or an absolute sysfs backlight device path."
  @type device :: :auto | nil | String.t()
  @type t :: %__MODULE__{device: device(), brightness: number() | nil}
  @typedoc "Typed keyword shorthand accepted in this record's parent field."
  @type options :: [
          {:device, device()}
          | {:brightness, number() | nil}
        ]
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: t()

  @doc false
  def __schema__,
    do: [device: {:optional, {:one_of, [:auto, :path]}}, brightness: {:optional, {:range, 0, 1}}]
end
