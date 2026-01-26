defmodule Emerge.Reconcile do
  @moduledoc """
  Reconcile Emerge.Element trees into stable ids and patch operations.
  """

  alias Emerge.Element
  alias Emerge.Patch
  alias Emerge.VNode

  @type result :: {VNode.t(), [Patch.patch()], Element.t()}

  @doc """
  Assign ids to a tree without a previous version.
  """
  @spec assign_ids(Element.t()) :: {VNode.t(), Element.t()}
  def assign_ids(%Element{} = element) do
    {vnode, assigned, _seen} = build_vnode(element, :root, 0, MapSet.new())
    {vnode, assigned}
  end

  @doc """
  Reconcile a new tree against the previous vdom.
  """
  @spec reconcile(VNode.t() | nil, Element.t()) :: result()
  def reconcile(nil, %Element{} = element) do
    {vnode, assigned, _seen} = build_vnode(element, :root, 0, MapSet.new())
    {vnode, [], assigned}
  end

  def reconcile(%VNode{} = old_vnode, %Element{} = element) do
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

      attrs = normalize_nearby_attrs(element.attrs)
      assigned = %{element | id: id, children: child_elements, attrs: attrs}

      patches =
        []
        |> maybe_set_attrs(old, assigned)
        |> maybe_set_children(old, child_vnodes)
        |> Kernel.++(child_patches)

      vnode = %VNode{
        id: id,
        kind: element.type,
        key: key,
        attrs: assigned.attrs,
        children: child_vnodes
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

    attrs = normalize_nearby_attrs(element.attrs)
    assigned = %{element | id: id, children: child_elements, attrs: attrs}

    vnode = %VNode{
      id: id,
      kind: element.type,
      key: key,
      attrs: assigned.attrs,
      children: child_vnodes
    }

    {vnode, assigned, seen}
  end

  defp maybe_set_attrs(patches, %VNode{attrs: old_attrs}, %Element{attrs: new_attrs, id: id}) do
    old_filtered = Emerge.Tree.strip_runtime_attrs(old_attrs)
    new_filtered = Emerge.Tree.strip_runtime_attrs(new_attrs)

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

  defp normalize_nearby_attrs(attrs) when is_map(attrs) do
    attrs
    |> normalize_nearby_attr(:above)
    |> normalize_nearby_attr(:below)
    |> normalize_nearby_attr(:on_left)
    |> normalize_nearby_attr(:on_right)
    |> normalize_nearby_attr(:in_front)
    |> normalize_nearby_attr(:behind)
  end

  defp normalize_nearby_attr(attrs, key) do
    case Map.get(attrs, key) do
      %Element{} = element ->
        {_vdom, assigned} = assign_ids(element)
        Map.put(attrs, key, assigned)

      _ ->
        attrs
    end
  end
end
