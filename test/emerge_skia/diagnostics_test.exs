defmodule EmergeSkia.DiagnosticsTest do
  use ExUnit.Case, async: true
  alias Emerge.Config

  test "stats enables collection without logs, and stats logs enable it without stats" do
    for {stats, logs, enabled} <- [
          {false, [], false},
          {true, [], true},
          {false, [:stats], true},
          {true, [:stats], true},
          {false, [:animation], false}
        ] do
      with_renderer(%Config.Diagnostics{stats: stats, logs: logs}, fn renderer ->
        assert {:ok, snapshot} = EmergeSkia.stats(renderer, :peek)
        assert snapshot.enabled == enabled
        assert snapshot.window.reset_on_read == false
      end)
    end
  end

  test "renderer collection cannot be configured after startup" do
    with_renderer(%Config.Diagnostics{}, fn renderer ->
      assert {:error, "renderer stats are configured when the renderer starts"} =
               EmergeSkia.stats(renderer, {:configure, %{enabled: true}})

      assert {:ok, %{enabled: false}} = EmergeSkia.stats(renderer, :peek)
    end)
  end

  defp with_renderer(diagnostics, run) do
    config = %Config{
      output: %Config.Headless{target: self(), size: {2, 2}},
      renderer: [api: :raster],
      assets: [otp_app: :emerge],
      diagnostics: diagnostics
    }

    {:ok, renderer} = EmergeSkia.start(config)

    try do
      run.(renderer)
    after
      :ok = EmergeSkia.stop(renderer)
    end
  end
end
