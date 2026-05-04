defmodule Emerge.Bench.Config do
  @moduledoc false

  def options(extra \\ []) do
    Keyword.merge(
      [
        warmup: env_float("EMERGE_BENCH_WARMUP", 1.0),
        time: env_float("EMERGE_BENCH_TIME", 3.0),
        memory_time: env_float("EMERGE_BENCH_MEMORY_TIME", 0.0),
        parallel: env_integer("EMERGE_BENCH_PARALLEL", 1)
      ],
      extra
    )
  end

  defp env_float(name, default) do
    case System.get_env(name) do
      nil ->
        default

      value ->
        case Float.parse(value) do
          {parsed, ""} -> parsed
          _ -> default
        end
    end
  end

  defp env_integer(name, default) do
    case System.get_env(name) do
      nil -> default
      value -> String.to_integer(value)
    end
  end
end
