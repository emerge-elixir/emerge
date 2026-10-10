defmodule Emerge.Config.Font do
  @moduledoc """
  A font loaded from the application's assets. Family and source must be supplied.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `family` | `nil` |
  | `source` | `nil` |
  | `weight` | `400` |
  | `italic` | `false` |
  """
  defstruct family: nil,
            source: nil,
            weight: 400,
            italic: false

  @type t :: %__MODULE__{
          family: String.t() | nil,
          source: String.t() | Emerge.Assets.Ref.t() | nil,
          weight: 100..900,
          italic: boolean()
        }
  @typedoc "Typed keyword shorthand accepted in this record's parent field."
  @type options :: [
          {:family, String.t() | nil}
          | {:source, String.t() | Emerge.Assets.Ref.t() | nil}
          | {:weight, 100..900}
          | {:italic, boolean()}
        ]
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: t()

  @doc false
  def __schema__,
    do: [
      family: {:optional, :non_empty_string},
      source: :asset_source,
      weight: {:integer_range, 100, 900},
      italic: :boolean
    ]
end
