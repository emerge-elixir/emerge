defmodule Emerge.Config.Renderer do
  @moduledoc """
  How frames are rendered. `target_fps` caps rendering, not physical display refresh. `device` selects the Vulkan device independently of the DRM scanout card.

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
  @type t :: %__MODULE__{
          api: api(),
          device: String.t() | nil,
          raster_present: raster_present(),
          target_fps: pos_integer() | nil,
          cache: Emerge.Config.RendererCache.t() | Emerge.Config.RendererCache.options()
        }
  @typedoc "Typed keyword shorthand accepted in this record's parent field."
  @type options :: [
          {:api, api()}
          | {:device, String.t() | nil}
          | {:raster_present, raster_present()}
          | {:target_fps, pos_integer() | nil}
          | {:cache, Emerge.Config.RendererCache.t() | Emerge.Config.RendererCache.options()}
        ]
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: %__MODULE__{
          api: api(),
          device: String.t() | nil,
          raster_present: raster_present(),
          target_fps: pos_integer() | nil,
          cache: Emerge.Config.RendererCache.normalized_t()
        }

  @doc false
  def __schema__,
    do: [
      api: {:enum, [:auto, :metal, :opengl, :raster, :vulkan]},
      device: {:optional, :path},
      raster_present: {:enum, [:auto, :cpu, :gpu_upload]},
      target_fps: {:optional, :positive_integer},
      cache: {:record, Emerge.Config.RendererCache}
    ]
end
