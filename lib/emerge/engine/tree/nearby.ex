defmodule Emerge.Engine.Tree.Nearby do
  @moduledoc false

  alias Emerge.Engine.Element

  @slots [:behind, :above, :on_right, :below, :on_left, :in_front]

  @doc false
  def nearby_slots, do: @slots

  @doc false
  def split_nearby_attrs(attrs) when is_map(attrs) do
    nearby =
      Enum.reduce(@slots, %{}, fn slot, acc ->
        case Map.get(attrs, slot) do
          %Element{} = element -> Map.put(acc, slot, element)
          _ -> acc
        end
      end)

    {Map.drop(attrs, @slots), nearby}
  end

  @doc false
  def strip_nearby_attrs(attrs) when is_map(attrs) do
    Map.drop(attrs, @slots)
  end

  @doc false
  def merge_nearby_attrs(attrs, nearby) when is_map(attrs) and is_map(nearby) do
    Enum.reduce(@slots, attrs, fn slot, acc ->
      case Map.get(nearby, slot) do
        %Element{} = element -> Map.put(acc, slot, element)
        _ -> acc
      end
    end)
  end

  @doc false
  def nearby_children(%Element{} = element) do
    nearby_children(element.attrs)
  end

  def nearby_children(attrs) when is_map(attrs) do
    Enum.flat_map(@slots, fn slot ->
      case Map.get(attrs, slot) do
        %Element{} = element -> [{slot, element}]
        _ -> []
      end
    end)
  end
end
