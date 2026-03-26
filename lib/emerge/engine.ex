defmodule Emerge.Engine do
  @moduledoc """
  Retained tree diffing, encoding, and event routing helpers.
  """

  alias Emerge.Engine.DiffState
  alias Emerge.Engine.Reconcile
  alias Emerge.Engine.Serialization

  @typedoc "Incremental diff state used between tree uploads and patches."
  @type diff_state :: DiffState.t()

  @doc """
  Initialize a diff state for incremental updates.
  """
  @spec diff_state_new(Emerge.tree() | nil) :: diff_state()
  def diff_state_new(tree \\ nil) do
    DiffState.new(tree)
  end

  @doc """
  Compute patches for a new tree and return {patch_binary, next_state, assigned_tree}.
  """
  @spec diff_state_update(diff_state(), Emerge.tree()) :: {binary(), diff_state(), Emerge.tree()}
  def diff_state_update(state, tree) do
    DiffState.diff_and_encode(state, tree)
  end

  @doc """
  Encode a full tree with ids and return {binary, next_state, assigned_tree}.
  """
  @spec encode_full(diff_state(), Emerge.tree()) :: {binary(), diff_state(), Emerge.tree()}
  def encode_full(%DiffState{} = state, tree) do
    {vdom, assigned} = Reconcile.assign_ids(tree)

    {
      Serialization.encode_tree(assigned),
      %DiffState{
        state
        | tree: assigned,
          vdom: vdom,
          event_registry: DiffState.build_event_registry(assigned)
      },
      assigned
    }
  end

  @doc """
  Encode a full tree and return {full_binary, patch_binary, next_state, assigned_tree}.

  The patch binary is always empty for initial uploads.
  """
  @spec encode_full_with_empty_patch(diff_state(), Emerge.tree()) ::
          {binary(), binary(), diff_state(), Emerge.tree()}
  def encode_full_with_empty_patch(%DiffState{} = state, tree) do
    {full_bin, next_state, assigned} = encode_full(state, tree)
    {full_bin, <<>>, next_state, assigned}
  end

  @doc """
  Dispatch a click event to the handler registered for an element id.
  """
  @spec dispatch_click(diff_state(), binary()) :: :ok
  def dispatch_click(%DiffState{} = state, id_bin) when is_binary(id_bin) do
    DiffState.dispatch_click(state, id_bin)
  end

  @doc """
  Dispatch an element event to the handler registered for an element id.
  """
  @spec dispatch_event(diff_state(), binary(), term()) :: :ok
  def dispatch_event(%DiffState{} = state, id_bin, event)
      when is_binary(id_bin) do
    DiffState.dispatch_event(state, id_bin, event)
  end

  @doc """
  Dispatch an element event with payload to the registered handler.
  """
  @spec dispatch_event(diff_state(), binary(), term(), term()) :: :ok
  def dispatch_event(%DiffState{} = state, id_bin, event, payload)
      when is_binary(id_bin) do
    DiffState.dispatch_event(state, id_bin, event, payload)
  end

  @doc """
  Lookup the handler payload for an element event.
  """
  @spec lookup_event(diff_state(), binary(), term()) :: {:ok, {pid(), term()}} | :error
  def lookup_event(%DiffState{} = state, id_bin, event)
      when is_binary(id_bin) do
    DiffState.lookup_event(state, id_bin, event)
  end
end
