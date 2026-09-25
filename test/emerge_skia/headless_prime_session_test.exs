defmodule EmergeSkia.HeadlessPrimeSessionTest do
  use ExUnit.Case, async: false

  alias EmergeSkia.{Assets, HeadlessPrimeSession, Options}

  test "startup returns a shared completion flag and stop proves cleanup even after exit" do
    session = start_session()
    monitor = Process.monitor(session.pid)

    assert is_reference(session.completion)
    assert :atomics.get(session.completion, 1) == 0
    assert HeadlessPrimeSession.running?(session)
    assert {:ok, %{cleanup_complete: false}} = HeadlessPrimeSession.status(session)
    assert {:ok, ^session} = GenServer.call(session.pid, :renderer)

    assert :ok = HeadlessPrimeSession.stop(session)
    assert_receive {:DOWN, ^monitor, :process, _pid, :normal}
    assert :atomics.get(session.completion, 1) == 1

    assert {:ok, %{state: :stopped, cleanup_complete: true}} =
             HeadlessPrimeSession.status(session)

    assert :ok = HeadlessPrimeSession.stop(session)
    refute HeadlessPrimeSession.running?(session)
  end

  test "a dead session without proven completion never reports successful cleanup" do
    pid = spawn(fn -> :ok end)
    monitor = Process.monitor(pid)
    assert_receive {:DOWN, ^monitor, :process, ^pid, _reason}
    completion = :atomics.new(1, signed: false)
    session = %HeadlessPrimeSession{pid: pid, renderer: make_ref(), completion: completion}

    assert {:error, :cleanup_unproven} = HeadlessPrimeSession.stop(session)
    :atomics.put(completion, 1, 2)
    assert {:error, :cleanup_unproven} = HeadlessPrimeSession.stop(session)
  end

  defp start_session do
    # Exercise the real relay initialization/dispatcher/LeaseOwner lifecycle with
    # a raster renderer: no frames are issued and CI needs no DMA-BUF hardware.
    opts =
      Options.build_start_native_opts!(
        backend: :headless,
        rendering_api: :raster,
        width: 2,
        height: 2,
        headless: [target: self(), mode: :binary]
      )
      |> Map.merge(
        Assets.native_start_asset_config(Assets.normalize_asset_config!(otp_app: :emerge))
      )
      |> Map.put(:drm_cursor, [])

    assert {:ok, session} = HeadlessPrimeSession.start(opts)
    on_exit(fn -> HeadlessPrimeSession.stop(session) end)
    session
  end
end
