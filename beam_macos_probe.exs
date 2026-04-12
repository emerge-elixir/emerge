if :os.type() != {:unix, :darwin} do
  raise "beam_macos_probe.exs must be run on macOS"
end

defmodule BeamMacosProbe do
  alias EmergeSkia.Native

  def run do
    IO.puts("== BEAM macOS NIF probe ==")
    IO.puts("schedulers_online=#{System.schedulers_online()}")
    IO.puts("dirty_io_schedulers=#{:erlang.system_info(:dirty_io_schedulers)}")
    IO.puts("dirty_cpu_schedulers=#{:erlang.system_info(:dirty_cpu_schedulers)}")

    inspect_call("load context", fn -> Native.macos_probe_load_context() end)
    inspect_call("regular call context", fn -> Native.macos_probe_call_context() end)
    inspect_call("dirty call context", fn -> Native.macos_probe_dirty_call_context() end)
    inspect_call("spawned thread context", fn -> Native.macos_probe_spawned_thread_context() end)

    inspect_concurrent("concurrent regular call contexts", fn ->
      Native.macos_probe_call_context()
    end)

    inspect_concurrent("concurrent dirty call contexts", fn ->
      Native.macos_probe_dirty_call_context()
    end)
  end

  defp inspect_call(label, fun) do
    IO.puts("\n-- #{label} --")
    IO.inspect(run_call(fun), pretty: true, limit: :infinity)
  end

  defp inspect_concurrent(label, fun) do
    IO.puts("\n-- #{label} --")

    1..8
    |> Task.async_stream(fn _ -> run_call(fun) end,
      max_concurrency: min(8, System.schedulers_online()),
      ordered: false,
      timeout: 5_000
    )
    |> Enum.map(fn
      {:ok, result} -> result
      {:exit, reason} -> {:task_exit, reason}
    end)
    |> Enum.each(&IO.inspect(&1, pretty: true, limit: :infinity))
  end

  defp run_call(fun) do
    try do
      fun.()
    rescue
      error -> {:raised, error, __STACKTRACE__}
    catch
      kind, reason -> {:caught, kind, reason}
    end
  end
end

BeamMacosProbe.run()
