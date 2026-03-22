defmodule Emerge.Engine.Tree do
  @moduledoc """
  Utilities for working with Emerge.Engine.Element trees.
  """

  alias Emerge.Engine.Element
  alias Emerge.Engine.Reconcile
  alias Emerge.Engine.Tree.Attrs
  alias Emerge.Engine.Tree.Nearby

  @type id_state :: %{
          explicit_seen: MapSet.t()
        }

  @doc """
  Assign ids to elements missing an id.
  """
  @spec assign_ids(Element.t(), id_state()) :: {Element.t(), id_state()}
  def assign_ids(element, state \\ default_id_state())

  def assign_ids(%Element{} = element, state) do
    state = reset_explicit_seen(state)
    {_, assigned} = Reconcile.assign_ids(element)
    {assigned, state}
  end

  def assign_ids(elements, state) when is_list(elements) do
    state = reset_explicit_seen(state)

    Enum.map_reduce(elements, state, fn element, acc ->
      {_, assigned} = Reconcile.assign_ids(element)
      {assigned, acc}
    end)
  end

  @doc """
  Assign ids using a previous tree (compatibility wrapper).
  """
  @spec assign_ids_with_prev(Element.t(), Element.t() | nil, id_state()) ::
          {Element.t(), id_state()}
  def assign_ids_with_prev(element, _prev_element, state \\ default_id_state()) do
    assign_ids(element, state)
  end

  def default_id_state do
    %{explicit_seen: MapSet.new()}
  end

  @doc """
  Compute a hash of attributes excluding volatile fields.
  """
  @spec attrs_hash(map()) :: non_neg_integer()
  def attrs_hash(attrs) when is_map(attrs) do
    Attrs.attrs_hash(attrs)
  end

  @doc """
  Return the list of runtime-only attributes.
  """
  def runtime_attrs do
    Attrs.runtime_attrs()
  end

  @doc """
  Return the fixed nearby mount order used across traversal and encoding.
  """
  def nearby_slots do
    Nearby.nearby_slots()
  end

  @doc """
  Split nearby mount attrs from ordinary attrs.
  """
  def split_nearby_attrs(attrs) when is_map(attrs) do
    Nearby.split_nearby_attrs(attrs)
  end

  @doc """
  Drop nearby mounts from an attrs map.
  """
  def strip_nearby_attrs(attrs) when is_map(attrs) do
    Nearby.strip_nearby_attrs(attrs)
  end

  @doc """
  Merge nearby mounts back into an attrs map.
  """
  def merge_nearby_attrs(attrs, nearby) when is_map(attrs) and is_map(nearby) do
    Nearby.merge_nearby_attrs(attrs, nearby)
  end

  @doc """
  Return nearby mounted children in canonical order.
  """
  def nearby_children(%Element{} = element) do
    Nearby.nearby_children(element)
  end

  def nearby_children(attrs) when is_map(attrs) do
    Nearby.nearby_children(attrs)
  end

  @doc """
  Drop runtime-only attributes from an attrs map.
  """
  def strip_runtime_attrs(attrs) when is_map(attrs) do
    Attrs.strip_runtime_attrs(attrs)
  end

  defp reset_explicit_seen(state) do
    case state do
      %{explicit_seen: _} -> %{state | explicit_seen: MapSet.new()}
      _ -> default_id_state()
    end
  end
end
