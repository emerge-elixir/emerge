Code.require_file("probe.exs", __DIR__)

case System.argv() do
  [output] ->
    IO.write(Emerge.Benchmarks.SharedAnimation.summary!(output))

  _ ->
    raise ArgumentError, "usage: elixir scripts/benchmarks/shared-animation/summarize.exs OUTPUT"
end
