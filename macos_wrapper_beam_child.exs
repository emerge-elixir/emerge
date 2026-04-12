if :os.type() != {:unix, :darwin} do
  raise "macos_wrapper_beam_child.exs must be run on macOS"
end

defmodule MacosWrapperBeamChild do
  import Bitwise

  alias Emerge.UI
  alias EmergeSkia.Macos.Renderer

  def run do
    socket = System.get_env("EMERGE_SKIA_MACOS_HOST_SOCKET")

    if socket in [nil, ""] do
      raise "EMERGE_SKIA_MACOS_HOST_SOCKET is required for wrapper smoke child"
    end

    IO.puts("== Wrapper-started BEAM macOS smoke child ==")
    IO.puts("host socket=#{socket}")

    {:ok, renderer1} = start_renderer("Wrapper macOS child 1", 620, 400)
    IO.puts("child renderer1 backend=#{renderer1.macos_backend}")
    assert_running(renderer1, "renderer1 should be running after start")
    flush_mailbox()
    register_targets(renderer1)
    exercise_tree_render(renderer1)
    assert_notifications("child renderer1")

    {:ok, renderer2} = start_renderer("Wrapper macOS child 2", 700, 460)
    IO.puts("child renderer2 backend=#{renderer2.macos_backend}")
    assert_running(renderer2, "renderer2 should be running after start")
    flush_mailbox()
    register_targets(renderer2)
    exercise_tree_render(renderer2)
    assert_notifications("child renderer2")

    assert_same_host(renderer1, renderer2)

    IO.puts("child stopping renderer1")
    :ok = EmergeSkia.stop(renderer1)
    Process.sleep(300)
    assert_stopped(renderer1, "renderer1 should be stopped after stop/1")
    assert_running(renderer2, "renderer2 should still be running after renderer1 stops")

    {:ok, renderer3} = start_renderer("Wrapper macOS child 3", 760, 500)
    IO.puts("child renderer3 backend=#{renderer3.macos_backend}")
    assert_running(renderer3, "renderer3 should be running after start")
    assert_same_host(renderer2, renderer3)
    flush_mailbox()
    register_targets(renderer3)
    exercise_tree_render(renderer3)
    assert_notifications("child renderer3")

    {:ok, renderer4} =
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :macos,
        macos_backend: :raster,
        title: "Wrapper macOS child forced raster",
        width: 560,
        height: 340
      )

    IO.puts("child renderer4 backend=#{renderer4.macos_backend}")
    assert_true(renderer4.macos_backend == :raster, "renderer4 should force raster backend")
    flush_mailbox()
    register_targets(renderer4)
    exercise_tree_render(renderer4)
    assert_notifications("child renderer4")

    IO.puts("child stopping renderer2 and renderer3")
    :ok = EmergeSkia.stop(renderer2)
    :ok = EmergeSkia.stop(renderer3)
    :ok = EmergeSkia.stop(renderer4)
    Process.sleep(300)
    assert_stopped(renderer2, "renderer2 should be stopped after stop/1")
    assert_stopped(renderer3, "renderer3 should be stopped after stop/1")
    assert_stopped(renderer4, "renderer4 should be stopped after stop/1")

    IO.puts("wrapper-started BEAM macOS smoke child passed")
  end

  defp start_renderer(title, width, height) do
    EmergeSkia.start(
      otp_app: :emerge,
      backend: :macos,
      title: title,
      width: width,
      height: height
    )
  end

  defp assert_same_host(%Renderer{} = left, %Renderer{} = right) do
    if left.host_id != right.host_id or left.host_pid != right.host_pid do
      raise "expected renderers to share one macOS host, got #{inspect(left)} and #{inspect(right)}"
    end
  end

  defp assert_running(renderer, message) do
    if not EmergeSkia.running?(renderer) do
      raise message
    end
  end

  defp assert_stopped(renderer, message) do
    if EmergeSkia.running?(renderer) do
      raise message
    end
  end

  defp assert_true(true, _message), do: :ok

  defp assert_true(false, message) do
    raise message
  end

  defp exercise_tree_render(renderer) do
    tree1 =
      UI.column([], [UI.text("wrapper child initial")])

    {state, _assigned1} = EmergeSkia.TreeRenderer.upload_tree(renderer, tree1)
    Process.sleep(150)

    tree2 =
      UI.column([], [
        UI.text("wrapper child patched"),
        UI.text("second line")
      ])

    {_state, _assigned2} = EmergeSkia.TreeRenderer.patch_tree(renderer, state, tree2)
    Process.sleep(150)
  end

  defp register_targets(renderer) do
    :ok = EmergeSkia.set_input_target(renderer, self())
    :ok = EmergeSkia.set_log_target(renderer, self())
    :ok = EmergeSkia.set_input_mask(renderer, 0x04 ||| 0x08 ||| 0x10 ||| 0x20 ||| 0x40 ||| 0x80)
  end

  defp assert_notifications(label) do
    assert_receive_match(fn
      {:emerge_skia_log, level, source, message}
      when level in [:info, :warning, :error] and is_binary(source) and is_binary(message) ->
        IO.puts("#{label} log=#{level} source=#{source} message=#{message}")
        true

      _ ->
        false
    end, "#{label} should receive a native log")

    assert_receive_match(fn
      {:emerge_skia_event, {:resized, {width, height, scale_factor}}}
      when is_integer(width) and is_integer(height) and is_float(scale_factor) ->
        IO.puts("#{label} resized=#{width}x#{height} scale=#{scale_factor}")
        true

      _ ->
        false
    end, "#{label} should receive a resize event")
  end

  defp assert_receive_match(fun, message, timeout \\ 2_000) when is_function(fun, 1) do
    deadline = System.monotonic_time(:millisecond) + timeout
    await_match(fun, message, deadline)
  end

  defp await_match(fun, message, deadline) do
    remaining = max(deadline - System.monotonic_time(:millisecond), 0)

    receive do
      payload ->
        if fun.(payload) do
          :ok
        else
          await_match(fun, message, deadline)
        end
    after
      remaining ->
        raise message
    end
  end

  defp flush_mailbox do
    receive do
      _message -> flush_mailbox()
    after
      0 -> :ok
    end
  end
end

MacosWrapperBeamChild.run()
