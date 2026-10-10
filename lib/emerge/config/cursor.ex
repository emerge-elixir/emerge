defmodule Emerge.Config.Cursor do
  @moduledoc """
  DRM cursor hardware preference and optional per-icon asset overrides.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `hardware` | `true` |
  | `default` | `nil` |
  | `text` | `nil` |
  | `pointer` | `nil` |
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
