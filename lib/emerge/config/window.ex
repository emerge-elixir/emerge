defmodule Emerge.Config.Window do
  @moduledoc """
  A desktop window. Set this record as `Emerge.Config.output`.

  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.Window{title: "My App", size: {1024, 768}}
  }
  ```

  ## Fields

  - `backend` — `:auto` (default), `:wayland` or `:macos`. Automatic selection uses
    the platform's window backend. This is separate from the drawing API in
    `Emerge.Config.Renderer`.
  - `title` — window title; defaults to `"Emerge"`.
  - `size` — initial `{width, height}` in positive integer surface pixels;
    defaults to `{800, 600}`. The window manager can choose a different size.
  - `always_on_top` — macOS floating-window preference; defaults to `false`.
    It has no effect on Wayland.

  ## Changing settings

  `title` can change through the viewport's `config` callback on Wayland and macOS.
  Changing the other fields requires starting a new viewport. `size` is not a
  command to resize an existing window.
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
