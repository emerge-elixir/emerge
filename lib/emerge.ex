defmodule Emerge do
  @moduledoc """
  Tree definition, identity, and EMRG encoding helpers.
  """

  alias Emerge.DiffState
  alias Emerge.Element

  @doc """
  Initialize a diff state for incremental updates.
  """
  @spec diff_state_new(Element.t() | nil) :: DiffState.t()
  def diff_state_new(tree \\ nil) do
    DiffState.new(tree)
  end

  @doc """
  Compute patches for a new tree and return {patch_binary, next_state, assigned_tree}.
  """
  @spec diff_state_update(DiffState.t(), Element.t()) :: {binary(), DiffState.t(), Element.t()}
  def diff_state_update(state, tree) do
    DiffState.diff_and_encode(state, tree)
  end

  @doc """
  Encode a full tree with ids and return {binary, next_state, assigned_tree}.
  """
  @spec encode_full(DiffState.t(), Element.t()) :: {binary(), DiffState.t(), Element.t()}
  def encode_full(%DiffState{} = state, tree) do
    {vdom, assigned} = Emerge.Reconcile.assign_ids(tree)
    {Emerge.Serialization.encode_tree(assigned), %DiffState{state | tree: assigned, vdom: vdom},
     assigned}
  end

  @doc """
  Encode a full tree and return {full_binary, patch_binary, next_state, assigned_tree}.

  The patch binary is always empty for initial uploads.
  """
  @spec encode_full_with_empty_patch(DiffState.t(), Element.t()) ::
          {binary(), binary(), DiffState.t(), Element.t()}
  def encode_full_with_empty_patch(%DiffState{} = state, tree) do
    {full_bin, next_state, assigned} = encode_full(state, tree)
    {full_bin, <<>>, next_state, assigned}
  end
end
