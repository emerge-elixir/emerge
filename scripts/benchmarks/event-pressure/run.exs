Code.require_file("probe.exs", __DIR__)

case System.argv() do
  [output] -> Emerge.Benchmarks.EventPressure.run!(output)
  _ -> raise ArgumentError, "usage: elixir scripts/benchmarks/event-pressure/run.exs OUTPUT"
end
