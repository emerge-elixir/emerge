defmodule Emerge.Mix.InputSourceCache do
  @moduledoc false

  def pins!(manifest) do
    [_, sources] = Regex.run(~r/pub const SOURCES:.*?= &\[(.*?)\n\];/s, manifest)

    pins =
      Regex.scan(~r/archive:\s*"([^"]+)".*?sha256:\s*"([0-9a-f]{64})"/s, sources,
        capture: :all_but_first
      )
      |> Enum.map(fn [archive, hash] -> {archive, hash} end)

    if pins == [] or length(pins) != length(Regex.scan(~r/Source\s*\{/, sources)) or
         Enum.any?(pins, fn {archive, _} -> Path.basename(archive) != archive end) do
      raise ArgumentError, "invalid pinned input source manifest"
    end

    pins
  end

  def complete?(cache, pins) do
    pins != [] and
      Enum.all?(pins, fn {archive, expected} ->
        case File.read(Path.join(cache, archive)) do
          {:ok, bytes} -> Base.encode16(:crypto.hash(:sha256, bytes), case: :lower) == expected
          {:error, _} -> false
        end
      end)
  end
end
