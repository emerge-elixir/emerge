defmodule Emerge.Engine.Reconcile do
  @moduledoc """
  Reconcile Emerge.Engine.Element trees into stable node ids and patch operations.
  """

  alias Emerge.Engine.Element
  alias Emerge.Engine.Patch
  alias Emerge.Engine.Tree.Attrs, as: TreeAttrs
  alias Emerge.Engine.VNode

  @type scope_ref :: :root | {:children, non_neg_integer()} | {:nearby, non_neg_integer()}

  @type ctx :: %{
          next_id: non_neg_integer(),
          seen: MapSet.t(),
          old_key_index: %{optional(term()) => %{scope: scope_ref(), vnode: VNode.t()}}
        }

  @type result :: {VNode.t(), [Patch.patch()], Element.t()}

  @doc """
  Assign fresh node ids to a tree without a previous version.
  """
  @spec assign_ids(Element.t()) :: {VNode.t(), Element.t()}
  def assign_ids(%Element{} = element) do
    {vnode, assigned, _next_node_id} = assign_ids(element, 1)
    {vnode, assigned}
  end

  @spec assign_ids(Element.t(), non_neg_integer()) :: {VNode.t(), Element.t(), non_neg_integer()}
  def assign_ids(%Element{} = element, next_id)
      when is_integer(next_id) and next_id > 0 do
    validate_viewport_root!(element)

    ctx = %{next_id: next_id, seen: MapSet.new(), old_key_index: %{}}
    {vnode, assigned, ctx} = build_fresh_subtree(element, ctx)
    {vnode, assigned, ctx.next_id}
  end

  @doc """
  Reconcile a new tree against the previous vdom.
  """
  @spec reconcile(VNode.t() | nil, Element.t()) :: result()
  def reconcile(old_vnode, %Element{} = element) do
    {vnode, patches, assigned, _next_node_id} = reconcile(old_vnode, element, 1)
    {vnode, patches, assigned}
  end

  @spec reconcile(VNode.t() | nil, Element.t(), non_neg_integer()) ::
          {VNode.t(), [Patch.patch()], Element.t(), non_neg_integer()}
  def reconcile(nil, %Element{} = element, next_id)
      when is_integer(next_id) and next_id > 0 do
    validate_viewport_root!(element)

    ctx = %{next_id: next_id, seen: MapSet.new(), old_key_index: %{}}
    {vnode, assigned, ctx} = build_fresh_subtree(element, ctx)
    {vnode, [], assigned, ctx.next_id}
  end

  def reconcile(%VNode{} = old_vnode, %Element{} = element, next_id)
      when is_integer(next_id) and next_id > 0 do
    validate_viewport_root!(element)

    ctx = %{
      next_id: next_id,
      seen: MapSet.new(),
      old_key_index: build_old_key_index(old_vnode)
    }

    if reusable_root?(old_vnode, element) do
      {vnode, patches, assigned, ctx} = reconcile_matched_node(old_vnode, element, ctx)
      {vnode, patches, assigned, ctx.next_id}
    else
      {vnode, assigned, ctx} = build_fresh_subtree(element, ctx)

      {vnode, [{:remove, old_vnode.id}, {:insert_subtree, nil, 0, assigned}], assigned,
       ctx.next_id}
    end
  end

  defp reconcile_matched_node(%VNode{} = old, %Element{} = element, ctx) do
    key = element_key(element)
    ctx = ensure_unique_key!(ctx, key)

    {child_vnodes, child_elements, child_patches, ctx} =
      reconcile_children(old.children, element.children, old.id, ctx)

    {nearby_vnodes, nearby_elements, nearby_patches, ctx} =
      reconcile_nearby(old.nearby, element.nearby, old.id, ctx)

    new_attrs = element.attrs

    parent_patches =
      []
      |> maybe_set_nearby_mounts(old, nearby_vnodes)
      |> maybe_set_children(old, child_vnodes)
      |> maybe_set_attrs(old, new_attrs, old.id)

    patches_rev =
      []
      |> prepend_many(nearby_patches)
      |> prepend_many(child_patches)
      |> prepend_many(parent_patches)

    patches = Enum.reverse(patches_rev)

    vnode = %VNode{
      id: old.id,
      kind: element.type,
      key: key,
      attrs: new_attrs,
      children: child_vnodes,
      nearby: nearby_vnodes
    }

    assigned = %{
      element
      | id: old.id,
        attrs: new_attrs,
        children: child_elements,
        nearby: nearby_elements
    }

    {vnode, patches, assigned, ctx}
  end

  defp reconcile_children(old_children, new_children, parent_node_id, ctx) do
    case children_mode(new_children) do
      :keyed -> reconcile_children_keyed(old_children, new_children, parent_node_id, ctx)
      :unkeyed -> reconcile_children_unkeyed(old_children, new_children, parent_node_id, ctx)
    end
  end

  defp reconcile_children_keyed(old_children, new_children, parent_node_id, ctx) do
    scope = {:children, parent_node_id}

    {vnodes_rev, elements_rev, patches_rev, used_old_ids, ctx} =
      do_reconcile_children_keyed(
        new_children,
        0,
        scope,
        parent_node_id,
        ctx,
        [],
        [],
        [],
        MapSet.new()
      )

    patches_rev = prepend_removed_children(old_children, used_old_ids, patches_rev)

    {Enum.reverse(vnodes_rev), Enum.reverse(elements_rev), Enum.reverse(patches_rev), ctx}
  end

  defp do_reconcile_children_keyed(
         [],
         _index,
         _scope,
         _parent_node_id,
         ctx,
         vnodes_rev,
         elements_rev,
         patches_rev,
         used_old_ids
       ) do
    {vnodes_rev, elements_rev, patches_rev, used_old_ids, ctx}
  end

  defp do_reconcile_children_keyed(
         [child | rest],
         index,
         scope,
         parent_node_id,
         ctx,
         vnodes_rev,
         elements_rev,
         patches_rev,
         used_old_ids
       ) do
    key = element_key(child)

    case Map.get(ctx.old_key_index, key) do
      %{scope: ^scope, vnode: %VNode{kind: kind} = old_child} when kind == child.type ->
        {vnode, child_patches, assigned, ctx} = reconcile_matched_node(old_child, child, ctx)

        do_reconcile_children_keyed(
          rest,
          index + 1,
          scope,
          parent_node_id,
          ctx,
          [vnode | vnodes_rev],
          [assigned | elements_rev],
          prepend_many(patches_rev, child_patches),
          MapSet.put(used_old_ids, old_child.id)
        )

      _ ->
        {vnode, assigned, ctx} = build_fresh_subtree(child, ctx)

        do_reconcile_children_keyed(
          rest,
          index + 1,
          scope,
          parent_node_id,
          ctx,
          [vnode | vnodes_rev],
          [assigned | elements_rev],
          [{:insert_subtree, parent_node_id, index, assigned} | patches_rev],
          used_old_ids
        )
    end
  end

  defp reconcile_children_unkeyed(old_children, new_children, parent_node_id, ctx) do
    {vnodes_rev, elements_rev, patches_rev, ctx} =
      do_reconcile_children_unkeyed(
        old_children,
        new_children,
        0,
        parent_node_id,
        ctx,
        [],
        [],
        []
      )

    {Enum.reverse(vnodes_rev), Enum.reverse(elements_rev), Enum.reverse(patches_rev), ctx}
  end

  defp do_reconcile_children_unkeyed(
         [],
         [],
         _index,
         _parent_node_id,
         ctx,
         vnodes_rev,
         elements_rev,
         patches_rev
       ) do
    {vnodes_rev, elements_rev, patches_rev, ctx}
  end

  defp do_reconcile_children_unkeyed(
         [%VNode{} = old_child | old_rest],
         [%Element{} = child | new_rest],
         index,
         parent_node_id,
         ctx,
         vnodes_rev,
         elements_rev,
         patches_rev
       ) do
    if old_child.kind == child.type and old_child.key == nil and not has_key?(child) do
      {vnode, child_patches, assigned, ctx} = reconcile_matched_node(old_child, child, ctx)

      do_reconcile_children_unkeyed(
        old_rest,
        new_rest,
        index + 1,
        parent_node_id,
        ctx,
        [vnode | vnodes_rev],
        [assigned | elements_rev],
        prepend_many(patches_rev, child_patches)
      )
    else
      {vnode, assigned, ctx} = build_fresh_subtree(child, ctx)

      do_reconcile_children_unkeyed(
        old_rest,
        new_rest,
        index + 1,
        parent_node_id,
        ctx,
        [vnode | vnodes_rev],
        [assigned | elements_rev],
        [
          {:insert_subtree, parent_node_id, index, assigned},
          {:remove, old_child.id} | patches_rev
        ]
      )
    end
  end

  defp do_reconcile_children_unkeyed(
         [],
         [%Element{} = child | new_rest],
         index,
         parent_node_id,
         ctx,
         vnodes_rev,
         elements_rev,
         patches_rev
       ) do
    {vnode, assigned, ctx} = build_fresh_subtree(child, ctx)

    do_reconcile_children_unkeyed(
      [],
      new_rest,
      index + 1,
      parent_node_id,
      ctx,
      [vnode | vnodes_rev],
      [assigned | elements_rev],
      [{:insert_subtree, parent_node_id, index, assigned} | patches_rev]
    )
  end

  defp do_reconcile_children_unkeyed(
         [%VNode{} = old_child | old_rest],
         [],
         index,
         parent_node_id,
         ctx,
         vnodes_rev,
         elements_rev,
         patches_rev
       ) do
    do_reconcile_children_unkeyed(
      old_rest,
      [],
      index + 1,
      parent_node_id,
      ctx,
      vnodes_rev,
      elements_rev,
      [{:remove, old_child.id} | patches_rev]
    )
  end

  defp reconcile_nearby(old_nearby, new_nearby, host_node_id, ctx) do
    case nearby_mode(new_nearby) do
      :keyed -> reconcile_nearby_keyed(old_nearby, new_nearby, host_node_id, ctx)
      :unkeyed -> reconcile_nearby_unkeyed(old_nearby, new_nearby, host_node_id, ctx)
    end
  end

  defp reconcile_nearby_keyed(old_nearby, new_nearby, host_node_id, ctx) do
    scope = {:nearby, host_node_id}

    {vnodes_rev, elements_rev, patches_rev, used_old_ids, ctx} =
      do_reconcile_nearby_keyed(
        new_nearby,
        0,
        scope,
        host_node_id,
        ctx,
        [],
        [],
        [],
        MapSet.new()
      )

    patches_rev = prepend_removed_nearby(old_nearby, used_old_ids, patches_rev)

    {Enum.reverse(vnodes_rev), Enum.reverse(elements_rev), Enum.reverse(patches_rev), ctx}
  end

  defp do_reconcile_nearby_keyed(
         [],
         _index,
         _scope,
         _host_node_id,
         ctx,
         vnodes_rev,
         elements_rev,
         patches_rev,
         used_old_ids
       ) do
    {vnodes_rev, elements_rev, patches_rev, used_old_ids, ctx}
  end

  defp do_reconcile_nearby_keyed(
         [{slot, element} | rest],
         index,
         scope,
         host_node_id,
         ctx,
         vnodes_rev,
         elements_rev,
         patches_rev,
         used_old_ids
       ) do
    key = element_key(element)

    case Map.get(ctx.old_key_index, key) do
      %{scope: ^scope, vnode: %VNode{kind: kind} = old_vnode} when kind == element.type ->
        {vnode, mount_patches, assigned, ctx} = reconcile_matched_node(old_vnode, element, ctx)

        do_reconcile_nearby_keyed(
          rest,
          index + 1,
          scope,
          host_node_id,
          ctx,
          [{slot, vnode} | vnodes_rev],
          [{slot, assigned} | elements_rev],
          prepend_many(patches_rev, mount_patches),
          MapSet.put(used_old_ids, old_vnode.id)
        )

      _ ->
        {vnode, assigned, ctx} = build_fresh_subtree(element, ctx)

        do_reconcile_nearby_keyed(
          rest,
          index + 1,
          scope,
          host_node_id,
          ctx,
          [{slot, vnode} | vnodes_rev],
          [{slot, assigned} | elements_rev],
          [{:insert_nearby_subtree, host_node_id, index, slot, assigned} | patches_rev],
          used_old_ids
        )
    end
  end

  defp reconcile_nearby_unkeyed(old_nearby, new_nearby, host_node_id, ctx) do
    {vnodes_rev, elements_rev, patches_rev, ctx} =
      do_reconcile_nearby_unkeyed(old_nearby, new_nearby, 0, host_node_id, ctx, [], [], [])

    {Enum.reverse(vnodes_rev), Enum.reverse(elements_rev), Enum.reverse(patches_rev), ctx}
  end

  defp do_reconcile_nearby_unkeyed(
         [],
         [],
         _index,
         _host_node_id,
         ctx,
         vnodes_rev,
         elements_rev,
         patches_rev
       ) do
    {vnodes_rev, elements_rev, patches_rev, ctx}
  end

  defp do_reconcile_nearby_unkeyed(
         [{_old_slot, %VNode{} = old_vnode} | old_rest],
         [{slot, %Element{} = element} | new_rest],
         index,
         host_node_id,
         ctx,
         vnodes_rev,
         elements_rev,
         patches_rev
       ) do
    if old_vnode.kind == element.type and old_vnode.key == nil and not has_key?(element) do
      {vnode, mount_patches, assigned, ctx} = reconcile_matched_node(old_vnode, element, ctx)

      do_reconcile_nearby_unkeyed(
        old_rest,
        new_rest,
        index + 1,
        host_node_id,
        ctx,
        [{slot, vnode} | vnodes_rev],
        [{slot, assigned} | elements_rev],
        prepend_many(patches_rev, mount_patches)
      )
    else
      {vnode, assigned, ctx} = build_fresh_subtree(element, ctx)

      do_reconcile_nearby_unkeyed(
        old_rest,
        new_rest,
        index + 1,
        host_node_id,
        ctx,
        [{slot, vnode} | vnodes_rev],
        [{slot, assigned} | elements_rev],
        [
          {:insert_nearby_subtree, host_node_id, index, slot, assigned},
          {:remove, old_vnode.id}
          | patches_rev
        ]
      )
    end
  end

  defp do_reconcile_nearby_unkeyed(
         [],
         [{slot, %Element{} = element} | new_rest],
         index,
         host_node_id,
         ctx,
         vnodes_rev,
         elements_rev,
         patches_rev
       ) do
    {vnode, assigned, ctx} = build_fresh_subtree(element, ctx)

    do_reconcile_nearby_unkeyed(
      [],
      new_rest,
      index + 1,
      host_node_id,
      ctx,
      [{slot, vnode} | vnodes_rev],
      [{slot, assigned} | elements_rev],
      [{:insert_nearby_subtree, host_node_id, index, slot, assigned} | patches_rev]
    )
  end

  defp do_reconcile_nearby_unkeyed(
         [{_old_slot, %VNode{} = old_vnode} | old_rest],
         [],
         index,
         host_node_id,
         ctx,
         vnodes_rev,
         elements_rev,
         patches_rev
       ) do
    do_reconcile_nearby_unkeyed(
      old_rest,
      [],
      index + 1,
      host_node_id,
      ctx,
      vnodes_rev,
      elements_rev,
      [{:remove, old_vnode.id} | patches_rev]
    )
  end

  defp build_fresh_subtree(%Element{} = element, ctx) do
    key = element_key(element)
    ctx = ensure_unique_key!(ctx, key)
    _ = children_mode(element.children)
    _ = nearby_mode(element.nearby)

    {id, ctx} = alloc_id(ctx)

    {child_vnodes_rev, child_elements_rev, ctx} =
      Enum.reduce(element.children, {[], [], ctx}, fn child, {vnodes_rev, elements_rev, ctx} ->
        {child_vnode, child_element, ctx} = build_fresh_subtree(child, ctx)
        {[child_vnode | vnodes_rev], [child_element | elements_rev], ctx}
      end)

    {nearby_vnodes_rev, nearby_elements_rev, ctx} =
      Enum.reduce(element.nearby, {[], [], ctx}, fn {slot, child},
                                                    {vnodes_rev, elements_rev, ctx} ->
        {nearby_vnode, nearby_element, ctx} = build_fresh_subtree(child, ctx)

        {
          [{slot, nearby_vnode} | vnodes_rev],
          [{slot, nearby_element} | elements_rev],
          ctx
        }
      end)

    child_vnodes = Enum.reverse(child_vnodes_rev)
    child_elements = Enum.reverse(child_elements_rev)
    nearby_vnodes = Enum.reverse(nearby_vnodes_rev)
    nearby_elements = Enum.reverse(nearby_elements_rev)

    vnode = %VNode{
      id: id,
      kind: element.type,
      key: key,
      attrs: element.attrs,
      children: child_vnodes,
      nearby: nearby_vnodes
    }

    assigned = %{
      element
      | id: id,
        children: child_elements,
        nearby: nearby_elements
    }

    {vnode, assigned, ctx}
  end

  defp build_old_key_index(%VNode{} = old_root) do
    build_old_key_index(old_root, :root, %{})
  end

  defp build_old_key_index(%VNode{key: key, id: id} = vnode, scope, acc) do
    acc = if is_nil(key), do: acc, else: Map.put(acc, key, %{scope: scope, vnode: vnode})

    acc =
      Enum.reduce(vnode.children, acc, fn child, next_acc ->
        build_old_key_index(child, {:children, id}, next_acc)
      end)

    Enum.reduce(vnode.nearby, acc, fn {_slot, nearby_vnode}, next_acc ->
      build_old_key_index(nearby_vnode, {:nearby, id}, next_acc)
    end)
  end

  defp reusable_root?(%VNode{kind: kind, key: key}, %Element{} = element) do
    kind == element.type and key == element_key(element)
  end

  defp children_mode(children) do
    sibling_mode(children, &has_key?/1, "All siblings must have key when any key is provided")
  end

  defp nearby_mode(nearby) do
    sibling_mode(
      nearby,
      fn {_slot, element} -> has_key?(element) end,
      "All nearby mounts on a host must have key when any key is provided"
    )
  end

  defp sibling_mode(items, has_key_fun, mixed_error) do
    mode =
      Enum.reduce(items, :unknown, fn item, mode ->
        case {mode, has_key_fun.(item)} do
          {:unknown, true} -> :keyed
          {:unknown, false} -> :unkeyed
          {:keyed, true} -> :keyed
          {:unkeyed, false} -> :unkeyed
          _ -> raise ArgumentError, mixed_error
        end
      end)

    case mode do
      :keyed -> :keyed
      _ -> :unkeyed
    end
  end

  defp prepend_removed_children(old_children, used_old_ids, patches_rev) do
    Enum.reduce(old_children, patches_rev, fn child, acc ->
      if MapSet.member?(used_old_ids, child.id) do
        acc
      else
        [{:remove, child.id} | acc]
      end
    end)
  end

  defp prepend_removed_nearby(old_nearby, used_old_ids, patches_rev) do
    Enum.reduce(old_nearby, patches_rev, fn {_slot, vnode}, acc ->
      if MapSet.member?(used_old_ids, vnode.id) do
        acc
      else
        [{:remove, vnode.id} | acc]
      end
    end)
  end

  defp maybe_set_attrs(patches, %VNode{attrs: old_attrs}, new_attrs, id) do
    old_filtered = TreeAttrs.strip_runtime_attrs(old_attrs)
    new_filtered = TreeAttrs.strip_runtime_attrs(new_attrs)

    if old_filtered != new_filtered do
      [{:set_attrs, id, new_filtered} | patches]
    else
      patches
    end
  end

  defp maybe_set_children(patches, %VNode{id: id, children: old_children}, new_children) do
    old_ids = Enum.map(old_children, & &1.id)
    new_ids = Enum.map(new_children, & &1.id)

    inserted_ids = new_ids -- old_ids
    removed_ids = old_ids -- new_ids

    old_remaining = old_ids -- removed_ids
    new_remaining = new_ids -- inserted_ids

    cond do
      old_ids == new_ids ->
        patches

      old_remaining != new_remaining ->
        [{:set_children, id, new_ids} | patches]

      true ->
        patches
    end
  end

  defp maybe_set_nearby_mounts(
         patches,
         %VNode{id: id, nearby: old_nearby},
         new_nearby
       ) do
    old_refs = mount_refs(old_nearby)
    new_refs = mount_refs(new_nearby)

    inserted_ids = mount_ids_from_refs(new_refs) -- mount_ids_from_refs(old_refs)
    removed_ids = mount_ids_from_refs(old_refs) -- mount_ids_from_refs(new_refs)

    old_remaining = Enum.reject(old_refs, fn {_slot, mount_id} -> mount_id in removed_ids end)
    new_remaining = Enum.reject(new_refs, fn {_slot, mount_id} -> mount_id in inserted_ids end)

    cond do
      old_refs == new_refs ->
        patches

      old_remaining != new_remaining ->
        [{:set_nearby_mounts, id, new_refs} | patches]

      true ->
        patches
    end
  end

  defp prepend_many(acc, patches) when is_list(patches) do
    Enum.reverse(patches, acc)
  end

  defp alloc_id(%{next_id: id} = ctx) do
    {id, %{ctx | next_id: id + 1}}
  end

  defp mount_refs(nearby) do
    Enum.map(nearby, fn {slot, vnode} -> {slot, vnode.id} end)
  end

  defp mount_ids_from_refs(refs) do
    Enum.map(refs, fn {_slot, id} -> id end)
  end

  defp element_key(%Element{key: key}) when not is_nil(key), do: key
  defp element_key(_), do: nil

  defp has_key?(%Element{key: key}) when not is_nil(key), do: true
  defp has_key?(_), do: false

  defp ensure_unique_key!(ctx, nil), do: ctx

  defp ensure_unique_key!(%{seen: seen} = ctx, key) do
    if MapSet.member?(seen, key) do
      raise ArgumentError, "duplicate explicit key: #{inspect(key)}"
    end

    %{ctx | seen: MapSet.put(seen, key)}
  end

  defp validate_viewport_root!(%Element{attrs: attrs}) do
    if Map.has_key?(attrs, :animate_exit) do
      raise ArgumentError, "animate_exit is not allowed on the viewport root"
    end
  end
end
