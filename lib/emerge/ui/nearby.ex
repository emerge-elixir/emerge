defmodule Emerge.UI.Nearby do
  @moduledoc "Overlay and nearby positioning helpers that do not affect layout flow."

  @type overlay_attr ::
          {:above, Emerge.UI.element()}
          | {:below, Emerge.UI.element()}
          | {:on_left, Emerge.UI.element()}
          | {:on_right, Emerge.UI.element()}
          | {:in_front, Emerge.UI.element()}
          | {:behind, Emerge.UI.element()}

  @type t :: overlay_attr()

  @doc "Place an element above the current one without affecting layout flow"
  @spec above(Emerge.UI.element()) :: {:above, Emerge.UI.element()}
  def above(element), do: {:above, element}

  @doc "Place an element below the current one without affecting layout flow"
  @spec below(Emerge.UI.element()) :: {:below, Emerge.UI.element()}
  def below(element), do: {:below, element}

  @doc "Place an element on the left of the current one without affecting layout flow"
  @spec on_left(Emerge.UI.element()) :: {:on_left, Emerge.UI.element()}
  def on_left(element), do: {:on_left, element}

  @doc "Place an element on the right of the current one without affecting layout flow"
  @spec on_right(Emerge.UI.element()) :: {:on_right, Emerge.UI.element()}
  def on_right(element), do: {:on_right, element}

  @doc "Render an element in front of the current one"
  @spec in_front(Emerge.UI.element()) :: {:in_front, Emerge.UI.element()}
  def in_front(element), do: {:in_front, element}

  @doc "Render an element behind the current one"
  @spec behind_content(Emerge.UI.element()) :: {:behind, Emerge.UI.element()}
  def behind_content(element), do: {:behind, element}
end
