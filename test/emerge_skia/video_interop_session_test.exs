defmodule EmergeSkia.VideoInteropSessionTest do
  use ExUnit.Case, async: false

  import ExUnit.CaptureLog

  alias EmergeSkia.{HeadlessPrimeSession, Native, VideoConsumerSession, VideoTarget}
  alias EmergeSkia.VideoTargetConsumer
  alias VideoInterop.{ConsumerContractError, Format, Frame, Lease, LeaseOwner, Rect}
  alias VideoInterop.DMABuf
  alias VideoInterop.DMABuf.Descriptor

  @abgr8888 VideoInterop.DMABuf.FourCC.from_string!("AB24")

  defp target do
    %VideoTarget{id: "preview", width: 64, height: 32, mode: :prime, ref: make_ref()}
  end

  defp format(alpha_mode) do
    %Format{
      width: 64,
      height: 32,
      framerate: nil,
      storage: %DMABuf.Format{fourcc: @abgr8888, modifier: :per_buffer},
      interlace_mode: :progressive,
      alpha_mode: alpha_mode
    }
  end

  defp malformed_frame do
    %Frame{
      coded_width: 64,
      coded_height: 32,
      visible_rect: %Rect{x: 0, y: 0, width: 64, height: 32},
      storage: %Descriptor{objects: :not_a_list, layers: []},
      acquire_sync: :implicit,
      lease: Lease.new(self(), :malformed_frame)
    }
  end

  test "raw native frame decode normalizes malformed nested data as caller-owned" do
    assert {:error, {:caller_owned, reason}} =
             Native.video_consumer_decode_for_test(malformed_frame())

    assert reason =~ "invalid VideoInterop.Frame"
  end

  test "invalid native receipts raise with ownership unknown" do
    error =
      assert_raise ConsumerContractError, fn ->
        VideoConsumerSession.normalize_transfer_result({:ok, :unexpected})
      end

    assert error.ownership == :unknown
    assert error.result == {:invalid_native_receipt, {:ok, :unexpected}}
  end

  test "native implementation exceptions are consumer contract errors with unknown ownership" do
    session = %VideoConsumerSession{ref: make_ref(), monitor: self()}

    error =
      assert_raise ConsumerContractError, fn ->
        VideoInterop.consume(session, malformed_frame())
      end

    assert error.ownership == :unknown
    assert error.operation == :transfer
    assert match?({:exception, :error, _reason}, error.result)
    refute_receive {:video_interop_release, :malformed_frame, _holder}
  end

  test "ABGR rejects straight alpha and accepts opaque or premultiplied alpha" do
    assert {:error, {:unsupported_alpha_mode, :straight}} =
             VideoTargetConsumer.validate_target_format(target(), format(:straight))

    assert :ok = VideoTargetConsumer.validate_target_format(target(), format(:opaque))
    assert :ok = VideoTargetConsumer.validate_target_format(target(), format(:premultiplied))
  end

  test "lease release failures are recorded and logged while LeaseOwner retains retry ownership" do
    state = %{lease_owner: self(), release_failure_count: 0, last_release_failure: nil}

    log =
      capture_log(fn ->
        assert {:noreply, updated} =
                 HeadlessPrimeSession.handle_info(
                   {:video_interop_lease_release_failed, self(), :token, %{sequence: 7}, :busy},
                   state
                 )

        assert updated.release_failure_count == 1
        assert updated.last_release_failure == {:token, %{sequence: 7}, :busy}
      end)

    assert log =~ "VideoInterop.LeaseOwner will retry"
    assert log =~ "reason=:busy"
  end

  test "external destination exit begins drained shutdown" do
    owner =
      start_supervised!(
        {LeaseOwner,
         producer: self(), release: fn _token -> :ok end, max_active: 2, notify: self()}
      )

    monitor = make_ref()

    assert {:noreply, state} =
             HeadlessPrimeSession.handle_info(
               {:DOWN, monitor, :process, self(), :normal},
               %{
                 mode: :open,
                 destination: {:external, self()},
                 destination_monitor: monitor,
                 lease_owner: owner
               }
             )

    assert state.mode == :draining
    assert state.destination == :disconnected
    assert state.destination_monitor == nil
    assert_receive {:video_interop_lease_owner_drained, ^owner}
  end

  test "shutdown result propagates native and LeaseOwner failures to stop waiters" do
    assert :ok = HeadlessPrimeSession.shutdown_result_for_test({:ok, :ok}, nil)

    assert {:error, {:native_stop_failed, "backend join failed"}} =
             HeadlessPrimeSession.shutdown_result_for_test(
               {:error, "backend join failed"},
               nil
             )

    assert {:error, {:lease_owner_exit, :boom}} =
             HeadlessPrimeSession.shutdown_result_for_test(
               {:ok, :ok},
               {:lease_owner_exit, :boom}
             )

    assert {:error, {{:lease_owner_exit, :boom}, {:native_stop_failed, "backend join failed"}}} =
             HeadlessPrimeSession.shutdown_result_for_test(
               {:error, "backend join failed"},
               {:lease_owner_exit, :boom}
             )
  end

  test "inactive submission stays connected while terminal errors disconnect" do
    assert :keep = HeadlessPrimeSession.submission_error_action(:inactive)
    assert :disconnect = HeadlessPrimeSession.submission_error_action(:stale_target)
    assert :disconnect = HeadlessPrimeSession.submission_error_action(:invalid_frame)
  end
end
