defmodule Emerge.Config.CursorIcon do
  @moduledoc """
  Cursor asset and hotspot in image pixels.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `source` | `nil` |
  | `hotspot` | `{0, 0}` |
  """
  defstruct source: nil,
            hotspot: {0, 0}

  @type t :: %__MODULE__{
          source: String.t() | Emerge.Assets.Ref.t() | nil,
          hotspot: {number(), number()}
        }
  @typedoc "Typed keyword shorthand accepted in this record's parent field."
  @type options :: [
          {:source, String.t() | Emerge.Assets.Ref.t() | nil}
          | {:hotspot, {number(), number()}}
        ]
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: t()

  @doc false
  def __schema__, do: [source: :asset_source, hotspot: :position]
end
