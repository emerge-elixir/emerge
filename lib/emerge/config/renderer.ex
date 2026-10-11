defmodule Emerge.Config.Renderer do
  @moduledoc """
  How Emerge draws frames. Set this record as `Emerge.Config.renderer`;
  choose where the frames go separately with `config.output`.

  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.Headless{target: self()},
    renderer: %Emerge.Config.Renderer{api: :raster, target_fps: 30},
    assets: %Emerge.Config.Assets{otp_app: :my_app}
  }
  ```

  ## Fields

  - `api` — `:auto` (default), `:raster`, `:opengl`, `:vulkan` or `:metal`.
    Automatic selection uses an available API for the output/build. Raster draws
    on the CPU; the others use a compatible GPU. Metal is macOS-only; OpenGL and
    Vulkan are available on supported Linux builds. A config cannot enable an API
    absent from the installed renderer.
  - `device` — optional absolute DRM device path for Vulkan GPU selection.
    Defaults to `nil`. Explicit DRM Vulkan requires a device, independently of
    the display card in `Emerge.Config.DRM`.
  - `raster_present` — how CPU-drawn frames reach the output: `:auto` (default),
    `:cpu` or `:gpu_upload`. Automatic uses CPU presentation on DRM and the
    backend's normal route elsewhere. GPU upload requires OpenGL support.
    This setting applies to `api: :raster` or `api: :auto`, not an explicit GPU API.
  - `target_fps` — positive integer frame-rate ceiling, or `nil` (default) for
    normal backend pacing. It limits drawing, not physical display refresh or
    application timers. It does not make an idle scene redraw continuously.
  - `cache` — `Emerge.Config.RendererCache`, controlling reuse of rendered content.
    Defaults to that record's settings; it is separate from image asset caching.

  ## Changing settings

  Only `target_fps` can change through `config`, on headless and Wayland outputs.
  Those outputs also accept it at startup. DRM and macOS do not support frame caps.
  Changing the API, device, presentation route or cache requires a new viewport.
  """
  defstruct api: :auto,
            device: nil,
            raster_present: :auto,
            target_fps: nil,
            cache: %Emerge.Config.RendererCache{}

  @typedoc "Rendering API, subject to backend/build support."
  @type api :: :auto | :metal | :opengl | :raster | :vulkan
  @typedoc "CPU scanout or GPU-upload presentation of raster frames."
  @type raster_present :: :auto | :cpu | :gpu_upload
  @typedoc "Positive 32-bit frame-rate limit; nil restores backend pacing."
  @type frame_rate :: 1..4_294_967_295
  @type t :: %__MODULE__{
          api: api(),
          device: String.t() | nil,
          raster_present: raster_present(),
          target_fps: frame_rate() | nil,
          cache: Emerge.Config.RendererCache.t() | Emerge.Config.RendererCache.options()
        }
  @typedoc "Typed keyword shorthand accepted in this record's parent field."
  @type options :: [
          {:api, api()}
          | {:device, String.t() | nil}
          | {:raster_present, raster_present()}
          | {:target_fps, frame_rate() | nil}
          | {:cache, Emerge.Config.RendererCache.t() | Emerge.Config.RendererCache.options()}
        ]
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: %__MODULE__{
          api: api(),
          device: String.t() | nil,
          raster_present: raster_present(),
          target_fps: frame_rate() | nil,
          cache: Emerge.Config.RendererCache.normalized_t()
        }

  @doc false
  def __schema__,
    do: [
      api: {:enum, [:auto, :metal, :opengl, :raster, :vulkan]},
      device: {:optional, :path},
      raster_present: {:enum, [:auto, :cpu, :gpu_upload]},
      target_fps: {:optional, {:integer_range, 1, 4_294_967_295}},
      cache: {:record, Emerge.Config.RendererCache}
    ]
end
