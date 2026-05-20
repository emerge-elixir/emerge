Code.require_file("support/benchee_config.exs", __DIR__)
Code.require_file("support/scenarios.exs", __DIR__)

alias Emerge.Bench.Config
alias Emerge.Bench.Scenarios
alias Emerge.Engine
alias Emerge.Engine.DiffState
alias Emerge.Engine.Patch
alias Emerge.Engine.Reconcile

inputs =
  Scenarios.inputs()
  |> Enum.flat_map(fn {scenario_label, input} ->
    Scenarios.mutation_ids()
    |> Enum.map(fn mutation ->
      variant = Map.fetch!(input.variants, mutation)

      {vdom, patches, assigned, _next_id} =
        Reconcile.reconcile(input.state.vdom, variant, input.state.next_id)

      {
        "#{scenario_label}/#{mutation}",
        %{
          state: input.state,
          variant: variant,
          vdom: vdom,
          patches: patches,
          assigned: assigned
        }
      }
    end)
  end)
  |> Map.new()

Benchee.run(
  %{
    "engine/update/public_assigned" => fn %{state: state, variant: variant} ->
      Engine.diff_state_update(state, variant)
    end,
    "engine/update/runtime_binary" => fn %{state: state, variant: variant} ->
      Engine.diff_state_update_binary(state, variant)
    end,
    "engine/reconcile/assigned" => fn %{state: state, variant: variant} ->
      Reconcile.reconcile(state.vdom, variant, state.next_id)
    end,
    "engine/reconcile/runtime_binary" => fn %{state: state, variant: variant} ->
      Reconcile.reconcile_patches(state.vdom, variant, state.next_id)
    end,
    "engine/reconcile/runtime_registry" => fn %{state: state, variant: variant} ->
      Reconcile.reconcile_patches_and_event_registry(
        state.vdom,
        variant,
        state.next_id,
        state.event_registry
      )
    end,
    "engine/patch/encode" => fn %{patches: patches} ->
      Patch.encode(patches)
    end,
    "engine/registry/assigned" => fn %{assigned: assigned} ->
      DiffState.build_event_registry(assigned)
    end,
    "engine/registry/vdom" => fn %{vdom: vdom} ->
      DiffState.build_event_registry(vdom)
    end
  },
  Config.options(inputs: inputs)
)
