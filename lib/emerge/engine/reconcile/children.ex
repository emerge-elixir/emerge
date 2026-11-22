defmodule Emerge.Engine.Reconcile.Children do
  @moduledoc false
  # Settle in declaration order. Build output on return so a stable suffix
  # (often the entire accepted list) is reused without comparing VNode trees.
  defstruct nodes: [], assigned: [], wire: nil, order_equal: true, stable: true, cover: nil

  def finish([], ctx), do: {%__MODULE__{}, ctx}

  def finish(old_tail, ctx),
    do: {%__MODULE__{order_equal: false, stable: false}, Enum.reduce(old_tail, ctx, &displace/2)}

  def prepend(tag, fold, old, suffix, ctx) do
    same = old != [] and bare(hd(old)) == {tag, fold.node.id}
    ctx = if same or old == [], do: ctx, else: displace(hd(old), ctx)
    stable = same and fold.stable and suffix.stable

    wire =
      cond do
        suffix.wire != nil -> [{tag, fold.body} | suffix.wire]
        same and fold.cover == nil -> nil
        true -> [{tag, fold.body} | Enum.map(suffix.nodes, &bare/1)]
      end

    {%__MODULE__{
       nodes: if(stable, do: old, else: [wrap(tag, fold.node) | suffix.nodes]),
       assigned:
         if(ctx.mode == :assigned, do: [wrap(tag, fold.assigned) | suffix.assigned], else: []),
       wire: wire,
       order_equal: same and suffix.order_equal,
       stable: stable,
       cover: merge(fold.cover, suffix.cover)
     }, ctx}
  end

  defp displace(entry, ctx) do
    {_, node} = edge(entry)

    if Emerge.Engine.Reconcile.claimed?(ctx, node),
      do: ctx,
      else: %{ctx | displaced: [node | ctx.displaced]}
  end

  def emit(list, tail) do
    case list.wire do
      nil -> List.foldr(list.nodes, tail, fn n, acc -> [bare(n) | acc] end)
      edges -> edges ++ tail
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
