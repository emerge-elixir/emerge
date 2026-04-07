defmodule Emerge.Engine.Reconcile do
  @moduledoc """
  Reconcile Emerge.Engine.Element trees into stable ids and patch operations.
  """

  alias Emerge.Engine.Element
  alias Emerge.Engine.Patch
  alias Emerge.Engine.Tree.Attrs, as: TreeAttrs
  alias Emerge.Engine.Tree.Nearby
  alias Emerge.Engine.VNode

  @type result :: {VNode.t(), [Patch.patch()], Element.t()}

  @doc """
  Assign ids to a tree without a previous version.
  """
  @spec assign_ids(Element.t()) :: {VNode.t(), Element.t()}
  def assign_ids(%Element{} = element) do
    validate_viewport_root!(element)
    {vnode, assigned, _seen} = build_vnode(element, :root, 0, MapSet.new())
    {vnode, assigned}
  end

  @doc """
  Reconcile a new tree against the previous vdom.
  """
  @spec reconcile(VNode.t() | nil, Element.t()) :: result()
  def reconcile(nil, %Element{} = element) do
    validate_viewport_root!(element)
    {vnode, assigned, _seen} = build_vnode(element, :root, 0, MapSet.new())
    {vnode, [], assigned}
  end

  def reconcile(%VNode{} = old_vnode, %Element{} = element) do
    validate_viewport_root!(element)

    {vnode, patches, assigned, _seen} =
      reconcile_node(old_vnode, element, :root, 0, MapSet.new())

    {vnode, patches, assigned}
  end

  defp reconcile_node(%VNode{} = old, %Element{} = element, parent_id, index, seen) do
    key = element_key(element)
    seen = ensure_unique_key!(seen, key)
    local_identity = local_identity(key, index)
    id = make_id(parent_id, element.type, local_identity)

    if old.kind != element.type or old.id != id do
      {new_vnode, assigned, seen} = build_vnode(element, parent_id, index, seen)
      patches = [{:remove, old.id}, {:insert_subtree, parent_id, index, assigned}]
      {new_vnode, patches, assigned, seen}
    else
      {child_vnodes, child_elements, child_patches, seen} =
        reconcile_children(old.children, element.children, id, seen)

      {nearby_vnodes, nearby_assigned, nearby_patches, seen} =
        reconcile_nearby(old.nearby, element.nearby, id, seen)

      assigned = %{
        element
        | id: id,
          children: child_elements,
          attrs: element.attrs,
          nearby: nearby_assigned
      }

      patches =
        []
        |> maybe_set_attrs(old, element.attrs, id)
        |> maybe_set_children(old, child_vnodes)
        |> maybe_set_nearby_mounts(old, nearby_vnodes)
        |> Kernel.++(child_patches)
        |> Kernel.++(nearby_patches)

      vnode = %VNode{
        id: id,
        kind: element.type,
        key: key,
        attrs: element.attrs,
        children: child_vnodes,
        nearby: nearby_vnodes
      }

      {vnode, patches, assigned, seen}
    end
  end

  defp reconcile_children(old_children, new_children, parent_id, seen) do
    if keyed_children?(new_children) do
      reconcile_children_keyed(old_children, new_children, parent_id, seen)
    else
      reconcile_children_indexed(old_children, new_children, parent_id, seen)
    end
  end

  defp reconcile_children_keyed(old_children, new_children, parent_id, seen) do
    old_by_key =
      old_children
      |> Enum.filter(& &1.key)
      |> Map.new(fn child -> {child.key, child} end)

    {child_vnodes, child_elements, patches, used_old_ids, seen} =
      Enum.with_index(new_children)
      |> Enum.reduce({[], [], [], MapSet.new(), seen}, fn {child, index},
                                                          {vnodes, elements, patches,
                                                           used_old_ids, seen} ->
        key = element_key(child)

        case match_keyed_child(old_by_key, old_children, key, index, child.type) do
          {:ok, old_child} when old_child.kind == child.type ->
            {vnode, child_patches, assigned, seen} =
              reconcile_node(old_child, child, parent_id, index, seen)

            {
              [vnode | vnodes],
              [assigned | elements],
              patches ++ child_patches,
              MapSet.put(used_old_ids, old_child.id),
              seen
            }

          _ ->
            {vnode, assigned, seen} = build_vnode(child, parent_id, index, seen)
            insert = {:insert_subtree, parent_id, index, assigned}

            {
              [vnode | vnodes],
              [assigned | elements],
              patches ++ [insert],
              used_old_ids,
              seen
            }
        end
      end)

    removed =
      old_children
      |> Enum.reject(fn child -> MapSet.member?(used_old_ids, child.id) end)
      |> Enum.map(&{:remove, &1.id})

    {Enum.reverse(child_vnodes), Enum.reverse(child_elements), removed ++ patches, seen}
  end

  defp match_keyed_child(_old_by_key, old_children, nil, index, kind) do
    case Enum.at(old_children, index) do
      %VNode{kind: ^kind, key: nil} = child -> {:ok, child}
      _ -> :error
    end
  end

  defp match_keyed_child(old_by_key, _old_children, key, _index, _kind) do
    Map.fetch(old_by_key, key)
  end

  defp keyed_children?(children) do
    key_count = Enum.count(children, &has_key?/1)
    total_count = length(children)

    cond do
      key_count == 0 ->
        false

      key_count == total_count ->
        true

      true ->
        raise ArgumentError,
              "All siblings must have key when any key is provided"
    end
  end

  defp reconcile_children_indexed(old_children, new_children, parent_id, seen) do
    {child_vnodes, child_elements, patches, seen} =
      new_children
      |> Enum.with_index()
      |> Enum.reduce({[], [], [], seen}, fn {child, index}, {vnodes, elements, patches, seen} ->
        case Enum.at(old_children, index) do
          %VNode{kind: kind} = old_child when kind == child.type ->
            {vnode, child_patches, assigned, seen} =
              reconcile_node(old_child, child, parent_id, index, seen)

            {[vnode | vnodes], [assigned | elements], patches ++ child_patches, seen}

          %VNode{} = old_child ->
            {vnode, assigned, seen} = build_vnode(child, parent_id, index, seen)
            insert = {:insert_subtree, parent_id, index, assigned}

            {
              [vnode | vnodes],
              [assigned | elements],
              patches ++ [{:remove, old_child.id}, insert],
              seen
            }

          nil ->
            {vnode, assigned, seen} = build_vnode(child, parent_id, index, seen)
            insert = {:insert_subtree, parent_id, index, assigned}
            {[vnode | vnodes], [assigned | elements], patches ++ [insert], seen}
        end
      end)

    removed =
      old_children
      |> Enum.drop(length(new_children))
      |> Enum.map(&{:remove, &1.id})

    {Enum.reverse(child_vnodes), Enum.reverse(child_elements), removed ++ patches, seen}
  end

  defp reconcile_nearby(old_nearby, new_nearby, host_id, seen) do
    if keyed_nearby?(new_nearby) do
      reconcile_nearby_keyed(old_nearby, new_nearby, host_id, seen)
    else
      reconcile_nearby_indexed(old_nearby, new_nearby, host_id, seen)
    end
  end

  defp reconcile_nearby_keyed(old_nearby, new_nearby, host_id, seen) do
    old_by_key =
      old_nearby
      |> Enum.filter(fn {_slot, vnode} -> vnode.key end)
      |> Map.new(fn {_slot, vnode} -> {vnode.key, vnode} end)

    {nearby_vnodes, nearby_elements, patches, used_old_ids, seen} =
      Enum.with_index(new_nearby)
      |> Enum.reduce({[], [], [], MapSet.new(), seen}, fn {{slot, element}, index},
                                                          {vnodes, elements, patches,
                                                           used_old_ids, seen} ->
        key = element_key(element)

        case match_keyed_nearby(old_by_key, old_nearby, key, index, element.type) do
          {:ok, %VNode{} = old_vnode} when old_vnode.kind == element.type ->
            {vnode, mount_patches, assigned, seen} =
              reconcile_nearby_node(old_vnode, element, host_id, index, seen)

            {
              [{slot, vnode} | vnodes],
              [{slot, assigned} | elements],
              patches ++ mount_patches,
              MapSet.put(used_old_ids, old_vnode.id),
              seen
            }

          _ ->
            {vnode, assigned, seen} = build_nearby_vnode(element, host_id, index, seen)
            insert = {:insert_nearby_subtree, host_id, index, slot, assigned}

            {
              [{slot, vnode} | vnodes],
              [{slot, assigned} | elements],
              patches ++ [insert],
              used_old_ids,
              seen
            }
        end
      end)

    removed =
      old_nearby
      |> Enum.map(fn {_slot, vnode} -> vnode end)
      |> Enum.reject(fn vnode -> MapSet.member?(used_old_ids, vnode.id) end)
      |> Enum.map(&{:remove, &1.id})

    {Enum.reverse(nearby_vnodes), Enum.reverse(nearby_elements), removed ++ patches, seen}
  end

  defp match_keyed_nearby(_old_by_key, old_nearby, nil, index, kind) do
    case Enum.at(old_nearby, index) do
      {_slot, %VNode{kind: ^kind, key: nil} = vnode} -> {:ok, vnode}
      _ -> :error
    end
  end

  defp match_keyed_nearby(old_by_key, _old_nearby, key, _index, _kind) do
    Map.fetch(old_by_key, key)
  end

  defp keyed_nearby?(nearby) do
    key_count = Enum.count(nearby, fn {_slot, element} -> has_key?(element) end)
    total_count = length(nearby)

    cond do
      key_count == 0 ->
        false

      key_count == total_count ->
        true

      true ->
        raise ArgumentError, "All nearby mounts on a host must have key when any key is provided"
    end
  end

  defp reconcile_nearby_indexed(old_nearby, new_nearby, host_id, seen) do
    {nearby_vnodes, nearby_elements, patches, seen} =
      new_nearby
      |> Enum.with_index()
      |> Enum.reduce({[], [], [], seen}, fn {{slot, element}, index},
                                            {vnodes, elements, patches, seen} ->
        case Enum.at(old_nearby, index) do
          {_old_slot, %VNode{kind: kind} = old_vnode} when kind == element.type ->
            {vnode, mount_patches, assigned, seen} =
              reconcile_nearby_node(old_vnode, element, host_id, index, seen)

            {
              [{slot, vnode} | vnodes],
              [{slot, assigned} | elements],
              patches ++ mount_patches,
              seen
            }

          {_old_slot, %VNode{} = old_vnode} ->
            {vnode, assigned, seen} = build_nearby_vnode(element, host_id, index, seen)
            insert = {:insert_nearby_subtree, host_id, index, slot, assigned}

            {
              [{slot, vnode} | vnodes],
              [{slot, assigned} | elements],
              patches ++ [{:remove, old_vnode.id}, insert],
              seen
            }

          nil ->
            {vnode, assigned, seen} = build_nearby_vnode(element, host_id, index, seen)
            insert = {:insert_nearby_subtree, host_id, index, slot, assigned}

            {
              [{slot, vnode} | vnodes],
              [{slot, assigned} | elements],
              patches ++ [insert],
              seen
            }
        end
      end)

    removed =
      old_nearby
      |> Enum.drop(length(new_nearby))
      |> Enum.map(fn {_slot, vnode} -> {:remove, vnode.id} end)

    {Enum.reverse(nearby_vnodes), Enum.reverse(nearby_elements), removed ++ patches, seen}
  end

  defp reconcile_nearby_node(%VNode{} = old, %Element{} = element, host_id, index, seen) do
    parent_id = {:nearby, host_id}
    key = element_key(element)
    seen = ensure_unique_key!(seen, key)
    local_identity = local_identity(key, index)
    id = make_id(parent_id, element.type, local_identity)

    if old.kind != element.type or old.id != id do
      {vnode, assigned, seen} = build_vnode(element, parent_id, index, seen)
      patches = [{:remove, old.id}]
      {vnode, patches, assigned, seen}
    else
      {child_vnodes, child_elements, child_patches, seen} =
        reconcile_children(old.children, element.children, id, seen)

      {nearby_vnodes, nearby_assigned, nearby_patches, seen} =
        reconcile_nearby(old.nearby, element.nearby, id, seen)

      assigned = %{
        element
        | id: id,
          children: child_elements,
          attrs: element.attrs,
          nearby: nearby_assigned
      }

      patches =
        []
        |> maybe_set_attrs(old, element.attrs, id)
        |> maybe_set_children(old, child_vnodes)
        |> maybe_set_nearby_mounts(old, nearby_vnodes)
        |> Kernel.++(child_patches)
        |> Kernel.++(nearby_patches)

      vnode = %VNode{
        id: id,
        kind: element.type,
        key: key,
        attrs: element.attrs,
        children: child_vnodes,
        nearby: nearby_vnodes
      }

      {vnode, patches, assigned, seen}
    end
  end

  defp build_vnode(%Element{} = element, parent_id, index, seen) do
    key = element_key(element)
    seen = ensure_unique_key!(seen, key)
    local_identity = local_identity(key, index)
    id = make_id(parent_id, element.type, local_identity)

    _ = keyed_children?(element.children)

    {child_vnodes, child_elements, seen} =
      element.children
      |> Enum.with_index()
      |> Enum.reduce({[], [], seen}, fn {child, idx}, {vnodes, elements, seen} ->
        {child_vnode, child_element, seen} = build_vnode(child, id, idx, seen)
        {[child_vnode | vnodes], [child_element | elements], seen}
      end)

    child_vnodes = Enum.reverse(child_vnodes)
    child_elements = Enum.reverse(child_elements)

    {nearby_vnodes, nearby_assigned, seen} = build_nearby_vnodes(element.nearby, id, seen)

    assigned = %{
      element
      | id: id,
        children: child_elements,
        attrs: element.attrs,
        nearby: nearby_assigned
    }

    vnode = %VNode{
      id: id,
      kind: element.type,
      key: key,
      attrs: element.attrs,
      children: child_vnodes,
      nearby: nearby_vnodes
    }

    {vnode, assigned, seen}
  end

  defp build_nearby_vnodes(nearby_elements, host_id, seen) do
    Enum.with_index(nearby_elements)
    |> Enum.reduce({[], [], seen}, fn {{slot, element}, index}, {vnodes, elements, seen} ->
      {vnode, assigned, seen} = build_nearby_vnode(element, host_id, index, seen)

      {
        [{slot, vnode} | vnodes],
        [{slot, assigned} | elements],
        seen
      }
    end)
    |> then(fn {vnodes, elements, seen} ->
      {Enum.reverse(vnodes), Enum.reverse(elements), seen}
    end)
  end

  defp build_nearby_vnode(%Element{} = element, host_id, index, seen) do
    build_vnode(element, {:nearby, host_id}, index, seen)
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

  defp maybe_set_nearby_mounts(patches, %VNode{id: id, nearby: old_nearby}, new_nearby) do
    old_refs = mount_refs(old_nearby)
    new_refs = mount_refs(new_nearby)

    inserted_ids = Nearby.mount_ids_from_refs(new_refs) -- Nearby.mount_ids_from_refs(old_refs)
    removed_ids = Nearby.mount_ids_from_refs(old_refs) -- Nearby.mount_ids_from_refs(new_refs)

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

  defp mount_refs(nearby) do
    Enum.map(nearby, fn {slot, vnode} -> {slot, vnode.id} end)
  end

  defp element_key(%Element{id: id}) when not is_nil(id), do: id
  defp element_key(_), do: nil

  defp has_key?(%Element{id: id}) when not is_nil(id), do: true
  defp has_key?(_), do: false

  defp local_identity(nil, index), do: {:i, index}
  defp local_identity(key, _index), do: {:k, key}

  defp ensure_unique_key!(seen, nil), do: seen

  defp ensure_unique_key!(seen, key) do
    if MapSet.member?(seen, key) do
      raise ArgumentError, "duplicate explicit id/key: #{inspect(key)}"
    end

    MapSet.put(seen, key)
  end

  defp make_id(parent_id, kind, local_identity) do
    :erlang.phash2({parent_id, kind, local_identity})
  end

  defp validate_viewport_root!(%Element{attrs: attrs}) do
    if Map.has_key?(attrs, :animate_exit) do
      raise ArgumentError, "animate_exit is not allowed on the viewport root"
    end
  end
end
