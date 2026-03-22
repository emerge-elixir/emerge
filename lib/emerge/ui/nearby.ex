defmodule Emerge.UI.Nearby do
  @moduledoc "Overlay and nearby positioning helpers that do not affect layout flow."

  @doc "Place an element above the current one without affecting layout flow"
  def above(element), do: {:above, element}

  @doc "Place an element below the current one without affecting layout flow"
  def below(element), do: {:below, element}

  @doc "Place an element on the left of the current one without affecting layout flow"
  def on_left(element), do: {:on_left, element}

  @doc "Place an element on the right of the current one without affecting layout flow"
  def on_right(element), do: {:on_right, element}

  @doc "Render an element in front of the current one"
  def in_front(element), do: {:in_front, element}

  @doc "Render an element behind the current one"
  def behind_content(element), do: {:behind, element}
end
