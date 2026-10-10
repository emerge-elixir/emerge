defmodule Emerge.Config.Renderer do
  @moduledoc """
  How frames are rendered. `device` selects the Vulkan device independently
  of the DRM scanout card.

  `target_fps` caps render admissions, not physical display refresh or application
  timers. Headless and Wayland outputs support startup and live caps for all redraw sources;
  pending scenes coalesce, and already admitted frames may finish. `nil` restores
  backend pacing. Config-only changes do not redraw static content, and shutdown
  and PRIME lease retirement remain responsive while capped.

  Wayland caps apply to raster, raster GPU upload, OpenGL and Vulkan. Compositor
  callbacks, input and video retirement remain independent of render admission;
  cap changes do not alter the physical refresh estimate.

  macOS and DRM frame caps are not implemented yet and are rejected explicitly.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `api` | `:auto` |
  | `device` | `nil` |
  | `raster_present` | `:auto` |
  | `target_fps` | `nil` |
  | `cache` | `%Emerge.Config.RendererCache{}` |
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
