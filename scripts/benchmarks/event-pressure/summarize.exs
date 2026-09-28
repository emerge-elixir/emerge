Code.require_file("probe.exs", __DIR__)

case System.argv() do
  [output] -> Emerge.Benchmarks.EventPressure.summarize!(output)
  _ -> raise ArgumentError, "usage: elixir scripts/benchmarks/event-pressure/summarize.exs OUTPUT"
end
