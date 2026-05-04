Code.require_file("support/scenarios.exs", __DIR__)

alias Emerge.Bench.Scenarios

fixture_root = Path.expand("fixtures", __DIR__)
File.rm_rf!(fixture_root)
File.mkdir_p!(fixture_root)

Scenarios.inputs()
|> Enum.each(fn {label, input} ->
  scenario_dir = Path.join(fixture_root, label)
  File.mkdir_p!(scenario_dir)

  File.write!(Path.join(scenario_dir, "full.emrg"), input.full_bin)

  input.patch_bins
  |> Enum.each(fn {mutation, patch_bin} ->
    File.write!(Path.join(scenario_dir, "#{mutation}.patch"), patch_bin)
  end)

  manifest = %{
    id: label,
    scenario: input.metadata.scenario,
    size: input.metadata.size,
    item_count: input.metadata.item_count,
    constraint: input.metadata.constraint,
    node_count: input.metadata.node_count,
    text_node_count: input.metadata.text_node_count,
    full_emrg_bytes: input.metadata.full_emrg_bytes,
    element_types: input.metadata.element_types,
    attr_families: input.metadata.attr_families,
    patches: input.metadata.patches
  }

  File.write!(Path.join(scenario_dir, "manifest.json"), Jason.encode!(manifest, pretty: true))

  IO.puts("wrote benchmark fixture #{Path.relative_to_cwd(scenario_dir)}")
end)
