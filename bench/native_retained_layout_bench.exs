Code.require_file("support/benchee_config.exs", __DIR__)
Code.require_file("support/native_helpers.exs", __DIR__)
Code.require_file("support/scenarios.exs", __DIR__)

defmodule Emerge.Bench.NativeRetainedLayout do
  @moduledoc false

  alias Emerge.Bench.NativeHelpers
  alias Emerge.Bench.Scenarios
  alias EmergeSkia.Native

  @default_scenarios [:list_text, :text_rich, :layout_matrix, :paint_rich, :nearby_rich]
  @retained_mutations [
    :noop,
    :paint_attr,
    :event_attr,
    :layout_attr,
    :text_content,
    :keyed_reorder,
    :insert_tail,
    :remove_tail,
    :nearby_slot_change,
    :nearby_reorder,
    :animation_attr
  ]
  @cache_stat_keys [
    :intrinsic_measure_hits,
    :intrinsic_measure_misses,
    :intrinsic_measure_stores,
    :subtree_measure_hits,
    :subtree_measure_misses,
    :subtree_measure_stores,
    :resolve_hits,
    :resolve_misses,
    :resolve_stores
  ]

  def base_inputs do
    Scenarios.inputs(Scenarios.sizes(), scenario_ids())
  end

  def retained_inputs(inputs) do
    Map.new(inputs, fn {label, input} ->
      {label, Map.put(input, :tree, upload_warm_tree!(input))}
    end)
  end

  def mutation_inputs(inputs) do
    for {label, input} <- inputs, mutation <- retained_mutations(), into: %{} do
      {"#{label}/#{mutation}", Map.put(input, :mutation, mutation)}
    end
  end

  def prepare_after_patch_input(input) do
    tree = upload_warm_tree!(input)

    tree
    |> Native.tree_patch(Map.fetch!(input.patch_bins, input.mutation))
    |> NativeHelpers.ok!()

    Map.put(input, :tree, tree)
  end

  def prepare_patch_layout_input(input) do
    input
    |> Map.put(:tree, upload_warm_tree!(input))
    |> Map.put(:patch_bin, Map.fetch!(input.patch_bins, input.mutation))
  end

  def layout!(tree, constraint) do
    tree
    |> Native.tree_layout(constraint.width, constraint.height, constraint.scale)
    |> NativeHelpers.unwrap!()
  end

  def upload_warm_tree!(input) do
    tree = NativeHelpers.upload_tree!(input.full_bin)
    layout!(tree, input.constraint)
    tree
  end

  def enable_stats!(tree) do
    tree
    |> Native.stats({:configure, %{enabled: true}})
    |> NativeHelpers.unwrap!()

    tree
  end

  def reset_stats!(tree) do
    tree
    |> Native.stats(:reset)
    |> NativeHelpers.unwrap!()

    tree
  end

  def print_cache_stats(inputs, mutation_inputs) do
    inputs
    |> sorted_inputs()
    |> Enum.each(fn {label, input} ->
      tree = upload_warm_tree!(input)
      enable_stats!(tree)
      reset_stats!(tree)
      layout!(tree, input.constraint)
      print_cache_stats_line(label, :warm_cache, tree)
    end)

    mutation_inputs
    |> sorted_inputs()
    |> Enum.each(fn {label, input} ->
      prepared = prepare_after_patch_input(input)
      enable_stats!(prepared.tree)
      reset_stats!(prepared.tree)
      layout!(prepared.tree, prepared.constraint)
      print_cache_stats_line(label, :after_patch, prepared.tree)
    end)

    mutation_inputs
    |> sorted_inputs()
    |> Enum.each(fn {label, input} ->
      prepared = prepare_patch_layout_input(input)
      enable_stats!(prepared.tree)
      reset_stats!(prepared.tree)
      prepared.tree |> Native.tree_patch(prepared.patch_bin) |> NativeHelpers.ok!()
      layout!(prepared.tree, prepared.constraint)
      print_cache_stats_line(label, :patch_layout, prepared.tree)
    end)
  end

  defp scenario_ids do
    case System.get_env("EMERGE_BENCH_SCENARIOS") do
      nil -> @default_scenarios
      _value -> Scenarios.scenario_ids()
    end
  end

  defp retained_mutations do
    case System.get_env("EMERGE_BENCH_MUTATIONS") do
      nil ->
        @retained_mutations

      value ->
        requested = String.split(value, ",", trim: true)
        selected = Enum.filter(@retained_mutations, &(Atom.to_string(&1) in requested))

        if selected == [] do
          raise "no retained benchmark mutations matched EMERGE_BENCH_MUTATIONS=#{value}"
        end

        selected
    end
  end

  defp sorted_inputs(inputs), do: Enum.sort_by(inputs, fn {label, _input} -> label end)

  defp print_cache_stats_line(label, phase, tree) do
    stats = tree |> Native.stats(:take) |> NativeHelpers.unwrap!()
    layout_cache_stats = stats.counters.layout_cache

    formatted_stats =
      @cache_stat_keys
      |> Enum.map(fn key -> "#{key}=#{Map.fetch!(layout_cache_stats, key)}" end)
      |> Enum.join(" ")

    IO.puts("layout_cache_stats case=#{label} phase=#{phase} #{formatted_stats}")
  end
end

alias Emerge.Bench.Config
alias Emerge.Bench.NativeHelpers
alias Emerge.Bench.NativeRetainedLayout
alias Emerge.Bench.Scenarios
alias EmergeSkia.Native

inputs = NativeRetainedLayout.base_inputs()
mutation_inputs = NativeRetainedLayout.mutation_inputs(inputs)
Scenarios.print_metadata(inputs)
NativeRetainedLayout.print_cache_stats(inputs, mutation_inputs)

Benchee.run(
  %{
    "native/tree_layout_retained/warm_cache" => fn %{tree: tree, constraint: constraint} ->
      NativeRetainedLayout.layout!(tree, constraint)
    end
  },
  Config.options(
    inputs: NativeRetainedLayout.retained_inputs(inputs),
    parallel: 1
  )
)

# Rebuild a warmed tree per invocation so the measured call is the first layout after patching.
Benchee.run(
  %{
    "native/tree_layout_retained_after_patch/layout_only" => fn %{
                                                                  tree: tree,
                                                                  constraint: constraint
                                                                } ->
      NativeRetainedLayout.layout!(tree, constraint)
    end
  },
  Config.options(
    inputs: mutation_inputs,
    before_each: &NativeRetainedLayout.prepare_after_patch_input/1,
    parallel: 1
  )
)

Benchee.run(
  %{
    "native/tree_patch_layout_retained/apply_patch_then_layout" => fn %{
                                                                        tree: tree,
                                                                        patch_bin: patch_bin,
                                                                        constraint: constraint
                                                                      } ->
      tree |> Native.tree_patch(patch_bin) |> NativeHelpers.ok!()
      NativeRetainedLayout.layout!(tree, constraint)
    end
  },
  Config.options(
    inputs: mutation_inputs,
    before_each: &NativeRetainedLayout.prepare_patch_layout_input/1,
    parallel: 1
  )
)
