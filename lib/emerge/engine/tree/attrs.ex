defmodule Emerge.Engine.Tree.Attrs do
  @moduledoc false

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
    :mouse_down_active,
    :focused_active,
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
    :mouse_down_active,
    :focused_active,
    :text_input_focused,
    :text_input_cursor,
    :text_input_selection_anchor,
    :__attrs_hash
  ]

  @doc false
  def runtime_attrs, do: @runtime_attrs

  @doc false
  def strip_runtime_attrs(attrs) when is_map(attrs) do
    Map.drop(attrs, @runtime_attrs)
  end

  @doc false
  def attrs_hash(attrs) when is_map(attrs) do
    attrs
    |> Map.drop(@volatile_attrs)
    |> :erlang.phash2()
  end
end
