defmodule Emerge.Config.PaintLayerCache do
  @moduledoc """
  Limits for retained paint-layer payloads. Byte limits are bytes, ages are frames.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `max_entries` | `512` |
  | `max_bytes` | `671_088_640` |
  | `max_entry_bytes` | `268_435_456` |
  | `min_visible_before_store` | `1` |
  | `max_stale_frames` | `120` |
  """
  defstruct max_entries: 512,
            max_bytes: 671_088_640,
            max_entry_bytes: 268_435_456,
            min_visible_before_store: 1,
            max_stale_frames: 120

  @type t :: %__MODULE__{
          max_entries: non_neg_integer(),
          max_bytes: non_neg_integer(),
          max_entry_bytes: non_neg_integer(),
          min_visible_before_store: non_neg_integer(),
          max_stale_frames: non_neg_integer()
        }
  @typedoc "Typed keyword shorthand accepted in this record's parent field."
  @type options :: [
          {:max_entries, non_neg_integer()}
          | {:max_bytes, non_neg_integer()}
          | {:max_entry_bytes, non_neg_integer()}
          | {:min_visible_before_store, non_neg_integer()}
          | {:max_stale_frames, non_neg_integer()}
        ]
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: t()

  @doc false
  def __schema__,
    do: [
      max_entries: :non_negative_integer,
      max_bytes: :non_negative_integer,
      max_entry_bytes: :non_negative_integer,
      min_visible_before_store: :non_negative_integer,
      max_stale_frames: :non_negative_integer
    ]
end
