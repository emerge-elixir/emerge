defmodule Emerge.Config.CursorIcon do
  @moduledoc """
  An image and pointer hotspot for a DRM cursor override. Use this record in the
  `default`, `text` or `pointer` field of `Emerge.Config.Cursor`.

  ```elixir
  %Emerge.Config{
    input: %Emerge.Config.Input{
      cursor: %Emerge.Config.Cursor{
        pointer: %Emerge.Config.CursorIcon{source: "cursors/hand.png", hotspot: {4, 2}}
      }
    }
  }
  ```

  ## Fields

  - `source` — an image path or `Emerge.Assets.Ref`. Relative paths resolve under
    the application's `priv/` directory. Absolute paths are runtime assets and
    need the policy in `Emerge.Config.RuntimePaths`. The default `nil` is a
    placeholder; a supplied icon must have a source.
  - `hotspot` — `{x, y}` position of the pointing tip within the image, in image
    pixels. Defaults to `{0, 0}`; coordinates must be non-negative numbers.

  A hotspot changes where the cursor points, not where UI elements are laid out.
  Cursor overrides require a new viewport to change.
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
