defmodule Emerge.DiffState do
  @moduledoc """
  Stateful diff helper that keeps numeric id assignments stable.
  """

  alias Emerge.Reconcile
  alias Emerge.VNode

  @type t :: %__MODULE__{
          tree: Emerge.Element.t() | nil,
          vdom: VNode.t() | nil
        }

  defstruct tree: nil, vdom: nil

  @doc """
  Initialize diff state with an optional tree.
  """
  def new(tree \\ nil)

  def new(nil), do: %__MODULE__{}

  def new(tree) do
    {vdom, tree} = Reconcile.assign_ids(tree)
    %__MODULE__{tree: tree, vdom: vdom}
  end

  @doc """
  Compute patches for a new tree and return {patch_binary, updated_state, assigned_tree}.
  """
  @spec diff_and_encode(t(), Emerge.Element.t()) :: {binary(), t(), Emerge.Element.t()}
  def diff_and_encode(%__MODULE__{} = state, tree) do
    {vdom, patches, assigned} = Reconcile.reconcile(state.vdom, tree)
    {Emerge.Patch.encode(patches), %__MODULE__{tree: assigned, vdom: vdom}, assigned}
  end
end
