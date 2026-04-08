defmodule EmergeDemo.AppSelector.Screens do
  @moduledoc false

  use Solve.Controller, events: [:toggle_menu, :close_menu, :set_screen]

  @screens [%{id: :todo, label: "Todo"}]

  @impl true
  def init(_init_params, _dependencies) do
    %{current: :todo, menu_open?: false}
  end

  def toggle_menu(_payload, state), do: %{state | menu_open?: !state.menu_open?}

  def close_menu(_payload, state), do: %{state | menu_open?: false}

  def set_screen(:todo, state), do: %{state | current: :todo, menu_open?: false}
  def set_screen(_other, state), do: state

  @impl true
  def expose(state, _dependencies, _init_params) do
    %{current: state.current, menu_open?: state.menu_open?, screens: @screens}
  end
end
