defmodule Emerge.Config.Input do
  @moduledoc """
  Raw observer subscriptions delivered to `c:Emerge.handle_input/2`. These do not disable element interactions, scrolling or native text editing. See the input guide and `Emerge.Input` for exact payloads. The default is no raw subscriptions.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `events` | `[]` |
  | `scroll_line_pixels` | `30.0` |
  | `cursor` | `%Emerge.Config.Cursor{}` |
  """
  defstruct events: [],
            scroll_line_pixels: 30.0,
            cursor: %Emerge.Config.Cursor{}

  @typedoc "Raw input category. `:text` includes committed text and IME events; enter/focus refer to the window, not an element."
  @type event_kind ::
          :key
          | :text
          | :pointer_move
          | :pointer_button
          | :pointer_scroll
          | :pointer_enter
          | :resize
          | :focus
  @type t :: %__MODULE__{
          events: [event_kind()],
          scroll_line_pixels: number(),
          cursor: Emerge.Config.Cursor.t() | Emerge.Config.Cursor.options()
        }
  @typedoc "Typed keyword shorthand accepted in this record's parent field."
  @type options :: [
          {:events, [event_kind()]}
          | {:scroll_line_pixels, number()}
          | {:cursor, Emerge.Config.Cursor.t() | Emerge.Config.Cursor.options()}
        ]
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: %__MODULE__{
          events: [event_kind()],
          scroll_line_pixels: number(),
          cursor: Emerge.Config.Cursor.normalized_t()
        }

  @doc false
  def __schema__,
    do: [
      events:
        {:set,
         {:enum,
          [
            :key,
            :text,
            :pointer_move,
            :pointer_button,
            :pointer_scroll,
            :pointer_enter,
            :resize,
            :focus
          ]}},
      scroll_line_pixels: :positive_number,
      cursor: {:record, Emerge.Config.Cursor}
    ]

  @doc "Returns every supported raw input category."
  @spec all_events() :: [event_kind()]
  def all_events,
    do: [
      :key,
      :text,
      :pointer_move,
      :pointer_button,
      :pointer_scroll,
      :pointer_enter,
      :resize,
      :focus
    ]
end
