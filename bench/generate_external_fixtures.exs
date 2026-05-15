fixture_root = Path.expand("external_fixtures", __DIR__)

defmodule Emerge.Bench.ExternalFixtures do
  @moduledoc false

  alias Emerge.Engine
  alias EmergeDemo.Showcase

  def generate_showcase_interaction!(fixture_root) do
    app = start_showcase_app!(Showcase.App)

    Process.put(:solve_app, app)
    set_showcase_page!(app, :interaction)
    dispatch_showcase_event!(app, :text_input, :focused)
    wait_until!(fn -> Solve.Lookup.solve(app, :text_input).focused? end)

    initial_tree = Showcase.View.layout()
    {full_bin, diff_state, _assigned} = Engine.encode_full(Engine.diff_state_new(), initial_tree)

    dispatch_showcase_event!(app, :text_input, :changed, "quick brown foxa")
    wait_until!(fn -> Solve.Lookup.solve(app, :text_input).value == "quick brown foxa" end)

    patched_tree = Showcase.View.layout()
    {patch_bin, next_state, _assigned} = Engine.diff_state_update(diff_state, patched_tree)

    {reverse_patch_bin, _restored_state, _assigned} =
      Engine.diff_state_update(next_state, initial_tree)

    fixture_dir = Path.join(fixture_root, "emerge_demo_showcase_interaction")
    File.rm_rf!(fixture_dir)
    File.mkdir_p!(fixture_dir)
    File.write!(Path.join(fixture_dir, "full.emrg"), full_bin)
    File.write!(Path.join(fixture_dir, "virtual_key_text_echo.patch"), patch_bin)
    File.write!(Path.join(fixture_dir, "virtual_key_text_echo_reverse.patch"), reverse_patch_bin)

    IO.puts(
      "wrote #{Path.relative_to_cwd(fixture_dir)} " <>
        "full=#{byte_size(full_bin)}B patch=#{byte_size(patch_bin)}B " <>
        "reverse=#{byte_size(reverse_patch_bin)}B"
    )
  end

  defp start_showcase_app!(name) do
    case Showcase.App.start_link(name: name) do
      {:ok, pid} ->
        pid

      {:error, {:already_started, pid}} ->
        pid
    end
  end

  defp set_showcase_page!(app, page) do
    _ = Solve.Lookup.solve(app, :pages)
    Solve.dispatch(app, :pages, :set_page, page)
    wait_until!(fn -> Solve.Lookup.solve(app, :pages).current == page end)
  end

  defp dispatch_showcase_event!(app, controller, event, payload \\ %{}) do
    _ = Solve.Lookup.solve(app, controller)
    Solve.dispatch(app, controller, event, payload)
    flush_lookup_updates()
  end

  defp wait_until!(predicate, retries \\ 100)

  defp wait_until!(predicate, retries) when retries > 0 do
    flush_lookup_updates()

    if predicate.() do
      :ok
    else
      Process.sleep(5)
      wait_until!(predicate, retries - 1)
    end
  end

  defp wait_until!(_predicate, 0), do: raise("timed out waiting for fixture state")

  defp flush_lookup_updates do
    receive do
      message ->
        _ = Solve.Lookup.handle_message(message)
        flush_lookup_updates()
    after
      0 -> :ok
    end
  end
end

Mix.Task.run("app.start")
Emerge.Bench.ExternalFixtures.generate_showcase_interaction!(fixture_root)
