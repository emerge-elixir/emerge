defmodule Emerge.Engine.EventRegistry do
  @moduledoc false

  alias Emerge.Engine.Element
  alias Emerge.Engine.NodeId
  alias Emerge.Engine.Tree.Nearby
  alias Emerge.Engine.VNode

  @type events :: %{term() => {pid(), term()}}
  @type t :: %{binary() => events()}

  @spec build(Element.t() | VNode.t() | nil) :: t()
  def build(tree), do: merge_tree(%{}, tree)

  @spec merge_tree(t(), Element.t() | VNode.t() | nil) :: t()
  def merge_tree(registry, nil), do: registry

  def merge_tree(registry, %VNode{} = vnode) do
    registry
    |> put_events(vnode.id, vnode.events)
    |> then(fn registry ->
      registry = Enum.reduce(vnode.children, registry, &merge_tree(&2, &1))

      Enum.reduce(vnode.nearby, registry, fn {_slot, child}, next_registry ->
        merge_tree(next_registry, child)
      end)
    end)
  end

  def merge_tree(registry, %Element{} = element) do
    registry
    |> put_events(element.id, node_events(element.attrs))
    |> then(fn registry ->
      registry = Enum.reduce(element.children, registry, &merge_tree(&2, &1))

      Enum.reduce(Nearby.nearby_children(element), registry, fn {_slot, child}, next_registry ->
        merge_tree(next_registry, child)
      end)
    end)
  end

  @spec delete_vnode_subtree(t(), VNode.t()) :: t()
  def delete_vnode_subtree(registry, %VNode{} = vnode) do
    registry
    |> delete_node(vnode.id)
    |> then(fn registry ->
      registry = Enum.reduce(vnode.children, registry, &delete_vnode_subtree(&2, &1))

      Enum.reduce(vnode.nearby, registry, fn {_slot, child}, next_registry ->
        delete_vnode_subtree(next_registry, child)
      end)
    end)
  end

  @spec node_events(map()) :: events()
  def node_events(attrs) do
    %{}
    |> register_event(attrs, :on_click, :click)
    |> register_event(attrs, :on_press, :press)
    |> register_event(attrs, :on_swipe_up, :swipe_up)
    |> register_event(attrs, :on_swipe_down, :swipe_down)
    |> register_event(attrs, :on_swipe_left, :swipe_left)
    |> register_event(attrs, :on_swipe_right, :swipe_right)
    |> register_event(attrs, :on_mouse_down, :mouse_down)
    |> register_event(attrs, :on_mouse_up, :mouse_up)
    |> register_event(attrs, :on_mouse_enter, :mouse_enter)
    |> register_event(attrs, :on_mouse_leave, :mouse_leave)
    |> register_event(attrs, :on_mouse_move, :mouse_move)
    |> register_event(attrs, :on_change, :change)
    |> register_event(attrs, :on_focus, :focus)
    |> register_event(attrs, :on_blur, :blur)
    |> register_virtual_key_hold_event(attrs)
    |> register_key_events(attrs, :on_key_down, :key_down)
    |> register_key_events(attrs, :on_key_up, :key_up)
    |> register_key_events(attrs, :on_key_press, :key_press)
  end

  @spec put_events(t(), non_neg_integer() | nil, events()) :: t()
  def put_events(registry, nil, _events), do: registry
  def put_events(registry, id, events) when map_size(events) == 0, do: delete_node(registry, id)

  def put_events(registry, id, events) when is_integer(id) do
    Map.put(registry, NodeId.encode(id), events)
  end

  @spec delete_node(t(), non_neg_integer() | nil) :: t()
  def delete_node(registry, nil), do: registry
  def delete_node(registry, id) when is_integer(id), do: Map.delete(registry, NodeId.encode(id))

  # Compatibility wrapper for callers that only know about attrs.
  @spec put_node(t(), non_neg_integer() | nil, map()) :: t()
  def put_node(registry, id, attrs), do: put_events(registry, id, node_events(attrs))

  defp register_event(events, attrs, attr, event) do
    case Map.get(attrs, attr) do
      {pid, msg} when is_pid(pid) -> Map.put(events, event, {pid, msg})
      _ -> events
    end
  end

  defp register_key_events(events, attrs, attr, event_type) do
    case Map.get(attrs, attr) do
      bindings when is_list(bindings) ->
        Enum.reduce(bindings, events, fn binding, next_events ->
          register_key_event(next_events, event_type, binding)
        end)

      _ ->
        events
    end
  end

  defp register_key_event(events, event_type, %{route: route, payload: {pid, msg}})
       when is_binary(route) and is_pid(pid) do
    Map.put(events, {event_type, route}, {pid, msg})
  end

  defp register_key_event(events, _event_type, _binding), do: events

  defp register_virtual_key_hold_event(events, attrs) do
    case Map.get(attrs, :virtual_key) do
      %{hold: {:event, {pid, msg}}} when is_pid(pid) ->
        Map.put(events, :virtual_key_hold, {pid, msg})

      _ ->
        events
    end
  end
end
