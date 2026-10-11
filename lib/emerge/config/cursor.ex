defmodule Emerge.Config.Cursor do
  @moduledoc """
  DRM pointer appearance. Set this record as `Emerge.Config.Input.cursor`.
  Desktop window cursors remain controlled by their window backend.

  ```elixir
  %Emerge.Config{
    input: %Emerge.Config.Input{
      cursor: %Emerge.Config.Cursor{hardware: false}
    }
  }
  ```

  ## Fields

  - `hardware` — prefer a DRM hardware cursor; defaults to `true`. It is a
    preference, not a requirement. CPU raster always draws its cursor in software.
  - `default` — optional `Emerge.Config.CursorIcon` for the ordinary arrow.
  - `text` — optional `Emerge.Config.CursorIcon` for the text-selection cursor.
  - `pointer` — optional `Emerge.Config.CursorIcon` for clickable content.

  All icon fields default to `nil`, keeping the built-in icon. Overrides apply to
  DRM; they do not replace desktop system cursors. All fields require a new
  viewport to change.
  """
  defstruct hardware: true,
            default: nil,
            text: nil,
            pointer: nil

  @type t :: %__MODULE__{
          hardware: boolean(),
          default: Emerge.Config.CursorIcon.t() | Emerge.Config.CursorIcon.options() | nil,
          text: Emerge.Config.CursorIcon.t() | Emerge.Config.CursorIcon.options() | nil,
          pointer: Emerge.Config.CursorIcon.t() | Emerge.Config.CursorIcon.options() | nil
        }
  @typedoc "Typed keyword shorthand accepted in this record's parent field."
  @type options :: [
          {:hardware, boolean()}
          | {:default, Emerge.Config.CursorIcon.t() | Emerge.Config.CursorIcon.options() | nil}
          | {:text, Emerge.Config.CursorIcon.t() | Emerge.Config.CursorIcon.options() | nil}
          | {:pointer, Emerge.Config.CursorIcon.t() | Emerge.Config.CursorIcon.options() | nil}
        ]
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: %__MODULE__{
          hardware: boolean(),
          default: Emerge.Config.CursorIcon.normalized_t() | nil,
          text: Emerge.Config.CursorIcon.normalized_t() | nil,
          pointer: Emerge.Config.CursorIcon.normalized_t() | nil
        }

  @doc false
  def __schema__,
    do: [
      hardware: :boolean,
      default: {:optional, {:record, Emerge.Config.CursorIcon}},
      text: {:optional, {:record, Emerge.Config.CursorIcon}},
      pointer: {:optional, {:record, Emerge.Config.CursorIcon}}
    ]
end
