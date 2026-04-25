Code.require_file("support/benchee_config.exs", __DIR__)
Code.require_file("support/native_helpers.exs", __DIR__)
Code.require_file("support/scenarios.exs", __DIR__)

alias Emerge.Bench.Config
alias Emerge.Bench.NativeHelpers
alias Emerge.Bench.Scenarios
alias EmergeSkia.Native

inputs = Scenarios.inputs()
Scenarios.print_metadata(inputs)

patch_jobs =
  Scenarios.mutation_ids()
  |> Map.new(fn mutation ->
    {"native/tree_patch/#{mutation}",
     fn %{tree: tree, patch_bins: patch_bins} ->
       Native.tree_patch(tree, Map.fetch!(patch_bins, mutation)) |> NativeHelpers.ok!()
     end}
  end)

Benchee.run(
  patch_jobs,
  Config.options(
    inputs: inputs,
    before_each: fn input ->
      Map.put(input, :tree, NativeHelpers.upload_tree!(input.full_bin))
    end
  )
)
