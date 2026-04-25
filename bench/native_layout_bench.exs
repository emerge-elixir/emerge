Code.require_file("support/benchee_config.exs", __DIR__)
Code.require_file("support/native_helpers.exs", __DIR__)
Code.require_file("support/scenarios.exs", __DIR__)

alias Emerge.Bench.Config
alias Emerge.Bench.NativeHelpers
alias Emerge.Bench.Scenarios
alias EmergeSkia.Native

inputs = Scenarios.inputs()
Scenarios.print_metadata(inputs)

Benchee.run(
  %{
    "native/tree_layout/full_layout_collect_frames" => fn %{tree: tree, constraint: constraint} ->
      Native.tree_layout(tree, constraint.width, constraint.height, constraint.scale)
      |> NativeHelpers.unwrap!()
    end
  },
  Config.options(
    inputs: inputs,
    before_each: fn input ->
      input
      |> Map.put(:tree, NativeHelpers.upload_tree!(input.full_bin))
      |> Map.put(:constraint, input.constraint)
    end
  )
)

Benchee.run(
  %{
    "native/tree_upload/decode_replace" => fn %{full_bin: full_bin} ->
      tree = Native.tree_new()
      Native.tree_upload(tree, full_bin) |> NativeHelpers.ok!()
      tree
    end,
    "native/tree_roundtrip/decode_encode" => fn %{full_bin: full_bin} ->
      Native.tree_roundtrip(full_bin) |> NativeHelpers.unwrap!()
    end
  },
  Config.options(inputs: inputs)
)
