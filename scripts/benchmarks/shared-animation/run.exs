Code.require_file("probe.exs", __DIR__)

{options, args, invalid} = OptionParser.parse(System.argv(), strict: [closeout: :boolean])

case {args, invalid} do
  {[output], []} ->
    Emerge.Benchmarks.SharedAnimation.run!(output, Keyword.get(options, :closeout, false))

  _ ->
    raise ArgumentError,
          "usage: elixir scripts/benchmarks/shared-animation/run.exs OUTPUT [--closeout]"
end
