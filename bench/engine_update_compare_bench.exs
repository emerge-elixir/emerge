Code.require_file("support/scenarios.exs", __DIR__)

alias Emerge.Bench.Scenarios
alias Emerge.Engine
alias Emerge.Engine.DiffState
alias Emerge.Engine.Patch
alias Emerge.Engine.Reconcile

defmodule Emerge.Bench.UpdatePathCompare do
  @moduledoc false

  def bench(_fun, _warmup, reps) when reps <= 0 do
    raise ArgumentError, "REPS must be positive"
  end

  def bench(fun, warmup, reps) do
    if warmup > 0 do
      for _ <- 1..warmup, do: fun.()
    end

    :erlang.garbage_collect(self())

    samples =
      for _ <- 1..reps do
        {us, _result} = :timer.tc(fun)
        us
      end
      |> Enum.sort()

    count = length(samples)
    median = Enum.at(samples, div(count, 2))
    p95 = Enum.at(samples, max(0, floor(count * 0.95) - 1))
    mean = Enum.sum(samples) / count

    {median, p95, mean}
  end
end

reps = String.to_integer(System.get_env("REPS") || "300")
warmup = String.to_integer(System.get_env("WARMUP") || "30")

mutations =
  (System.get_env("BENCH_MUTATIONS") ||
     "noop,event_attr,keyed_reorder,insert_tail,remove_tail,nearby_reorder,nearby_slot_change")
  |> String.split(",", trim: true)
  |> Enum.map(&String.to_atom/1)

label = System.get_env("BENCH_LABEL") || "current"

inputs = Scenarios.inputs()

runtime_update? = function_exported?(Engine, :diff_state_update_binary, 2)

runtime_reconcile_registry_4? =
  function_exported?(Reconcile, :reconcile_patches_and_event_registry, 4)

runtime_reconcile_registry_3? =
  function_exported?(Reconcile, :reconcile_patches_and_event_registry, 3)

runtime_reconcile_patches? = function_exported?(Reconcile, :reconcile_patches, 3)

IO.puts("label,scenario,mutation,stage,median_us,p95_us,mean_us,reps")

for {scenario_label, input} <- inputs,
    mutation <- mutations,
    variant = Map.fetch!(input.variants, mutation) do
  {_vdom, patches, assigned, _next_id} =
    Reconcile.reconcile(input.state.vdom, variant, input.state.next_id)

  jobs = [
    {:public_update, fn -> Engine.diff_state_update(input.state, variant) end},
    {:reconcile_assigned,
     fn -> Reconcile.reconcile(input.state.vdom, variant, input.state.next_id) end},
    {:event_registry_assigned, fn -> DiffState.build_event_registry(assigned) end},
    {:patch_encode, fn -> Patch.encode(patches) end}
  ]

  jobs =
    if runtime_update? do
      [{:runtime_update, fn -> Engine.diff_state_update_binary(input.state, variant) end} | jobs]
    else
      jobs
    end

  jobs =
    cond do
      runtime_reconcile_registry_4? ->
        [
          {:runtime_reconcile_registry,
           fn ->
             Reconcile.reconcile_patches_and_event_registry(
               input.state.vdom,
               variant,
               input.state.next_id,
               input.state.event_registry
             )
           end}
          | jobs
        ]

      runtime_reconcile_registry_3? ->
        [
          {:runtime_reconcile_registry,
           fn ->
             Reconcile.reconcile_patches_and_event_registry(
               input.state.vdom,
               variant,
               input.state.next_id
             )
           end}
          | jobs
        ]

      true ->
        jobs
    end

  jobs =
    if runtime_reconcile_patches? do
      [
        {:runtime_reconcile_patches_only,
         fn -> Reconcile.reconcile_patches(input.state.vdom, variant, input.state.next_id) end}
        | jobs
      ]
    else
      jobs
    end

  jobs
  |> Enum.reverse()
  |> Enum.each(fn {stage, fun} ->
    {median, p95, mean} = Emerge.Bench.UpdatePathCompare.bench(fun, warmup, reps)

    IO.puts(
      "#{label},#{scenario_label},#{mutation},#{stage},#{median},#{p95},#{Float.round(mean, 1)},#{reps}"
    )
  end)
end
