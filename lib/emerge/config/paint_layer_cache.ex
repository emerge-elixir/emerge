defmodule Emerge.Config.PaintLayerCache do
  @moduledoc """
  Memory and age limits for reusable painted content. Set this record as
  `Emerge.Config.RendererCache.paint_layer`. These are advanced tuning settings,
  separate from decoded image budgets in `Emerge.Config.AssetCache`.

  ```elixir
  %Emerge.Config{
    renderer: %Emerge.Config.Renderer{
      cache: %Emerge.Config.RendererCache{
        paint_layer: %Emerge.Config.PaintLayerCache{max_bytes: 64 * 1024 * 1024}
      }
    }
  }
  ```

  ## Fields

  - `max_entries` — maximum retained paint entries; defaults to `512`.
    `0` prevents new paint entries from being stored.
  - `max_bytes` — total retained paint storage budget, in bytes;
    defaults to `671_088_640` (640 MiB).
  - `max_entry_bytes` — largest single entry eligible for storage, in bytes;
    defaults to `268_435_456` (256 MiB). Larger content still draws without caching.
  - `min_visible_before_store` — minimum visible-frame count before content is
    eligible for storage; defaults to `1`. The renderer may require a longer
    observation period for some content.
  - `max_stale_frames` — number of drawn frames an unseen entry may remain cached;
    defaults to `120`. This is a frame count, not milliseconds.

  Values are non-negative integers. Budgets describe retained cache storage, not
  total process/GPU memory or temporary drawing allocations. All fields require
  a new viewport to change.
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
