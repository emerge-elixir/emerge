Code.require_file("support/benchee_config.exs", __DIR__)
Code.require_file("support/scenarios.exs", __DIR__)

alias Emerge.Bench.Config
alias Emerge.Bench.Scenarios
alias Emerge.Engine.AttrCodec
alias Emerge.Engine.Patch
alias Emerge.Engine.Serialization

inputs = Scenarios.inputs()
Scenarios.print_metadata(inputs)

patch_jobs =
  Scenarios.mutation_ids()
  |> Enum.flat_map(fn mutation ->
    [
      {"engine/patch/encode/#{mutation}",
       fn %{patch_bins: patch_bins} ->
         patch_bins
         |> Map.fetch!(mutation)
         |> Patch.decode()
         |> Patch.encode()
       end},
      {"engine/patch/decode/#{mutation}",
       fn %{patch_bins: patch_bins} ->
         patch_bins
         |> Map.fetch!(mutation)
         |> Patch.decode()
       end}
    ]
  end)
  |> Map.new()

attr_jobs = %{
  "engine/attrs/encode/paint" => fn _input ->
    AttrCodec.encode_attrs(%{background: {:color_rgb, {255, 0, 0}}})
  end,
  "engine/attrs/decode/paint" => fn _input ->
    AttrCodec.decode_attrs(<<0, 1, 12, 0, 1, 255, 0, 0, 255>>)
  end,
  "engine/attrs/encode/layout" => fn _input ->
    AttrCodec.encode_attrs(%{width: {:px, 240}, padding: 8, spacing: 4})
  end,
  "engine/attrs/encode/event" => fn _input ->
    AttrCodec.encode_attrs(%{on_click: true, on_press: true})
  end
}

Benchee.run(
  %{
    "engine/emrg/encode_tree" => fn %{assigned: assigned} ->
      Serialization.encode_tree(assigned)
    end,
    "engine/emrg/decode_tree" => fn %{full_bin: full_bin} ->
      Serialization.decode(full_bin)
    end
  }
  |> Map.merge(patch_jobs)
  |> Map.merge(attr_jobs),
  Config.options(inputs: inputs)
)
