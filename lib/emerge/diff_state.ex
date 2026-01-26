defmodule Emerge.DiffState do
  @moduledoc """
  Stateful diff helper that keeps numeric id assignments stable.
  """

  alias Emerge.Reconcile
  alias Emerge.VNode

  @type t :: %__MODULE__{
          tree: Emerge.Element.t() | nil,
          vdom: VNode.t() | nil,
          event_registry: %{binary() => %{atom() => {pid(), term()}}}
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
  @spec diff_and_encode(t(), Emerge.Element.t()) :: {binary(), t(), Emerge.Element.t()}
  def diff_and_encode(%__MODULE__{} = state, tree) do
    {vdom, patches, assigned} = Reconcile.reconcile(state.vdom, tree)

    {
      Emerge.Patch.encode(patches),
      %__MODULE__{tree: assigned, vdom: vdom, event_registry: build_event_registry(assigned)},
      assigned
    }
  end

  @spec dispatch_click(t(), binary()) :: :ok
  def dispatch_click(%__MODULE__{} = state, id_bin) when is_binary(id_bin) do
    dispatch_event(state, id_bin, :click)
  end

  @spec dispatch_event(t(), binary(), atom()) :: :ok
  def dispatch_event(%__MODULE__{event_registry: registry}, id_bin, event)
      when is_binary(id_bin) and is_atom(event) do
    case lookup_event(%__MODULE__{event_registry: registry}, id_bin, event) do
      {pid, msg} when is_pid(pid) ->
        send(pid, msg)
        :ok

      _ ->
        :ok
    end
  end

  @spec lookup_event(t(), binary(), atom()) :: {:ok, {pid(), term()}} | :error
  def lookup_event(%__MODULE__{event_registry: registry}, id_bin, event)
      when is_binary(id_bin) and is_atom(event) do
    case Map.get(registry, id_bin, %{}) |> Map.get(event) do
      {pid, msg} when is_pid(pid) -> {:ok, {pid, msg}}
      _ -> :error
    end
  end

  def build_event_registry(tree) do
    tree
    |> collect_event_handlers(%{})
  end

  defp collect_event_handlers(%Emerge.Element{} = element, acc) do
    acc
    |> register_event(element, :on_click, :click)
    |> register_event(element, :on_mouse_down, :mouse_down)
    |> register_event(element, :on_mouse_up, :mouse_up)
    |> register_event(element, :on_mouse_enter, :mouse_enter)
    |> register_event(element, :on_mouse_leave, :mouse_leave)
    |> register_event(element, :on_mouse_move, :mouse_move)
    |> then(fn registry ->
      Enum.reduce(element.children, registry, fn child, next_registry ->
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
end
