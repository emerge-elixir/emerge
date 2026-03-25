defmodule Emerge.UI.Input do
  @moduledoc "Input elements"

  alias Emerge.UI.Internal.Builder
  alias Emerge.UI.Internal.Validation

  @type t :: Emerge.UI.element()

  @doc "Single-line text input"
  @spec text(Emerge.UI.attrs(), String.t()) :: t()
  def text(attrs, value) do
    attrs = Builder.prepare_attrs!("Input.text/2", attrs)
    value = Validation.validate_binary_string!("Input.text/2", value)

    attrs
    |> Map.put(:content, value)
    |> Builder.build_element(:text_input, [])
  end

  @doc "Button input with a single child element"
  @spec button(Emerge.UI.attrs(), Emerge.UI.child()) :: t()
  def button(attrs, child) do
    {attrs, child} = Builder.prepare_single_child!("Input.button/2", attrs, child)
    Builder.build_element(attrs, :el, [child])
  end
end
