defmodule Emerge.Config.Backlight do
  @moduledoc """
  The panel backlight managed by a DRM output. Set this record as
  `Emerge.Config.DRM.backlight`; it does not apply to desktop windows.

  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.DRM{
      backlight: %Emerge.Config.Backlight{
        device: "/sys/class/backlight/panel",
        brightness: 0.5
      }
    },
    renderer: %Emerge.Config.Renderer{api: :opengl}
  }
  ```

  Replace the example device path with your panel's actual backlight path.

  ## Fields

  - `device` — `:auto` (default), an absolute sysfs backlight path, or `nil` to
    leave brightness unmanaged. Automatic selection requires a clear association
    with the chosen connector. No associated backlight is acceptable unless you
    request brightness; an ambiguous association is an error.
  - `brightness` — a number from `0` to `1`, rounded to the device's supported
    levels. `0.5` requests half its numeric range, not a perceptual brightness curve.
    The default `nil` uses the original requested brightness. Returning to `nil`
    restores that original value, including after renderer recovery.

  Brightness `0` is **not** display-off. Set `DRM.display` to `:off` to blank the
  display. A brightness change while off is used when the display turns on again.
  Requesting brightness with `device: nil` is invalid.

  ## Changing settings

  Brightness can change through `config` with explicit OpenGL, Vulkan or raster
  (CPU or GPU-upload presentation).
  Changing `device` requires a new viewport. Avoid writing the same backlight
  from another process while Emerge manages it. Stopping a managed display leaves
  its backlight dark.
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
