defmodule Emerge.Engine.Reconcile.Children do
  @moduledoc false
  # Fold one ordered edge list against an old cursor. No recursive VNode
  # equality. Build a wire list only after the first changed edge or branch.
  defstruct nodes: [], assigned: [], wire: nil, order_equal: true, stable: true, cover: nil

  def close(folds, old, ctx, settle), do: close(folds, old, %__MODULE__{}, ctx, settle)

  defp close([], old_tail, list, ctx, _) do
    ctx = Enum.reduce(old_tail, ctx, &displace/2)

    {%{
       list
       | nodes: Enum.reverse(list.nodes),
         assigned: Enum.reverse(list.assigned),
         order_equal: list.order_equal and old_tail == []
     }, ctx}
  end

  defp close([entry | rest], old, list, ctx, settle) do
    {tag, pending} = edge(entry)
    {fold, ctx} = settle.(pending, ctx)

    {prior, tail} =
      case old do
        [] -> {nil, []}
        [p | tail] -> {p, tail}
      end

    same = prior != nil and bare(prior) == {tag, fold.node.id}
    ctx = if same or prior == nil, do: ctx, else: displace(prior, ctx)

    wire =
      cond do
        list.wire != nil -> [{tag, fold.body} | list.wire]
        same and fold.cover == nil -> nil
        true -> [{tag, fold.body} | Enum.map(list.nodes, &bare/1)]
      end

    next = %{
      list
      | nodes: [wrap(tag, fold.node) | list.nodes],
        wire: wire,
        assigned:
          if(ctx.mode == :assigned, do: [wrap(tag, fold.assigned) | list.assigned], else: []),
        order_equal: list.order_equal and same,
        stable: list.stable and same and fold.stable,
        cover: merge(list.cover, fold.cover)
    }

    close(rest, tail, next, ctx, settle)
  end

  defp displace(entry, ctx) do
    {_, node} = edge(entry)

    if MapSet.member?(ctx.claims, node.id),
      do: ctx,
      else: %{ctx | displaced: [node | ctx.displaced]}
  end

  def emit(list, tail) do
    case list.wire do
      nil -> List.foldr(list.nodes, tail, fn n, acc -> [bare(n) | acc] end)
      rev -> Enum.reverse(rev, tail)
    end
  end

  def merge(nil, b), do: b
  def merge(a, nil), do: a
  def merge(_, _), do: :many
  def edge({tag, child}), do: {tag, child}
  def edge(child), do: {:child, child}

  defp bare(entry) do
    {tag, node} = edge(entry)
    {tag, node.id}
  end

  defp wrap(:child, n), do: n
  defp wrap(tag, n), do: {tag, n}
end
