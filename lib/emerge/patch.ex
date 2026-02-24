defmodule Emerge.Patch do
  @moduledoc """
  Diff and encode patch operations for Emerge.Element trees.
  """

  alias Emerge.Element

  @type patch ::
          {:set_attrs, term(), map()}
          | {:set_children, term(), [term()]}
          | {:insert_subtree, term() | nil, non_neg_integer(), Element.t()}
          | {:remove, term()}

  @doc """
  Compute patches between two trees (expects numeric ids).
  """
  @spec diff(Element.t(), Element.t()) :: [patch()]
  def diff(old_tree, new_tree) do
    old_nodes = index_nodes(old_tree)
    new_nodes = index_nodes(new_tree)
    parent_map = parent_index(new_tree)

    old_ids = MapSet.new(Map.keys(old_nodes))
    new_ids = MapSet.new(Map.keys(new_nodes))

    removed =
      old_ids
      |> MapSet.difference(new_ids)
      |> Enum.map(&{:remove, &1})

    added_ids = MapSet.difference(new_ids, old_ids)

    insert_roots =
      added_ids
      |> Enum.filter(fn id ->
        parent_id = Map.get(parent_map, id)
        is_nil(parent_id) or not MapSet.member?(added_ids, parent_id)
      end)

    inserts =
      insert_roots
      |> Enum.map(fn id ->
        parent_id = Map.get(parent_map, id)
        index = child_index(parent_map, new_tree, id)
        {:insert_subtree, parent_id, index, Map.fetch!(new_nodes, id)}
      end)

    updates =
      new_ids
      |> MapSet.intersection(old_ids)
      |> Enum.flat_map(fn id ->
        old = Map.fetch!(old_nodes, id)
        new = Map.fetch!(new_nodes, id)

        attrs_patch =
          if Emerge.Tree.strip_runtime_attrs(old.attrs) !=
               Emerge.Tree.strip_runtime_attrs(new.attrs) do
            [{:set_attrs, id, Emerge.Tree.strip_runtime_attrs(new.attrs)}]
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

  defp decode_patches(
         <<3, parent_len::unsigned-32, rest::binary>>,
         acc
       ) do
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

  defp decode_patches(_other, _acc) do
    raise ArgumentError, "invalid patch stream"
  end

  defp decode_child_ids(rest, 0, acc), do: {Enum.reverse(acc), rest}

  defp decode_child_ids(<<len::unsigned-32, rest::binary>>, count, acc) do
    <<id_bin::binary-size(len), rest::binary>> = rest
    id = :erlang.binary_to_term(id_bin)
    decode_child_ids(rest, count - 1, [id | acc])
  end

  defp index_nodes(%Element{} = tree) do
    tree
    |> collect_nodes()
    |> Map.new(fn node -> {node.id, node} end)
  end

  defp collect_nodes(%Element{} = element) do
    [element | Enum.flat_map(element.children, &collect_nodes/1)]
  end

  defp parent_index(%Element{} = tree) do
    parent_index(tree, nil, %{})
  end

  defp parent_index(%Element{} = element, parent_id, acc) do
    acc = Map.put(acc, element.id, parent_id)

    Enum.reduce(element.children, acc, fn child, acc_child ->
      parent_index(child, element.id, acc_child)
    end)
  end

  defp child_ids(children) do
    Enum.map(children, & &1.id)
  end

  defp child_index(parent_map, %Element{} = tree, id) do
    parent_id = Map.get(parent_map, id)

    cond do
      is_nil(parent_id) ->
        0

      true ->
        parent = find_node(tree, parent_id)
        parent.children |> child_ids() |> Enum.find_index(&(&1 == id)) || 0
    end
  end

  defp find_node(%Element{id: id} = element, id), do: element

  defp find_node(%Element{children: children}, id) do
    Enum.find_value(children, fn child -> find_node(child, id) end)
  end
end
