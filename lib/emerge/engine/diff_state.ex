defmodule Emerge.Engine.DiffState do
  @moduledoc """
  Stateful diff helper that keeps numeric id assignments stable.
  """

  alias Emerge.Engine.EventRegistry
  alias Emerge.Engine.Reconcile
  alias Emerge.Engine.VNode

  @type t :: %__MODULE__{
          tree: Emerge.Engine.Element.t() | nil,
          vdom: VNode.t() | nil,
          event_registry: %{binary() => %{term() => {pid(), term()}}},
          next_id: non_neg_integer()
        }

  defstruct tree: nil, vdom: nil, event_registry: %{}, next_id: 1

  @doc """
  Initialize diff state with an optional tree.
  """
  def new(tree \\ nil)

  def new(nil), do: %__MODULE__{}

  def new(tree) do
    {vdom, tree, next_id} = Reconcile.assign_ids(tree, 1)

    %__MODULE__{
      tree: tree,
      vdom: vdom,
      event_registry: build_event_registry(tree),
      next_id: next_id
    }
  end

  @doc """
  Compute patches for a new tree and return {patch_binary, updated_state, assigned_tree}.
  """
  @spec diff_and_encode(t(), Emerge.Engine.Element.t()) ::
          {binary(), t(), Emerge.Engine.Element.t()}
  def diff_and_encode(%__MODULE__{} = state, tree) do
    {vdom, patches, assigned, next_id} =
      Reconcile.reconcile(state.vdom, tree, state.next_id)

    {
      Emerge.Engine.Patch.encode(patches),
      %__MODULE__{
        tree: assigned,
        vdom: vdom,
        event_registry: build_event_registry(assigned),
        next_id: next_id
      },
      assigned
    }
  end

  @doc """
  Compute patches for a new tree without constructing a full assigned tree.

  This is intended for runtime renderer updates that only need the patch binary
  and next diff state. Insert patches still encode assigned inserted subtrees.
  """
  @spec diff_and_encode_binary(t(), Emerge.Engine.Element.t()) :: {binary(), t()}
  def diff_and_encode_binary(%__MODULE__{} = state, tree) do
    {vdom, patches, event_registry, next_id} =
      Reconcile.reconcile_patches_and_event_registry(
        state.vdom,
        tree,
        state.next_id,
        state.event_registry
      )

    {
      Emerge.Engine.Patch.encode(patches),
      %__MODULE__{
        state
        | tree: nil,
          vdom: vdom,
          event_registry: event_registry,
          next_id: next_id
      }
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

  def build_event_registry(tree), do: EventRegistry.build(tree)
end
