defmodule Emerge.Engine.DiffState do
  @moduledoc """
  Stateful diff helper that keeps numeric id assignments stable.
  """

  alias Emerge.Engine.Reconcile
  alias Emerge.Engine.Tree.Nearby
  alias Emerge.Engine.VNode

  @type t :: %__MODULE__{
          tree: Emerge.Engine.Element.t() | nil,
          vdom: VNode.t() | nil,
          event_registry: %{binary() => %{term() => {pid(), term()}}}
        }

  defstruct tree: nil, vdom: nil, event_registry: %{}

  @doc """
  Initialize diff state with an optional tree.
  """
  def new(tree \\ nil)

  def new(nil), do: %__MODULE__{}

  def new(tree) do
    {vdom, tree} = Reconcile.assign_ids(tree)
    %__MODULE__{tree: tree, vdom: vdom, event_registry: build_event_registry(tree)}
  end

  @doc """
  Compute patches for a new tree and return {patch_binary, updated_state, assigned_tree}.
  """
  @spec diff_and_encode(t(), Emerge.Engine.Element.t()) ::
          {binary(), t(), Emerge.Engine.Element.t()}
  def diff_and_encode(%__MODULE__{} = state, tree) do
    {vdom, patches, assigned} = Reconcile.reconcile(state.vdom, tree)

    {
      Emerge.Engine.Patch.encode(patches),
      %__MODULE__{tree: assigned, vdom: vdom, event_registry: build_event_registry(assigned)},
      assigned
    }
  end

  @spec dispatch_click(t(), binary()) :: :ok
  def dispatch_click(%__MODULE__{} = state, id_bin) when is_binary(id_bin) do
    dispatch_event(state, id_bin, :click)
  end

  @spec dispatch_event(t(), binary(), term()) :: :ok
  def dispatch_event(%__MODULE__{event_registry: registry}, id_bin, event)
      when is_binary(id_bin) do
    dispatch_event_with_payload(%__MODULE__{event_registry: registry}, id_bin, event, :no_payload)
  end

  @spec dispatch_event(t(), binary(), term(), term()) :: :ok
  def dispatch_event(%__MODULE__{event_registry: registry}, id_bin, event, payload)
      when is_binary(id_bin) do
    dispatch_event_with_payload(
      %__MODULE__{event_registry: registry},
      id_bin,
      event,
      {:with_payload, payload}
    )
  end

  defp dispatch_event_with_payload(%__MODULE__{event_registry: registry}, id_bin, event, payload) do
    case lookup_event(%__MODULE__{event_registry: registry}, id_bin, event) do
      {:ok, {pid, msg}} when is_pid(pid) ->
        send(pid, dispatch_message(msg, payload))
        :ok

      _ ->
        :ok
    end
  end

  defp dispatch_message(msg, :no_payload), do: msg

  defp dispatch_message(msg, {:with_payload, payload}) when is_tuple(msg),
    do: Tuple.insert_at(msg, tuple_size(msg), payload)

  defp dispatch_message(msg, {:with_payload, payload}), do: {msg, payload}

  @spec lookup_event(t(), binary(), term()) :: {:ok, {pid(), term()}} | :error
  def lookup_event(%__MODULE__{event_registry: registry}, id_bin, event)
      when is_binary(id_bin) do
    case Map.get(registry, id_bin, %{}) |> Map.get(event) do
      {pid, msg} when is_pid(pid) -> {:ok, {pid, msg}}
      _ -> :error
    end
  end

  def build_event_registry(tree) do
    tree
    |> collect_event_handlers(%{})
  end

  defp collect_event_handlers(%Emerge.Engine.Element{} = element, acc) do
    acc
    |> register_event(element, :on_click, :click)
    |> register_event(element, :on_press, :press)
    |> register_event(element, :on_mouse_down, :mouse_down)
    |> register_event(element, :on_mouse_up, :mouse_up)
    |> register_event(element, :on_mouse_enter, :mouse_enter)
    |> register_event(element, :on_mouse_leave, :mouse_leave)
    |> register_event(element, :on_mouse_move, :mouse_move)
    |> register_event(element, :on_change, :change)
    |> register_event(element, :on_focus, :focus)
    |> register_event(element, :on_blur, :blur)
    |> register_key_events(element, :on_key_down, :key_down)
    |> register_key_events(element, :on_key_up, :key_up)
    |> register_key_events(element, :on_key_press, :key_press)
    |> then(fn registry ->
      registry =
        Enum.reduce(element.children, registry, fn child, next_registry ->
          collect_event_handlers(child, next_registry)
        end)

      Enum.reduce(Nearby.nearby_children(element), registry, fn {_slot, child}, next_registry ->
        collect_event_handlers(child, next_registry)
      end)
    end)
  end

  defp register_event(acc, element, attr, event) do
    case Map.get(element.attrs, attr) do
      {pid, msg} when is_pid(pid) ->
        id_bin = :erlang.term_to_binary(element.id)

        Map.update(acc, id_bin, %{event => {pid, msg}}, fn events ->
          Map.put(events, event, {pid, msg})
        end)

      _ ->
        acc
    end
  end

  defp register_key_events(acc, element, attr, event_type) do
    case Map.get(element.attrs, attr) do
      bindings when is_list(bindings) ->
        Enum.reduce(bindings, acc, fn binding, next_acc ->
          register_key_event(next_acc, element, event_type, binding)
        end)

      _ ->
        acc
    end
  end

  defp register_key_event(acc, element, event_type, %{route: route, payload: {pid, msg}})
       when is_binary(route) and is_pid(pid) do
    id_bin = :erlang.term_to_binary(element.id)
    event = {event_type, route}

    Map.update(acc, id_bin, %{event => {pid, msg}}, fn events ->
      Map.put(events, event, {pid, msg})
    end)
  end

  defp register_key_event(acc, _element, _event_type, _binding), do: acc
end
