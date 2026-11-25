defmodule Emerge.Engine.DiffState do
  @moduledoc """
  Stateful diff helper that keeps numeric id assignments stable.
  """

  alias Emerge.Engine.{Element, EventRegistry, Patch, Reconcile, VNode}
  alias Emerge.Engine.Reconcile.Index

  @type t :: %__MODULE__{
          accepted: {non_neg_integer(), non_neg_integer()} | nil,
          route_epoch: non_neg_integer(),
          tree: Emerge.Engine.Element.t() | nil,
          vdom: VNode.t() | nil,
          index: %Index{},
          event_registry: %{binary() => %{term() => {pid(), term()}}},
          next_id: non_neg_integer()
        }

  defstruct accepted: nil,
            route_epoch: 0,
            tree: nil,
            vdom: nil,
            index: %Index{},
            event_registry: %{},
            next_id: 1

  @doc """
  Initialize diff state with an optional tree.
  """
  def new(tree \\ nil)

  def new(nil), do: %__MODULE__{}

  def new(tree) do
    {_bytes, state, _assigned} = diff_and_encode(new(), tree)
    state
  end

  @doc """
  Compute patches for a new tree and return {patch_binary, updated_state, assigned_tree}.
  """
  @spec diff_and_encode(t(), Element.t() | nil) :: {binary(), t(), Element.t() | nil}
  def diff_and_encode(%__MODULE__{} = state, tree) do
    {bytes, next, _} = transition(state, tree, state.event_registry, :assigned)
    {bytes, next, next.tree}
  end

  @doc """
  Compute patches for a new tree without constructing a full assigned tree.

  This is intended for runtime renderer updates that only need the patch binary
  and next diff state. Intact branches are occurrence references.
  """
  @spec diff_and_encode_binary(t(), Element.t() | nil) :: {binary(), t()}
  def diff_and_encode_binary(%__MODULE__{} = state, tree) do
    {bytes, next, _} = transition(state, tree, state.event_registry, :binary)
    {bytes, next}
  end

  @doc false
  @spec diff_and_encode_routed(t(), Element.t() | nil) ::
          {binary(), t(), EventRegistry.edits()}
  def diff_and_encode_routed(%__MODULE__{} = state, tree),
    do: transition(state, tree, %EventRegistry.Changes{registry: state.event_registry}, :binary)

  defp transition(state, nil, registry, _mode) do
    ops = if state.vdom, do: [Patch.patch(state.vdom.id, nil)], else: []
    {routes, edits} = routes(EventRegistry.reset(registry))

    {Patch.encode(ops),
     %{state | tree: nil, vdom: nil, index: Index.vacate(state.index), event_registry: routes},
     edits}
  end

  defp transition(state, tree, registry, mode) do
    {root, ops, assigned, registry, next} =
      Reconcile.update(state.vdom, tree, state.next_id, registry, mode, state.index)

    {routes, edits} = routes(registry)

    {Patch.encode(ops, state.index.media),
     %{
       state
       | tree: assigned,
         vdom: root,
         index: root.index,
         event_registry: routes,
         next_id: next
     }, edits}
  end

  defp routes(%EventRegistry.Changes{registry: routes, edits: edits}), do: {routes, edits}
  defp routes(registry), do: {registry, %{}}

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

  def build_event_registry(tree), do: EventRegistry.build(tree)
end
