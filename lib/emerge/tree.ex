defmodule Emerge.Tree do
  @moduledoc """
  Utilities for working with Emerge.Element trees.
  """

  alias Emerge.Element

  @type id_state :: %{
          explicit_seen: MapSet.t()
        }

  @runtime_attrs [
    :scroll_x,
    :scroll_y,
    :scroll_max,
    :scroll_max_x,
    :scroll_bounds,
    :scroll_clip_bounds,
    :clip_bounds,
    :clip_content,
    :text_baseline_offset,
    :__layer,
    :scroll_capture,
    :mouse_over_active,
    :text_input_focused,
    :text_input_cursor,
    :text_input_selection_anchor,
    :nearby_behind,
    :nearby_in_front,
    :nearby_outside,
    :__attrs_hash
  ]

  @volatile_attrs [
    :scroll_x,
    :scroll_y,
    :scroll_max,
    :scroll_max_x,
    :scroll_bounds,
    :scroll_clip_bounds,
    :clip_bounds,
    :clip_content,
    :text_baseline_offset,
    :__layer,
    :scroll_capture,
    :mouse_over_active,
    :text_input_focused,
    :text_input_cursor,
    :text_input_selection_anchor,
    :__attrs_hash
  ]

  @doc """
  Assign ids to elements missing an id.
  """
  @spec assign_ids(Element.t(), id_state()) :: {Element.t(), id_state()}
  def assign_ids(element, state \\ default_id_state())

  def assign_ids(%Element{} = element, state) do
    state = reset_explicit_seen(state)
    {_, assigned} = Emerge.Reconcile.assign_ids(element)
    {assigned, state}
  end

  def assign_ids(elements, state) when is_list(elements) do
    state = reset_explicit_seen(state)

    Enum.map_reduce(elements, state, fn element, acc ->
      {_, assigned} = Emerge.Reconcile.assign_ids(element)
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

  defp normalize_attrs(attrs) do
    Map.drop(attrs, @volatile_attrs)
  end

  @doc """
  Compute a hash of attributes excluding volatile fields.
  """
  @spec attrs_hash(map()) :: non_neg_integer()
  def attrs_hash(attrs) when is_map(attrs) do
    :erlang.phash2(normalize_attrs(attrs))
  end

  @doc """
  Return the list of runtime-only attributes.
  """
  def runtime_attrs do
    @runtime_attrs
  end

  @doc """
  Drop runtime-only attributes from an attrs map.
  """
  def strip_runtime_attrs(attrs) when is_map(attrs) do
    Map.drop(attrs, @runtime_attrs)
  end

  defp reset_explicit_seen(state) do
    case state do
      %{explicit_seen: _} -> %{state | explicit_seen: MapSet.new()}
      _ -> default_id_state()
    end
  end
end
