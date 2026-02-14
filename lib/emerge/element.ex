defmodule Emerge.Element do
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
          | :image
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
          id: term() | nil,
          attrs: map(),
          children: [t()],
          frame: frame() | nil
        }

  defstruct [
    :type,
    :id,
    attrs: %{},
    children: [],
    frame: nil
  ]
end
