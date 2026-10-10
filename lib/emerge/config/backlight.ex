defmodule Emerge.Config.Backlight do
  @moduledoc """
  DRM backlight binding and requested brightness. `device: :auto` requires an unambiguous association; `nil` disables management. `brightness: nil` retains the initial device level. Zero brightness is not display-off.

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
