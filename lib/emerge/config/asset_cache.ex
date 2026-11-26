defmodule Emerge.Config.AssetCache do
  @moduledoc """
  Limits for retained image pixels and parsed SVGs. Set this record as
  `Emerge.Config.Assets.cache`. Budgets apply separately to each renderer.

  ```elixir
  %Emerge.Config{
    assets: %Emerge.Config.Assets{
      cache: %Emerge.Config.AssetCache{max_entries: 32, max_bytes: 32 * 1024 * 1024}
    }
  }
  ```

  ## Fields

  - `max_entries` — maximum retained decoded-image entries, including rasterized
    SVG sizes; defaults to `256`.
  - `max_bytes` — retained decoded-pixel budget in bytes;
    defaults to `268_435_456` (256 MiB). This is not compressed file size.
  - `svg_tree_max_entries` — maximum retained parsed SVGs; defaults to `64`.
    One parsed SVG can be reused to draw multiple sizes.
  - `svg_tree_max_bytes` — estimated parsed-SVG storage budget in bytes;
    defaults to `16_777_216` (16 MiB), separate from rendered pixels.

  All values are non-negative integers. A zero entry/byte limit disables retention
  in the corresponding cache; images still load and draw. Limits do not bound
  peak decoding memory, fonts or total application memory. These fields require
  a new viewport to change. See [Use assets](use_assets.html) for cache tuning.
  """
  defstruct max_entries: 256,
            max_bytes: 268_435_456,
            svg_tree_max_entries: 64,
            svg_tree_max_bytes: 16_777_216

  @type t :: %__MODULE__{
          max_entries: non_neg_integer(),
          max_bytes: non_neg_integer(),
          svg_tree_max_entries: non_neg_integer(),
          svg_tree_max_bytes: non_neg_integer()
        }
  @typedoc "Typed keyword shorthand accepted in this record's parent field."
  @type options :: [
          {:max_entries, non_neg_integer()}
          | {:max_bytes, non_neg_integer()}
          | {:svg_tree_max_entries, non_neg_integer()}
          | {:svg_tree_max_bytes, non_neg_integer()}
        ]
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: t()

  @doc false
  def __schema__,
    do: [
      max_entries: :non_negative_integer,
      max_bytes: :non_negative_integer,
      svg_tree_max_entries: :non_negative_integer,
      svg_tree_max_bytes: :non_negative_integer
    ]
end
