defmodule Emerge.Engine.Tree.Nearby do
  @moduledoc false

  alias Emerge.Engine.Element

  @slots [:behind, :above, :on_right, :below, :on_left, :in_front]
  @local_slots [:behind]
  @escape_slots [:above, :on_right, :below, :on_left, :in_front]

  @doc false
  def nearby_slots, do: @slots

  @doc false
  def local_slots, do: @local_slots

  @doc false
  def escape_slots, do: @escape_slots

  @doc false
  def nearby_attr?(slot) when slot in @slots, do: true
  def nearby_attr?(_slot), do: false

  @doc false
  def slot_tag(:behind), do: 1
  def slot_tag(:above), do: 2
  def slot_tag(:on_right), do: 3
  def slot_tag(:below), do: 4
  def slot_tag(:on_left), do: 5
  def slot_tag(:in_front), do: 6

  @doc false
  def slot_from_tag!(1), do: :behind
  def slot_from_tag!(2), do: :above
  def slot_from_tag!(3), do: :on_right
  def slot_from_tag!(4), do: :below
  def slot_from_tag!(5), do: :on_left
  def slot_from_tag!(6), do: :in_front

  def slot_from_tag!(tag), do: raise(ArgumentError, "invalid nearby slot tag: #{inspect(tag)}")

  @doc false
  def nearby_children(%Element{} = element), do: element.nearby

  def nearby_children(nearby) when is_list(nearby), do: nearby

  @doc false
  def mount_refs(nearby) when is_list(nearby) do
    Enum.map(nearby, fn {slot, %Element{id: id}} -> {slot, id} end)
  end

  @doc false
  def mount_ids(nearby) when is_list(nearby) do
    Enum.map(nearby, fn {_slot, %Element{id: id}} -> id end)
  end

  @doc false
  def mount_ids_from_refs(refs) when is_list(refs) do
    Enum.map(refs, fn {_slot, id} -> id end)
  end

  @doc false
  def local_nearby(nearby) when is_list(nearby) do
    Enum.filter(nearby, fn {slot, _element} -> slot in @local_slots end)
  end

  @doc false
  def escape_nearby(nearby) when is_list(nearby) do
    Enum.filter(nearby, fn {slot, _element} -> slot in @escape_slots end)
  end

  @doc false
  def strip_nearby_attrs(attrs) when is_map(attrs) do
    Enum.reduce(@slots, attrs, fn slot, acc -> Map.delete(acc, slot) end)
  end
end
