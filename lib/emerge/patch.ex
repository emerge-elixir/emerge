defmodule Emerge.Patch do
  @moduledoc """
  Diff and encode patch operations for Emerge.Element trees.
  """

  alias Emerge.Element
  alias Emerge.Tree.Attrs, as: TreeAttrs
  alias Emerge.Tree.Nearby

  @type nearby_slot :: :behind | :above | :on_right | :below | :on_left | :in_front

  @type patch ::
          {:set_attrs, term(), map()}
          | {:set_children, term(), [term()]}
          | {:insert_subtree, term() | nil, non_neg_integer(), Element.t()}
          | {:insert_nearby_subtree, term(), nearby_slot(), Element.t()}
          | {:remove, term()}

  @doc """
  Compute patches between two trees (expects numeric ids).
  """
  @spec diff(Element.t(), Element.t()) :: [patch()]
  def diff(old_tree, new_tree) do
    %{nodes: old_nodes} = index_tree(old_tree)
    %{nodes: new_nodes} = new_index = index_tree(new_tree)

    old_ids = MapSet.new(Map.keys(old_nodes))
    new_ids = MapSet.new(Map.keys(new_nodes))

    removed =
      old_ids
      |> MapSet.difference(new_ids)
      |> Enum.map(&{:remove, &1})

    added_ids = MapSet.difference(new_ids, old_ids)

    inserts =
      added_ids
      |> Enum.filter(&insert_root?(&1, new_index.mounts, added_ids))
      |> Enum.map(&insert_patch(&1, new_nodes, new_index.mounts))

    updates =
      new_ids
      |> MapSet.intersection(old_ids)
      |> Enum.flat_map(fn id ->
        old = Map.fetch!(old_nodes, id)
        new = Map.fetch!(new_nodes, id)

        attrs_patch =
          if comparable_attrs(old.attrs) != comparable_attrs(new.attrs) do
            [{:set_attrs, id, comparable_attrs(new.attrs)}]
          else
            []
          end

        children_patch =
          if child_ids(old.children) != child_ids(new.children) do
            [{:set_children, id, child_ids(new.children)}]
          else
            []
          end

        attrs_patch ++ children_patch
      end)

    removed ++ inserts ++ updates
  end

  @doc """
  Encode patches into a binary command stream.
  """
  @spec encode([patch()]) :: binary()
  def encode(patches) do
    patches
    |> Enum.map(&encode_patch/1)
    |> IO.iodata_to_binary()
  end

  @doc """
  Decode a binary command stream into patches.
  """
  @spec decode(binary()) :: [patch()]
  def decode(binary) when is_binary(binary) do
    binary
    |> decode_patches([])
    |> Enum.reverse()
  end

  defp encode_patch({:set_attrs, id, attrs}) do
    attr_bin = Emerge.AttrCodec.encode_attrs(attrs)
    id_bin = :erlang.term_to_binary(id)

    <<1, byte_size(id_bin)::unsigned-32, id_bin::binary, byte_size(attr_bin)::unsigned-32,
      attr_bin::binary>>
  end

  defp encode_patch({:set_children, id, children}) do
    id_bin = :erlang.term_to_binary(id)

    children_bin =
      children
      |> Enum.map(fn child_id ->
        bin = :erlang.term_to_binary(child_id)
        [<<byte_size(bin)::unsigned-32>>, bin]
      end)
      |> IO.iodata_to_binary()

    <<2, byte_size(id_bin)::unsigned-32, id_bin::binary, length(children)::unsigned-16,
      children_bin::binary>>
  end

  defp encode_patch({:insert_subtree, parent_id, index, subtree}) do
    subtree_bin = Emerge.Serialization.encode_tree(subtree)
    parent_bin = :erlang.term_to_binary(parent_id)

    <<3, byte_size(parent_bin)::unsigned-32, parent_bin::binary, index::unsigned-16,
      byte_size(subtree_bin)::unsigned-32, subtree_bin::binary>>
  end

  defp encode_patch({:remove, id}) do
    id_bin = :erlang.term_to_binary(id)
    <<4, byte_size(id_bin)::unsigned-32, id_bin::binary>>
  end

  defp encode_patch({:insert_nearby_subtree, host_id, slot, subtree}) do
    subtree_bin = Emerge.Serialization.encode_tree(subtree)
    host_bin = :erlang.term_to_binary(host_id)

    <<5, byte_size(host_bin)::unsigned-32, host_bin::binary, nearby_slot_tag(slot)::unsigned-8,
      byte_size(subtree_bin)::unsigned-32, subtree_bin::binary>>
  end

  defp decode_patches(<<>>, acc), do: acc

  defp decode_patches(<<1, id_len::unsigned-32, rest::binary>>, acc) do
    <<id_bin::binary-size(id_len), rest::binary>> = rest
    id = :erlang.binary_to_term(id_bin)
    <<attr_len::unsigned-32, rest::binary>> = rest
    <<attr_bin::binary-size(attr_len), rest::binary>> = rest
    attrs = Emerge.AttrCodec.decode_attrs(attr_bin)
    decode_patches(rest, [{:set_attrs, id, attrs} | acc])
  end

  defp decode_patches(<<2, id_len::unsigned-32, rest::binary>>, acc) do
    <<id_bin::binary-size(id_len), rest::binary>> = rest
    id = :erlang.binary_to_term(id_bin)
    <<count::unsigned-16, rest::binary>> = rest
    {children, rest} = decode_child_ids(rest, count, [])
    decode_patches(rest, [{:set_children, id, children} | acc])
  end

  defp decode_patches(<<3, parent_len::unsigned-32, rest::binary>>, acc) do
    <<parent_bin::binary-size(parent_len), rest::binary>> = rest
    parent_id = :erlang.binary_to_term(parent_bin)
    <<index::unsigned-16, len::unsigned-32, rest::binary>> = rest
    <<subtree_bin::binary-size(len), rest::binary>> = rest
    subtree = Emerge.Serialization.decode(subtree_bin)
    decode_patches(rest, [{:insert_subtree, parent_id, index, subtree} | acc])
  end

  defp decode_patches(<<4, id_len::unsigned-32, rest::binary>>, acc) do
    <<id_bin::binary-size(id_len), rest::binary>> = rest
    id = :erlang.binary_to_term(id_bin)
    decode_patches(rest, [{:remove, id} | acc])
  end

  defp decode_patches(<<5, host_len::unsigned-32, rest::binary>>, acc) do
    <<host_bin::binary-size(host_len), rest::binary>> = rest
    host_id = :erlang.binary_to_term(host_bin)
    <<slot_tag::unsigned-8, len::unsigned-32, rest::binary>> = rest
    <<subtree_bin::binary-size(len), rest::binary>> = rest
    subtree = Emerge.Serialization.decode(subtree_bin)
    slot = nearby_slot_from_tag!(slot_tag)
    decode_patches(rest, [{:insert_nearby_subtree, host_id, slot, subtree} | acc])
  end

  defp decode_patches(_other, _acc) do
    raise ArgumentError, "invalid patch stream"
  end

  defp decode_child_ids(rest, 0, acc), do: {Enum.reverse(acc), rest}

  defp decode_child_ids(<<len::unsigned-32, rest::binary>>, count, acc) do
    <<id_bin::binary-size(len), rest::binary>> = rest
    id = :erlang.binary_to_term(id_bin)
    decode_child_ids(rest, count - 1, [id | acc])
  end

  defp comparable_attrs(attrs) do
    attrs
    |> Nearby.strip_nearby_attrs()
    |> TreeAttrs.strip_runtime_attrs()
  end

  defp index_tree(%Element{} = tree) do
    collect_index(tree, :root, %{nodes: %{}, mounts: %{}})
  end

  defp collect_index(%Element{} = element, mount, acc) do
    acc = %{
      nodes: Map.put(acc.nodes, element.id, element),
      mounts: Map.put(acc.mounts, element.id, mount)
    }

    acc =
      element.children
      |> Enum.with_index()
      |> Enum.reduce(acc, fn {child, index}, next_acc ->
        collect_index(child, {:child, element.id, index}, next_acc)
      end)

    Enum.reduce(Nearby.nearby_children(element), acc, fn {slot, child}, next_acc ->
      collect_index(child, {:nearby, element.id, slot}, next_acc)
    end)
  end

  defp insert_root?(id, mounts, added_ids) do
    case Map.fetch!(mounts, id) do
      :root -> true
      {:child, parent_id, _index} -> not MapSet.member?(added_ids, parent_id)
      {:nearby, host_id, _slot} -> not MapSet.member?(added_ids, host_id)
    end
  end

  defp insert_patch(id, nodes, mounts) do
    node = Map.fetch!(nodes, id)

    case Map.fetch!(mounts, id) do
      :root -> {:insert_subtree, nil, 0, node}
      {:child, parent_id, index} -> {:insert_subtree, parent_id, index, node}
      {:nearby, host_id, slot} -> {:insert_nearby_subtree, host_id, slot, node}
    end
  end

  defp child_ids(children) do
    Enum.map(children, & &1.id)
  end

  defp nearby_slot_tag(:behind), do: 1
  defp nearby_slot_tag(:above), do: 2
  defp nearby_slot_tag(:on_right), do: 3
  defp nearby_slot_tag(:below), do: 4
  defp nearby_slot_tag(:on_left), do: 5
  defp nearby_slot_tag(:in_front), do: 6

  defp nearby_slot_from_tag!(1), do: :behind
  defp nearby_slot_from_tag!(2), do: :above
  defp nearby_slot_from_tag!(3), do: :on_right
  defp nearby_slot_from_tag!(4), do: :below
  defp nearby_slot_from_tag!(5), do: :on_left
  defp nearby_slot_from_tag!(6), do: :in_front

  defp nearby_slot_from_tag!(tag) do
    raise ArgumentError, "invalid nearby slot tag: #{inspect(tag)}"
  end
end
