defmodule Emerge.Engine.Serialization do
  @moduledoc """
  Binary serialization for Emerge.Engine.Element trees.
  """
  alias Emerge.Engine.{Element, Patch, Reconcile}

  def encode(tree) do
    {_, assigned} = Reconcile.assign_ids(tree)
    {encode_tree(assigned), assigned}
  end

  def encode_tree(%Element{} = tree), do: Patch.encode(Patch.patch(0, Patch.full(tree)))

  def decode(bytes) do
    case Patch.decode(bytes) do
      [{:patch, 0, nil}] -> nil
      [{:patch, 0, root}] -> element(root)
      _ -> raise ArgumentError, "expected a full declaration"
    end
  end

  defp element({:node, id, kind, attrs, edges}) when is_map(attrs) and is_list(edges) do
    {cs, ns} = Enum.split_while(edges, fn {tag, _} -> tag == :child end)

    %Element{
      id: id,
      type: kind,
      attrs: attrs,
      children: Enum.map(cs, fn {_, c} -> element(c) end),
      nearby: Enum.map(ns, fn {slot, c} -> {slot, element(c)} end)
    }
  end

  defp element(_), do: raise(ArgumentError, "keep in full declaration")
end
