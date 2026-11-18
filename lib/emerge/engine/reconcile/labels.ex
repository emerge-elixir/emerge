defmodule Emerge.Engine.Reconcile.Labels do
  @moduledoc false
  # A HAMT hashes to a bucket but compares actual keys. Descriptors contain
  # only local data and immediate tokens. Zero is reserved for the root site.
  defstruct values: %{}, by_token: %{}, next: 1, retired: []

  def bind(pool, old, descriptor) do
    case pool.values do
      %{^descriptor => {^old, _}} ->
        {old, pool}

      %{^descriptor => {id, refs}} ->
        {id, release(%{pool | values: Map.put(pool.values, descriptor, {id, refs + 1})}, old)}

      _ ->
        id = pool.next
        if id > 0xFFFFFFFF, do: raise(ArgumentError, "declaration label exhausted")

        {id,
         release(
           %{
             pool
             | next: id + 1,
               values: Map.put(pool.values, descriptor, {id, 1}),
               by_token: Map.put(pool.by_token, id, descriptor)
           },
           old
         )}
    end
  end

  def release(pool, token) when token in [nil, 0], do: pool

  def release(pool, token) do
    desc = Map.fetch!(pool.by_token, token)
    {^token, n} = Map.fetch!(pool.values, desc)
    retired = if n == 1, do: [token | pool.retired], else: pool.retired
    %{pool | values: Map.put(pool.values, desc, {token, n - 1}), retired: retired}
  end

  # Delay deletion until all new claims: changing the visitation order cannot
  # make an equal, still-live descriptor acquire a different token.
  def finish(pool) do
    Enum.reduce(pool.retired, %{pool | retired: []}, fn token, acc ->
      desc = Map.get(acc.by_token, token)

      if Map.get(acc.values, desc) == {token, 0} do
        %{acc | values: Map.delete(acc.values, desc), by_token: Map.delete(acc.by_token, token)}
      else
        acc
      end
    end)
  end
end
