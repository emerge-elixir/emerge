defmodule Emerge.Config.Font do
  @moduledoc """
  One application font registration. Add this record to `Emerge.Config.Assets.fonts`.
  Use the registered family with `Emerge.UI.Font.family/1` in your UI.

  ```elixir
  %Emerge.Config{
    assets: %Emerge.Config.Assets{
      fonts: [%Emerge.Config.Font{family: "Inter", source: "fonts/Inter-Bold.ttf", weight: 700}]
    }
  }
  ```

  ## Fields

  - `family` — non-empty family name used by your UI. Required; defaults to `nil`
    only so an incomplete record can be constructed.
  - `source` — logical font path under the application's `priv/` directory,
    or an `Emerge.Assets.Ref`. Required; defaults to `nil`.
  - `weight` — integer from `100` to `900`; defaults to `400` (regular).
    Use `700` for a bold variant when the supplied font provides it.
  - `italic` — whether this registration is an italic variant; defaults to `false`.

  The family/weight/italic combination must be unique in the font list. Registering
  a weight does not synthesize a different font file. Fonts require a new viewport
  to change. See [Use assets](use_assets.html) for registering multiple variants.
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
