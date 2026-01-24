defmodule Emerge.DiffState do
  @moduledoc """
  Stateful diff helper that keeps numeric id assignments stable.
  """

  alias Emerge.Reconcile
  alias Emerge.VNode

  @type t :: %__MODULE__{
          tree: Emerge.Element.t() | nil,
          vdom: VNode.t() | nil,
          click_registry: %{binary() => {pid(), term()}}
        }

  defstruct tree: nil, vdom: nil, click_registry: %{}

  @doc """
  Initialize diff state with an optional tree.
  """
  def new(tree \\ nil)

  def new(nil), do: %__MODULE__{}

  def new(tree) do
    {vdom, tree} = Reconcile.assign_ids(tree)
    %__MODULE__{tree: tree, vdom: vdom, click_registry: build_click_registry(tree)}
  end

  @doc """
  Compute patches for a new tree and return {patch_binary, updated_state, assigned_tree}.
  """
  @spec diff_and_encode(t(), Emerge.Element.t()) :: {binary(), t(), Emerge.Element.t()}
  def diff_and_encode(%__MODULE__{} = state, tree) do
    {vdom, patches, assigned} = Reconcile.reconcile(state.vdom, tree)

    {
      Emerge.Patch.encode(patches),
      %__MODULE__{tree: assigned, vdom: vdom, click_registry: build_click_registry(assigned)},
      assigned
    }
  end

  @spec dispatch_click(t(), binary()) :: :ok
  def dispatch_click(%__MODULE__{click_registry: registry}, id_bin) when is_binary(id_bin) do
    case Map.get(registry, id_bin) do
      {pid, msg} when is_pid(pid) ->
        send(pid, msg)
        :ok

      _ ->
        :ok
    end
  end

  def build_click_registry(tree) do
    tree
    |> collect_click_handlers(%{})
  end

  defp collect_click_handlers(%Emerge.Element{} = element, acc) do
    acc =
      case Map.get(element.attrs, :on_click) do
        {pid, msg} when is_pid(pid) ->
          Map.put(acc, :erlang.term_to_binary(element.id), {pid, msg})

        _ ->
          acc
      end

    Enum.reduce(element.children, acc, fn child, registry ->
      collect_click_handlers(child, registry)
    end)
  end
end
