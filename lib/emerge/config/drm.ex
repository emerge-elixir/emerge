defmodule Emerge.Config.DRM do
  @moduledoc """
  Direct DRM output. `display: :off` requests no scanout/illumination. Connector and mode selections are exact when provided; brightness is retained while off. `preferred_size` is only a mode-selection hint.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `card` | `"/dev/dri/card0"` |
  | `connector` | `nil` |
  | `mode` | `nil` |
  | `preferred_size` | `nil` |
  | `display` | `:on` |
  | `backlight` | `%Emerge.Config.Backlight{}` |
  """
  defstruct card: "/dev/dri/card0",
            connector: nil,
            mode: nil,
            preferred_size: nil,
            display: :on,
            backlight: %Emerge.Config.Backlight{}

  @typedoc "Desired display state, not application or system power state."
  @type display :: :on | :off
  @type t :: %__MODULE__{
          card: String.t(),
          connector: String.t() | nil,
          mode: String.t() | map() | nil,
          preferred_size: {pos_integer(), pos_integer()} | nil,
          display: display(),
          backlight: Emerge.Config.Backlight.t() | Emerge.Config.Backlight.options()
        }
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: %__MODULE__{
          card: String.t(),
          connector: String.t() | nil,
          mode: String.t() | map() | nil,
          preferred_size: {pos_integer(), pos_integer()} | nil,
          display: display(),
          backlight: Emerge.Config.Backlight.normalized_t()
        }

  @doc false
  def __schema__,
    do: [
      card: :path,
      connector: {:optional, :non_empty_string},
      mode: :mode,
      preferred_size: {:optional, :size},
      display: {:enum, [:on, :off]},
      backlight: {:record, Emerge.Config.Backlight}
    ]
end
