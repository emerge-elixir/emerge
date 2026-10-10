defmodule Emerge.Config do
  @moduledoc """
  Declarative viewport configuration.

  All groups have defaults. Output variants use explicit structs; fixed-shape
  groups also accept typed keyword shorthand:

      %Emerge.Config{
        output: %Emerge.Config.Window{title: "My App"},
        renderer: [api: :raster],
        input: [events: [:key, :resize]]
      }

  `c:Emerge.mount/1` supplies the initial config. The optional `c:Emerge.config/2`
  receives the previous **normalized desired config**, not physical hardware
  state. Nested records in that argument are always structs. Prefer typed
  updates with intermediate variables:

      def config(%Emerge.Config{output: %Emerge.Config.Window{} = output} = previous, state) do
        output = %Emerge.Config.Window{output | title: state.title}
        %Emerge.Config{previous | output: output}
      end

  A newly supplied nested record replaces that record. Keyword shorthand fills
  omitted fields from defaults, not from the previous config. Collections are
  replaced, never implicitly appended. Configuration does not interpret the
  cause of a display-state change.

  ## Implementation status

  This is the in-progress 0.5 contract. Native live updates currently support
  input subscriptions and Wayland/macOS titles. Other fields are startup-only unless
  explicitly supported by an adapter. DRM display/backlight controls and live
  frame-rate/output changes remain unfinished: explicit unsupported controls
  report errors, not successful hardware application. Backlight `:auto` does
  not yet manage a device. `target_fps` currently works only at headless startup.
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
