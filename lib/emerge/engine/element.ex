defmodule Emerge.Engine.Element do
  @moduledoc """
  Core data structure representing a layout element in the Emerge tree.
  """

  @type element_type ::
          :row
          | :wrapped_row
          | :column
          | :text_column
          | :el
          | :text
          | :text_input
          | :multiline
          | :image
          | :video
          | :none
          | :paragraph

  @type frame :: %{
          x: number(),
          y: number(),
          width: number(),
          height: number()
        }

  @type t :: %__MODULE__{
          type: element_type(),
          key: term() | nil,
          id: non_neg_integer() | nil,
          attrs: map(),
          children: [t()],
          nearby: [{atom(), t()}],
          frame: frame() | nil
        }

  defstruct [
    :type,
    :key,
    :id,
    attrs: %{},
    children: [],
    nearby: [],
    frame: nil
  ]
end
