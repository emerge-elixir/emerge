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

  # Until all new claims are known, even a descriptor losing its last old
  # reference can be reclaimed. Queue each decrement rather than writing a
  # zero-count entry only to delete it at the end of the same update.
  def release(pool, token), do: %{pool | retired: [token | pool.retired]}

  def finish(pool), do: Enum.reduce(pool.retired, %{pool | retired: []}, &drop_ref/2)

  defp drop_ref(token, pool) do
    desc = Map.fetch!(pool.by_token, token)

    case Map.fetch!(pool.values, desc) do
      {^token, 1} ->
        %{
          pool
          | values: Map.delete(pool.values, desc),
            by_token: Map.delete(pool.by_token, token)
        }

      {^token, n} when n > 1 ->
        %{pool | values: Map.put(pool.values, desc, {token, n - 1})}
    end
  end
end
