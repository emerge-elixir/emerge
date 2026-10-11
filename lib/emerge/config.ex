defmodule Emerge.Config do
  @moduledoc """
  Configuration returned by a viewport's `mount/1` callback, or passed to
  `EmergeSkia.start/1` when using a renderer directly.

  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.Window{title: "My App"},
    renderer: %Emerge.Config.Renderer{api: :raster}
  }
  ```

  ## Groups

  - `output` — `Emerge.Config.Window` (default), `Emerge.Config.DRM` or
    `Emerge.Config.Headless`. Chooses where frames go.
  - `renderer` — `Emerge.Config.Renderer`. Chooses how frames are drawn.
  - `input` — `Emerge.Config.Input`. Raw input subscriptions default to none;
    ordinary UI element interactions still work.
  - `assets` — `Emerge.Config.Assets`. Application asset paths, fonts and images.
  - `diagnostics` — `Emerge.Config.Diagnostics`. Optional measurements and logs.
  - `runtime` — `Emerge.Config.Runtime`. Health checks and advanced renderer options.

  All groups have defaults. Use explicit structs to choose the `output` and
  headless frame variant. Other records also accept keyword shorthand:

  ```elixir
  %Emerge.Config{input: [events: [:key, :resize]]}
  ```

  A viewport infers the asset application where possible. Direct renderer callers
  must supply `assets.otp_app`; headless output also needs a live target PID.

  ## Change settings while running

  An optional `c:Emerge.config/1` or `c:Emerge.config/2` callback receives the previous
  requested config, with all nested records expanded to structs. Return a config
  with the changes you want:

  ```elixir
  def config(previous, state) do
    output = %{previous.output | title: state.title}
    %{previous | output: output}
  end
  ```

  Updating an existing record preserves its other fields. Supplying a new record
  uses defaults for omitted fields; it does not merge with the previous record.
  Lists replace rather than append. Not all fields can change while running:
  each module explains which changes need a new viewport and its backend limits.

  See [viewport configuration](viewport-configuration.html) for complete examples,
  [raw input](handle_input.html) for callbacks, and
  [Migrate from 0.4.2 to 0.5.0](0-5.html) for the breaking changes.
  """

  alias Emerge.Config.{Assets, Diagnostics, Input, Renderer, Runtime, Window}

  defstruct output: %Window{},
            renderer: %Renderer{},
            input: %Input{},
            assets: %Assets{},
            diagnostics: %Diagnostics{},
            runtime: %Runtime{}

  @type output :: Window.t() | Emerge.Config.DRM.t() | Emerge.Config.Headless.t()
  @type t :: %__MODULE__{
          output: output(),
          renderer: Renderer.t() | Renderer.options(),
          input: Input.t() | Input.options(),
          assets: Assets.t() | Assets.options(),
          diagnostics: Diagnostics.t() | Diagnostics.options(),
          runtime: Runtime.t() | Runtime.options()
        }
  @typedoc "Validated configuration with all nested keyword records expanded into structs."
  @type normalized_t :: %__MODULE__{
          output:
            Window.normalized_t()
            | Emerge.Config.DRM.normalized_t()
            | Emerge.Config.Headless.normalized_t(),
          renderer: Renderer.normalized_t(),
          input: Input.normalized_t(),
          assets: Assets.normalized_t(),
          diagnostics: Diagnostics.normalized_t(),
          runtime: Runtime.normalized_t()
        }

  @doc false
  def __schema__ do
    [
      output: {:variants, [Window, Emerge.Config.DRM, Emerge.Config.Headless]},
      renderer: {:record, Renderer},
      input: {:record, Input},
      assets: {:record, Assets},
      diagnostics: {:record, Diagnostics},
      runtime: {:record, Runtime}
    ]
  end
end
