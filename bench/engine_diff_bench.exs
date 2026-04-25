Code.require_file("support/benchee_config.exs", __DIR__)
Code.require_file("support/scenarios.exs", __DIR__)

alias Emerge.Bench.Config
alias Emerge.Bench.Scenarios
alias Emerge.Engine

inputs = Scenarios.inputs()
Scenarios.print_metadata(inputs)

diff_jobs =
  Scenarios.mutation_ids()
  |> Map.new(fn mutation ->
    {"engine/diff/#{mutation}",
     fn %{state: state, variants: variants} ->
       Engine.diff_state_update(state, Map.fetch!(variants, mutation))
     end}
  end)

Benchee.run(
  Map.merge(diff_jobs, %{
    "engine/full/encode" => fn %{initial: initial} ->
      Engine.encode_full(Engine.diff_state_new(), initial)
    end
  }),
  Config.options(inputs: inputs)
)
