defmodule Emerge.Config.Input do
  @moduledoc """
  Raw input observation, scroll distance and cursor settings. Set this record as
  `Emerge.Config.input`. Ordinary element interactions do not require subscriptions.

  ```elixir
  %Emerge.Config{
    input: %Emerge.Config.Input{events: [:key, :pointer_button, :resize]}
  }
  ```

  ## Fields

  - `events` — raw categories forwarded to `c:Emerge.handle_input/2`; defaults to
    `[]` (none). Choose `:key`, `:text`, `:pointer_move`, `:pointer_button`,
    `:pointer_scroll`, `:pointer_enter`, `:resize` and/or `:focus`.
    `all_events/0` returns all eight. Duplicates and order do not matter.
    Integers, `nil`, `:all` and `:none` are not accepted.
  - `scroll_line_pixels` — positive pixel distance per wheel/line-scroll step;
    defaults to `30.0`. Precise pixel-scroll input is not converted into line steps.
  - `cursor` — `Emerge.Config.Cursor`, defaulting to its built-in cursor settings.

  Subscriptions filter only raw callbacks, not buttons, element key bindings,
  text editing, scrolling or `c:Emerge.handle_close/2`. The `:text` category includes
  committed text and IME updates. Enter/focus describe a window, not an element.
  See the [input guide](handle_input.html) and `Emerge.Input` for callback values
  and which events each backend can provide.

  ## Changing settings

  `events` can change through `config` and replaces the entire subscription list.
  Messages already in your mailbox are not retracted. Scroll distance and cursor
  settings require a new viewport to change.
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
