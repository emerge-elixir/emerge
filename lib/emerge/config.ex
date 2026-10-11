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

  ## Outputs and supported live changes

  - `Emerge.Config.Window` selects a desktop window; `backend: :auto` uses the
    platform default, independently of `renderer.api`.
  - `Emerge.Config.DRM` selects a KMS output; `preferred_size` is a mode-selection
    hint, not a guaranteed framebuffer size.
  - `Emerge.Config.Headless` sends frames to a required live local PID; use an
    explicit `Emerge.Config.BinaryFrames` or `Emerge.Config.PrimeFrames` variant.

  Input subscriptions and runtime health intervals can change without restarting.
  Window titles are live on Wayland/macOS; render-admission caps are live on
  headless/Wayland. DRM display/brightness controls support explicit OpenGL and
  CPU raster (`raster_present: :auto | :cpu`). Off retains ownership/tree/intent
  while destroying the presenter; on creates fresh resources and enables a managed
  backlight after first presentation. Backlight `:auto` requires proven connector
  association, `nil` opts out, and an explicit path selects a device.

  DRM connector/mode changes, Vulkan/raster GPU-upload power controls and DRM/macOS
  frame caps remain rejected. Output type, rendering API/device, assets, diagnostics,
  cursor policy and adapter identity/options are startup-only. Unsupported changes
  report errors instead of silently restarting or claiming hardware success.

  This is the breaking 0.5 API: old mount/start keyword configurations and public
  numeric input masks are removed. See the [configuration reference](viewport-configuration.html),
  [input guide](handle_input.html) and [0.5 migration guide](0-5.html). Software
  support does not establish panel timing or device qualification.
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
