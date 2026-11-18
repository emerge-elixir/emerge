defmodule Emerge.Engine.Reconcile do
  @moduledoc "Match occurrences on entry, compare exact local inputs and fold a sparse replacement on exit."
  alias Emerge.Engine.{
    AttrCodec,
    AttrValidation,
    Element,
    EventRegistry,
    Patch,
    ScrollTarget,
    VNode
  }

  alias Emerge.Engine.Reconcile.{Children, Labels}

  defmodule Index do
    @moduledoc false
    defstruct nodes: %{},
              keys: %{},
              routes: %{},
              own: %Labels{},
              branch: %Labels{},
              ancestry: %Labels{},
              next_id: 1

    def vacate(index) do
      %__MODULE__{
        own: %Labels{next: index.own.next},
        branch: %Labels{next: index.branch.next},
        ancestry: %Labels{next: index.ancestry.next},
        next_id: index.next_id
      }
    end
  end

  defmodule Fold do
    @moduledoc false
    defstruct [:node, :assigned, :body, :cover, :stable]
  end

  @axes [:scroll_position_x, :scroll_position_y]
  @inherited ~w(layout_scale font font_size font_weight font_style font_color font_underline font_strike font_letter_spacing font_word_spacing text_align snap_text_metrics)a
  @max_id 0xFFFFFFFFFFFFFFFF

  def assign_ids(tree) do
    {v, e, _} = assign_ids(tree, 1)
    {v, e}
  end

  def assign_ids(tree, next_id) do
    {v, _, e, _, next} = update(nil, tree, next_id, %{}, :assigned)
    {v, e, next}
  end

  def reconcile(old, tree) do
    {v, ops, e, _} = reconcile(old, tree, if(old, do: old.index.next_id, else: 1))
    {v, ops, e}
  end

  def reconcile(old, tree, next_id) do
    {v, ops, e, _, next} = update(old, tree, next_id, routes(old), :assigned)
    {v, public(ops), e, next}
  end

  def reconcile_patches(old, tree, next_id) do
    {v, ops, _, _, next} = update(old, tree, next_id, routes(old), :binary)
    {v, public(ops), next}
  end

  def reconcile_patches_and_event_registry(old, tree, next_id) do
    reconcile_patches_and_event_registry(old, tree, next_id, routes(old))
  end

  def reconcile_patches_and_event_registry(old, tree, next_id, routes) do
    {v, ops, _, routes, next} = update(old, tree, next_id, routes, :binary)
    {v, public(ops), routes, next}
  end

  defp routes(nil), do: %{}
  defp routes(root), do: root.index.routes

  # Detached callers already own assigned trees, but have no saved index. Only
  # this interface has to import the first declaration; DiffState never does.
  def diff_assigned(old, new) do
    {v, _, _, _, next} = update(nil, old, 1, %{}, :import)
    {_, ops, _, _, _} = update(v, new, next, %{}, :import)
    public(ops)
  end

  defp public(ops),
    do: Enum.map(ops, fn {:patch, target, body} -> Patch.patch(target, public_body(body)) end)

  defp public_body({:node, id, kind, attrs, edges}),
    do:
      {:node, id, kind, if(is_binary(attrs), do: AttrCodec.decode_attrs(attrs), else: attrs),
       if(is_list(edges),
         do: Enum.map(edges, fn {tag, c} -> {tag, public_body(c)} end),
         else: edges
       )}

  defp public_body(ref), do: ref

  @doc false
  def update(root, %Element{} = tree, next_id, routes, mode, vacant \\ %Index{}) do
    if Map.has_key?(tree.attrs, :animate_exit),
      do: raise(ArgumentError, "animate_exit is not allowed on the viewport root")

    old = if root, do: root.index, else: vacant

    ctx = %{
      old: old,
      index: old,
      claims: MapSet.new(),
      displaced: [],
      seen_keys: %{},
      routes: routes,
      mode: mode,
      next: max(old.next_id, next_id)
    }

    pos = if root, do: Map.fetch!(old.nodes, root.id)
    {result, ctx} = enter(tree, pos, nil, :root, ctx)
    {result, ctx} = settle(result, ctx)
    id = result.node.id
    ctx = if root && root.id != id, do: retire(pos, ctx), else: ctx
    ctx = Enum.reduce(ctx.displaced, ctx, &retire/2)
    idx = ctx.index

    idx = %{
      idx
      | own: Labels.finish(idx.own),
        branch: Labels.finish(idx.branch),
        ancestry: Labels.finish(idx.ancestry),
        routes:
          case ctx.routes do
            %EventRegistry.Changes{registry: registry} -> registry
            registry -> registry
          end,
        next_id: ctx.next
    }

    ops =
      cond do
        is_nil(root) -> [Patch.patch(0, result.body)]
        root.id != id -> [Patch.patch(root.id, result.body)]
        is_nil(result.cover) -> []
        true -> [result.cover]
      end

    {%{result.node | index: idx}, ops, result.assigned, ctx.routes, ctx.next}
  end

  defp enter(element, pos, parent, site, ctx) do
    old = occurrence(element, pos, ctx)

    {id, ctx} =
      cond do
        ctx.mode == :import and element.id in 1..@max_id ->
          {element.id, %{ctx | next: max(ctx.next, element.id + 1)}}

        old ->
          {old.id, ctx}

        true ->
          fresh(ctx)
      end

    ctx = claim(ctx, element.key, id)

    {ancestry, ctx} =
      if site == :root,
        do: {0, drop_site(ctx, old)},
        else: bind(ctx, :ancestry, part(old, :ancestry), site)

    {inherited, ctx} =
      if old && old.attrs === element.attrs do
        {old.inherited, ctx}
      else
        bind(
          ctx,
          :own,
          old && old.inherited,
          AttrCodec.encode_attrs(Map.take(element.attrs, @inherited))
        )
      end

    parent_site = {ancestry, id, inherited}

    {cs, pending_cs, ctx} =
      walk(element.children, if(old, do: old.children, else: []), :child, parent_site, ctx)

    {ns, pending_ns, ctx} =
      walk(element.nearby, if(old, do: old.nearby, else: []), :nearby, parent_site, ctx)

    {attrs, ctx} = scroll_declarations(element.attrs, old, ctx)

    node = %VNode{
      id: id,
      kind: element.type,
      key: element.key,
      attrs: attrs,
      owner: parent,
      inherited: inherited
    }

    exit = fn ctx -> close(node, old, element, ancestry, cs, ns, ctx) end

    if pending_cs or pending_ns or waiting?(attrs, ctx.seen_keys),
      do: {exit, ctx},
      else: exit.(ctx)
  end

  defp occurrence(%{id: id, type: kind}, _, %{mode: :import, old: old}) do
    case old.nodes do
      %{^id => %{kind: ^kind} = v} -> v
      %{^id => _} -> raise ArgumentError, "assigned ID changed kind"
      _ -> nil
    end
  end

  defp occurrence(%{key: nil, type: kind}, %{key: nil, kind: kind} = old, _), do: old
  defp occurrence(%{key: nil}, _, _), do: nil

  defp occurrence(%{key: key, type: kind}, _, ctx) do
    with {:ok, id} <- Map.fetch(ctx.old.keys, key),
         %{kind: ^kind} = old <- Map.fetch!(ctx.old.nodes, id) do
      old
    else
      _ -> nil
    end
  end

  defp fresh(%{next: id} = ctx) when id in 1..@max_id, do: {id, %{ctx | next: id + 1}}
  defp fresh(_), do: raise(ArgumentError, "occurrence ID exhausted")

  defp claim(ctx, key, id) do
    if key != nil and Map.has_key?(ctx.seen_keys, key),
      do: raise(ArgumentError, "duplicate explicit key: #{inspect(key)}")

    if MapSet.member?(ctx.claims, id), do: raise(ArgumentError, "duplicate occurrence ID")
    ctx = %{ctx | claims: MapSet.put(ctx.claims, id)}

    if is_nil(key) do
      ctx
    else
      index = ctx.index

      keys =
        if Map.get(index.keys, key) === id, do: index.keys, else: Map.put(index.keys, key, id)

      %{ctx | index: %{index | keys: keys}, seen_keys: Map.put(ctx.seen_keys, key, id)}
    end
  end

  defp walk(items, old, group, site, ctx),
    do: siblings(items, old, group, site, 0, nil, [], false, ctx)

  defp siblings([], _, _, _, _, _, acc, pending, ctx), do: {Enum.reverse(acc), pending, ctx}

  defp siblings([entry | tail], cursor, group, site, i, keyed, acc, pending, ctx) do
    {tag, child} = Children.edge(entry)
    mode = not is_nil(child.key)

    if keyed != nil and keyed != mode do
      raise ArgumentError,
            if(group == :child,
              do: "All siblings must have key when any key is provided",
              else: "All nearby mounts on a host must have key when any key is provided"
            )
    end

    {pos, cursor} =
      case cursor do
        [] -> {nil, []}
        [head | rest] -> {elem(Children.edge(head), 1), rest}
      end

    {ancestry, parent, inherited} = site
    {child, ctx} = enter(child, pos, parent, {ancestry, parent, inherited, tag, i}, ctx)
    entry = if group == :child, do: child, else: {tag, child}

    siblings(
      tail,
      cursor,
      group,
      site,
      i + 1,
      mode,
      [entry | acc],
      pending or is_function(child, 1),
      ctx
    )
  end

  defp settle(fun, ctx) when is_function(fun, 1), do: fun.(ctx)
  defp settle(done, ctx), do: {done, ctx}

  defp close(%VNode{} = node, old, element, ancestry, cs, ns, ctx) do
    {cs, ctx} = Children.close(cs, if(old, do: old.children, else: []), ctx, &settle/2)
    {ns, ctx} = Children.close(ns, if(old, do: old.nearby, else: []), ctx, &settle/2)

    attrs = resolve(node.attrs, ctx.seen_keys)
    same_attrs = old != nil and old.attrs === attrs
    raw = if same_attrs, do: old.native, else: AttrCodec.encode_attrs(attrs)
    {own, ctx} = if same_attrs, do: {old.own, ctx}, else: bind(ctx, :own, old && old.own, raw)
    own_equal = old != nil and old.own == own
    if old && !own_equal, do: AttrValidation.validate_change_update!(old.attrs, attrs)
    events = if same_attrs, do: old.events, else: EventRegistry.node_events(attrs)

    old_events = if old, do: old.events, else: %{}

    routes =
      if events === old_events,
        do: ctx.routes,
        else: EventRegistry.put_events(ctx.routes, node.id, events)

    order_equal = old != nil and cs.order_equal and ns.order_equal
    changed = Children.merge(cs.cover, ns.cover)
    list_same = order_equal and changed == nil

    values = %VNode{
      node
      | attrs: attrs,
        events: events,
        native: raw,
        own: own,
        children: cs.nodes,
        nearby: ns.nodes
    }

    {branch, ctx} =
      if list_same and own_equal do
        {part(old, :branch), ctx}
      else
        bind(
          ctx,
          :branch,
          part(old, :branch),
          {node.kind, own, Patch.edges(values, &part(&1, :branch))}
        )
      end

    node = %{values | stamp: <<branch::32, ancestry::32>>}

    {body, cover} =
      if list_same and own_equal do
        {node.id, nil}
      else
        body =
          {:node, node.id, node.kind, if(own_equal, do: :same, else: raw),
           if(list_same, do: :same, else: Children.emit(cs, Children.emit(ns, [])))}

        cover =
          if own_equal and order_equal and is_tuple(changed),
            do: changed,
            else: Patch.patch(node.id, body)

        {body, cover}
      end

    stable =
      same_attrs and order_equal and old.owner == node.owner and old.stamp == node.stamp and
        cs.stable and ns.stable

    idx = ctx.index

    {node, idx} =
      if stable, do: {old, idx}, else: {node, %{idx | nodes: Map.put(idx.nodes, node.id, node)}}

    assigned =
      if ctx.mode == :assigned,
        do: %{
          element
          | id: node.id,
            attrs: attrs,
            children: cs.assigned,
            nearby: ns.assigned
        }

    {%Fold{node: node, assigned: assigned, body: body, cover: cover, stable: stable},
     %{ctx | index: idx, routes: routes}}
  end

  defp part(nil, _), do: nil
  defp part(%{stamp: <<b::32, _::32>>}, :branch), do: b
  defp part(%{stamp: <<_::32, a::32>>}, :ancestry), do: a

  defp bind(ctx, pool, old, descriptor) do
    {id, labels} = Labels.bind(Map.fetch!(ctx.index, pool), old, descriptor)
    {id, %{ctx | index: Map.put(ctx.index, pool, labels)}}
  end

  defp drop_site(ctx, old),
    do: put_in(ctx.index.ancestry, Labels.release(ctx.index.ancestry, part(old, :ancestry)))

  defp retire(node, ctx) do
    if MapSet.member?(ctx.claims, node.id) or not Map.has_key?(ctx.index.nodes, node.id) do
      ctx
    else
      idx = ctx.index

      keys =
        if node.key != nil and Map.get(idx.keys, node.key) == node.id,
          do: Map.delete(idx.keys, node.key),
          else: idx.keys

      idx = %{
        idx
        | nodes: Map.delete(idx.nodes, node.id),
          keys: keys,
          branch: Labels.release(idx.branch, part(node, :branch)),
          ancestry: Labels.release(idx.ancestry, part(node, :ancestry)),
          own: idx.own |> Labels.release(node.own) |> Labels.release(node.inherited)
      }

      routes =
        if map_size(node.events) > 0,
          do: EventRegistry.delete_node(ctx.routes, node.id),
          else: ctx.routes

      ctx = Enum.reduce(node.children, %{ctx | index: idx, routes: routes}, &retire/2)
      Enum.reduce(node.nearby, ctx, fn {_, c}, ctx -> retire(c, ctx) end)
    end
  end

  defp scroll_declarations(attrs, old, ctx) do
    Enum.reduce(@axes, {attrs, ctx}, fn axis, {attrs, ctx} ->
      case Map.fetch(attrs, axis) do
        {:ok, %ScrollTarget{}} ->
          {attrs, ctx}

        {:ok, key} when not is_float(key) ->
          Emerge.UI.Scroll.validate_position!(key)

          {token, ctx} =
            case old && Map.get(old.attrs, axis) do
              %ScrollTarget{key: ^key, declaration: token} -> {token, ctx}
              _ -> fresh(ctx)
            end

          {Map.put(attrs, axis, %ScrollTarget{key: key, declaration: token, id: 0}), ctx}

        {:ok, fraction} ->
          Emerge.UI.Scroll.validate_position!(fraction)
          {attrs, ctx}

        :error ->
          {attrs, ctx}
      end
    end)
  end

  defp waiting?(attrs, seen),
    do:
      Enum.any?(@axes, fn axis ->
        case attrs do
          %{^axis => %ScrollTarget{key: key}} when key != nil -> not Map.has_key?(seen, key)
          _ -> false
        end
      end)

  defp resolve(attrs, seen),
    do:
      Enum.reduce(@axes, attrs, fn axis, attrs ->
        case attrs do
          %{^axis => %ScrollTarget{key: key} = ref} when key != nil ->
            Map.put(attrs, axis, %{ref | id: Map.get(seen, key, 0)})

          _ ->
            attrs
        end
      end)
end
