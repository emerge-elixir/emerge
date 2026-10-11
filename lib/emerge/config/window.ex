defmodule Emerge.Config.Window do
  @moduledoc """
  Desktop window output. `backend: :auto` chooses the platform default (macOS or
  Wayland), independently of `Emerge.Config.Renderer.api`. `size` requests initial
  surface dimensions, not a live resize. `title` is live on both window backends;
  other fields are startup-only. `always_on_top` is a macOS window preference.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `backend` | `:auto` |
  | `title` | `"Emerge"` |
  | `size` | `{800, 600}` |
  | `always_on_top` | `false` |
  """
  defstruct backend: :auto,
            title: "Emerge",
            size: {800, 600},
            always_on_top: false

  @typedoc "Window platform. Automatic selects the current platform."
  @type backend :: :auto | :wayland | :macos
  @type t :: %__MODULE__{
          backend: backend(),
          title: String.t(),
          size: {pos_integer(), pos_integer()},
          always_on_top: boolean()
        }
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: t()

  @doc false
  def __schema__,
    do: [
      backend: {:enum, [:auto, :wayland, :macos]},
      title: :string,
      size: :size,
      always_on_top: :boolean
    ]
end
