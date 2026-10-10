defmodule Emerge.Config.RendererCache do
  @moduledoc """
  Rendering cache policy. `enabled: :auto` enables GPU caching and disables it for explicit raster.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `enabled` | `:auto` |
  | `max_new_payloads_per_frame` | `64` |
  | `paint_layer` | `%Emerge.Config.PaintLayerCache{}` |
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
