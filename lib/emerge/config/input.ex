defmodule Emerge.Config.Input do
  @moduledoc """
  Raw observer subscriptions delivered to `c:Emerge.handle_input/2`. These do not
  disable element interactions, scrolling or native text editing. See
  `Emerge.Input` and the [input guide](handle_input.html) for exact payloads.

  `events: []` (the default) observes nothing; `all_events/0` returns all eight
  categories. Use a list of category atoms, not a numeric mask or `:all`/`:none`.
  Duplicate categories are deduplicated and order is insignificant. Live updates
  replace the subscriptions, but cannot retract already-forwarded mailbox messages.
  Window close goes to `c:Emerge.handle_close/2` independently of this list.

      %Emerge.Config{input: [events: [:key, :pointer_button, :resize]]}

  Subscription names describe backend window/surface input, not element events.
  Headless has no physical input source; DRM has no desktop window enter/focus
  lifecycle. IME availability is backend-dependent.

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
