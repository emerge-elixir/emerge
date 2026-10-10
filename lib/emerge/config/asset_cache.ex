defmodule Emerge.Config.AssetCache do
  @moduledoc """
  Decoded image and parsed SVG cache budgets in entries and bytes.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `max_entries` | `256` |
  | `max_bytes` | `268_435_456` |
  | `svg_tree_max_entries` | `64` |
  | `svg_tree_max_bytes` | `16_777_216` |
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
