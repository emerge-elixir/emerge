defmodule Emerge.Engine.Patch do
  @moduledoc """
  Diff and encode patch operations for Emerge.Engine.Element trees.
  """
  alias Emerge.Engine.{AttrCodec, Element, Reconcile}
  alias Emerge.Engine.Tree.Nearby

  @kinds ~w(row wrapped_row column el text none paragraph text_column image text_input video multiline slider wrapped_column)a
  @tags @kinds |> Enum.with_index(1) |> Map.new()
  @types Map.new(@tags, fn {kind, tag} -> {tag, kind} end)
  @type body ::
          nil
          | pos_integer()
          | {:node, pos_integer(), atom(), :same | map() | binary(), :same | [{atom(), body()}]}
  @type patch :: {:patch, non_neg_integer(), body()}
  @spec patch(non_neg_integer(), body()) :: patch()
  def patch(target, subtree), do: {:patch, target, subtree}
  def diff(%Element{} = old, %Element{} = new), do: Reconcile.diff_assigned(old, new)

  def full(%Element{} = n) do
    {:node, n.id, n.type, n.attrs, edges(n, &full/1)}
  end

  @doc false
  def edges(node, fun) do
    mounts = Enum.map(node.nearby, fn {tag, c} -> {tag, fun.(c)} end)
    List.foldr(node.children, mounts, fn c, tail -> [{:child, fun.(c)} | tail] end)
  end

  @spec encode(patch() | [patch()]) :: binary()
  def encode(patch, known \\ %{})
  def encode([], _known), do: <<>>
  def encode([op], known), do: encode(op, known)

  def encode({:patch, target, body}, known) when target in 0..0xFFFFFFFFFFFFFFFF do
    {records, count, _known} = records(body, {[], 0, known})
    wire = [<<"EMRG", 10, target::64, id(body)::64, count::32>>, Enum.reverse(records)]
    if IO.iodata_length(wire) > 67_108_864, do: raise(ArgumentError, "EMRG exceeds 64 MiB")
    IO.iodata_to_binary(wire)
  end

  defp id(nil), do: 0
  defp id({:node, id, _, _, _}), do: id(id)
  defp id(id) when id in 1..0xFFFFFFFFFFFFFFFF, do: id

  defp records({:node, id, kind, attrs, children}, {acc, n, known}) do
    {at, raw, known} = field(attrs, known)

    {ch, order} =
      case children do
        :same -> {0, []}
        edges -> {2, encode_edges(edges)}
      end

    record = [<<id(id)::64, Map.fetch!(@tags, kind), at + ch>>, raw, order]
    acc = {[record | acc], n + 1, known}
    if ch == 0, do: acc, else: Enum.reduce(children, acc, fn {_, c}, acc -> records(c, acc) end)
  end

  defp records(_ref, acc), do: acc

  defp field(:same, known), do: {0, [], known}
  defp field(attrs, known) when is_map(attrs), do: field(AttrCodec.local(attrs, nil), known)
  defp field(raw, known) when is_binary(raw), do: {1, [<<byte_size(raw)::32>>, raw], known}

  defp field({:media, prefix, digest, bytes}, known) do
    if Map.has_key?(known, digest) do
      field(AttrCodec.reference(prefix, digest), known)
    else
      len = byte_size(prefix) + 37 + byte_size(bytes)

      {1, [<<len::32>>, prefix, <<3, digest::binary, byte_size(bytes)::32>>, bytes],
       Map.put(known, digest, true)}
    end
  end

  defp encode_edges(edges) do
    {ordinary, nearby} = Enum.split_while(edges, fn {tag, _} -> tag == :child end)

    [
      count(ordinary),
      Enum.map(ordinary, fn {_, c} -> <<id(c)::64>> end),
      count(nearby),
      Enum.map(nearby, fn {slot, c} -> <<Nearby.slot_tag(slot), id(c)::64>> end)
    ]
  end

  defp count(list) do
    case length(list) do
      n when n <= 65_535 -> <<n::16>>
      _ -> raise ArgumentError, "too many child edges"
    end
  end

  @spec decode(binary()) :: [patch()]
  def decode(<<>>), do: []

  def decode(<<"EMRG", 10, target::64, root::64, count::32, rest::binary>>) do
    {defs, media, <<>>} = read_records(rest, count, %{}, %{})

    defs =
      if map_size(media) > 0 or target == 0 do
        Map.new(defs, fn
          {id, {kind, %{image_src: {:digest, key}} = attrs, edges}} ->
            src =
              Map.get(media, key) ||
                if(target == 0,
                  do: raise(ArgumentError, "undefined image source"),
                  else: {:digest, key}
                )

            {id, {kind, %{attrs | image_src: src}, edges}}

          entry ->
            entry
        end)
      else
        defs
      end

    {body, {remaining, _}} = expand(root, {defs, MapSet.new()})
    if map_size(remaining) != 0, do: raise(ArgumentError, "unreachable patch record")
    [patch(target, body)]
  end

  def decode(_), do: raise(ArgumentError, "expected EMRG version 10")

  defp read_records(bytes, 0, defs, media), do: {defs, media, bytes}

  defp read_records(<<id::64, tag, flags, bytes::binary>>, n, defs, media)
       when flags in 0..3 and id > 0 do
    if Map.has_key?(defs, id), do: raise(ArgumentError, "duplicate patch record")

    {attrs, bytes} =
      if flags in [1, 3] do
        <<len::32, raw::binary-size(len), tail::binary>> = bytes
        {AttrCodec.decode_attrs(raw), tail}
      else
        {:same, bytes}
      end

    {edges, bytes} = if flags >= 2, do: read_edges(bytes), else: {:same, bytes}

    media =
      case attrs do
        %{image_src: {:bytes, data} = source} ->
          Map.put(media, :crypto.hash(:sha256, data), source)

        _ ->
          media
      end

    read_records(bytes, n - 1, Map.put(defs, id, {Map.fetch!(@types, tag), attrs, edges}), media)
  end

  defp read_edges(
         <<n::16, ordinary::binary-size(n * 8), m::16, nearby::binary-size(m * 9), rest::binary>>
       ) do
    mounts = for <<tag, id::64 <- nearby>>, do: {Nearby.slot_from_tag!(tag), id}

    {List.foldr(for(<<id::64 <- ordinary>>, do: id), mounts, fn id, acc ->
       [{:child, id} | acc]
     end), rest}
  end

  defp expand(0, state), do: {nil, state}

  defp expand(id, {defs, seen}) do
    if MapSet.member?(seen, id), do: raise(ArgumentError, "cycle or duplicate child")
    seen = MapSet.put(seen, id)

    case Map.pop(defs, id) do
      {nil, _} ->
        {id, {defs, seen}}

      {{kind, attrs, :same}, defs} ->
        {{:node, id, kind, attrs, :same}, {defs, seen}}

      {{kind, attrs, edges}, defs} ->
        {children, state} =
          Enum.map_reduce(edges, {defs, seen}, fn {tag, id}, state ->
            if id == 0, do: raise(ArgumentError, "zero child ID")
            {c, state} = expand(id, state)
            {{tag, c}, state}
          end)

        {{:node, id, kind, attrs, children}, state}
    end
  end
end
