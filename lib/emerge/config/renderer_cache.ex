defmodule Emerge.Config.RendererCache do
  @moduledoc """
  Reuse rendered content instead of drawing it again. Set this record as
  `Emerge.Config.Renderer.cache`. Leave the defaults unless profiling suggests
  a memory or frame-time problem.

  ```elixir
  %Emerge.Config{
    renderer: %Emerge.Config.Renderer{
      cache: %Emerge.Config.RendererCache{enabled: false}
    }
  }
  ```

  ## Fields

  - `enabled` — `:auto` (default), `true` or `false`. Automatic enables caching
    for GPU rendering and disables it for an explicitly selected raster API.
    `false` disables renderer caching, not the image cache under `assets.cache`.
  - `max_new_payloads_per_frame` — maximum new paint-cache entries created in one
    drawn frame; defaults to `64`. Lower values spread cache creation over more
    frames. `0` prevents new entries while allowing existing entries to be reused.
  - `paint_layer` — `Emerge.Config.PaintLayerCache`, supplying memory, entry and
    age limits for reusable painted content.

  These settings apply to each renderer separately and require a new viewport
  to change. They do not set a total application memory limit.
  """
  defstruct enabled: :auto,
            max_new_payloads_per_frame: 64,
            paint_layer: %Emerge.Config.PaintLayerCache{}

  @typedoc "Explicit cache enable/disable, or backend-dependent automatic selection."
  @type enabled :: boolean() | :auto
  @type t :: %__MODULE__{
          enabled: enabled(),
          max_new_payloads_per_frame: non_neg_integer(),
          paint_layer: Emerge.Config.PaintLayerCache.t() | Emerge.Config.PaintLayerCache.options()
        }
  @typedoc "Typed keyword shorthand accepted in this record's parent field."
  @type options :: [
          {:enabled, enabled()}
          | {:max_new_payloads_per_frame, non_neg_integer()}
          | {:paint_layer,
             Emerge.Config.PaintLayerCache.t() | Emerge.Config.PaintLayerCache.options()}
        ]
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: %__MODULE__{
          enabled: enabled(),
          max_new_payloads_per_frame: non_neg_integer(),
          paint_layer: Emerge.Config.PaintLayerCache.normalized_t()
        }

  @doc false
  def __schema__,
    do: [
      enabled: {:one_of, [:boolean, :auto]},
      max_new_payloads_per_frame: :non_negative_integer,
      paint_layer: {:record, Emerge.Config.PaintLayerCache}
    ]
end
