defmodule EmergeSkia.VideoInteropSessionTest do
  use ExUnit.Case, async: false

  import ExUnit.CaptureLog

  alias EmergeSkia.{HeadlessPrimeSession, Native, VideoConsumerSession, VideoTarget}
  alias EmergeSkia.VideoTargetConsumer

  alias VideoInterop.{
    AbandonmentGuard,
    Colorimetry,
    ConsumerContractError,
    Format,
    Frame,
    Lease,
    LeaseOwner,
    Rect
  }

  alias VideoInterop.DMABuf
  alias VideoInterop.DMABuf.{Descriptor, Layer, Object, Plane}

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

  test "stale target receipts preserve the exact native reason" do
    reasons = [
      "video consumer stream is closed: renderer_epoch=1 target=preview target_incarnation=2 stream_id=7",
      "stale video consumer stream: renderer_epoch=1 target=preview target_incarnation=2 submitted_stream_id=7 active_stream_id=8",
      "stale video target incarnation: target=preview submitted_incarnation=2 active_incarnation=3 renderer_epoch=1 stream_id=7",
      "stale video renderer epoch: session_epoch=1 registry_epoch=2 target=preview target_incarnation=3 stream_id=7",
      "unknown video target: preview; renderer_epoch=1 target_incarnation=2 stream_id=7",
      "video registry is closed: renderer_epoch=1 target=preview target_incarnation=2 stream_id=7"
    ]

    Enum.each(reasons, fn reason ->
      assert {:error, {:caller_owned, {:stale_target, ^reason}}} =
               VideoConsumerSession.normalize_transfer_result({:error, {:caller_owned, reason}})
    end)
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

  test "headless PRIME forwarding creates a unique abandonment guard for every holder" do
    {owner, dispatcher} = start_guarded_owner()

    assert {:noreply, _state} =
             HeadlessPrimeSession.handle_info(
               {"emerge_skia_internal_prime_frame", raw_prime_frame(:guarded_frame)},
               forwarding_state(owner, self())
             )

    assert_receive {:emerge_skia_frame, output}
    root = output |> Map.new() |> Map.fetch!("dmabuf") |> Map.fetch!(:lease)
    assert {:ok, child} = Lease.retain(root)
    assert %AbandonmentGuard{} = root.abandonment_guard
    assert %AbandonmentGuard{} = child.abandonment_guard
    assert AbandonmentGuard.valid?(root.abandonment_guard)
    assert AbandonmentGuard.valid?(child.abandonment_guard)
    refute root.abandonment_guard == child.abandonment_guard

    assert :ok = Lease.release(root)
    assert :ok = Lease.release(child)
    assert_receive {:backend_released, :guarded_frame}
    assert :ok = LeaseOwner.close(owner)
    assert is_reference(dispatcher)
  end

  test "headless PRIME forwarding keeps factory failure ownership with LeaseOwner" do
    {owner, dispatcher} = start_guarded_owner()
    assert :ok = Native.headless_prime_release_dispatcher_close(dispatcher)

    assert {:noreply, _state} =
             HeadlessPrimeSession.handle_info(
               {"emerge_skia_internal_prime_frame", raw_prime_frame(:factory_failure)},
               forwarding_state(owner, self())
             )

    refute_receive {:emerge_skia_frame, _output}
    assert_receive {:backend_released, :factory_failure}
    assert LeaseOwner.stats(owner).active_holders == 0
    assert :ok = LeaseOwner.close(owner)
  end

  test "external destination death abandons its guarded frame and completes drainage" do
    {owner, _dispatcher} = start_guarded_owner()
    test_pid = self()

    destination =
      spawn(fn ->
        assert {:ok, frame} = LeaseOwner.issue(owner, :external_frame)
        send(self(), {:private_frame_queue, frame})
        send(test_pid, {:external_frame_stored, self()})
        Process.sleep(:infinity)
      end)

    destination_monitor = Process.monitor(destination)
    assert_receive {:external_frame_stored, ^destination}
    assert true = :erlang.garbage_collect(owner)

    Process.exit(destination, :kill)
    assert_receive {:DOWN, ^destination_monitor, :process, ^destination, :killed}

    assert {:noreply, state} =
             HeadlessPrimeSession.handle_info(
               {:DOWN, destination_monitor, :process, destination, :killed},
               %{
                 mode: :open,
                 destination: {:external, destination},
                 destination_monitor: destination_monitor,
                 lease_owner: owner
               }
             )

    assert state.mode == :draining
    assert state.destination == :disconnected
    assert state.destination_monitor == nil
    assert_receive {:backend_released, :external_frame}, 1_000
    assert_receive {:video_interop_lease_owner_final_stats, ^owner, stats}, 1_000
    assert stats.abandonments == 1
    assert_receive {:video_interop_lease_owner_drained, ^owner}, 1_000
  end

  test "conflicting consumer open returns target_busy without leaking its dispatcher" do
    assert {:ok, {session, target_resource}} = Native.video_consumer_session_open_for_test()

    baseline_threads =
      if File.dir?("/proc/self/task"), do: native_thread_count("emerge_skia_vid")

    if baseline_threads, do: assert(baseline_threads > 0)

    Enum.each(1..20, fn _attempt ->
      assert {:error, "target_busy"} =
               Native.video_consumer_session_open(target_resource, format(:opaque))

      if baseline_threads do
        assert native_thread_count("emerge_skia_vid") == baseline_threads
      end
    end)

    assert :ok = Native.video_consumer_session_close(session)

    if baseline_threads do
      eventually(fn -> native_thread_count("emerge_skia_vid") == baseline_threads - 1 end)
    end

    assert is_reference(target_resource)
  end

  test "video target info reports exact active stream identity and rejects stale targets" do
    assert {:ok, {session, target_resource}} = Native.video_consumer_session_open_for_test()

    target = %VideoTarget{
      id: "video-consumer-test",
      width: 64,
      height: 32,
      mode: :prime,
      ref: target_resource
    }

    assert {:ok,
            %{
              renderer_epoch: renderer_epoch,
              target_id: "video-consumer-test",
              target_incarnation: target_incarnation,
              active_stream_id: active_stream_id
            } = active_info} = EmergeSkia.video_target_info(target)

    assert renderer_epoch > 0
    assert target_incarnation > 0
    assert active_stream_id > 0

    assert Map.keys(active_info) |> Enum.sort() ==
             [:active_stream_id, :renderer_epoch, :target_id, :target_incarnation]

    assert :ok = Native.video_consumer_session_close(session)
    assert {:ok, %{active_stream_id: nil}} = EmergeSkia.video_target_info(target)

    assert {:ok, true} = Native.video_consumer_target_replace_for_test(target_resource)
    assert {:error, reason} = EmergeSkia.video_target_info(target)
    assert reason =~ "stale video target incarnation"
  end

  test "complete immutable format crosses the native consumer-open boundary" do
    assert {:ok, {initial, target_resource}} = Native.video_consumer_session_open_for_test()
    assert :ok = Native.video_consumer_session_close(initial)

    stream_format = %{
      format(:opaque)
      | framerate: {60, 1},
        storage: %DMABuf.Format{fourcc: @abgr8888, modifier: 0},
        acquire_sync: :sync_file,
        colorimetry: %Colorimetry{
          primaries: :bt709,
          transfer: :bt709,
          matrix: :bt709,
          range: :limited,
          chroma_location: :left
        },
        pixel_aspect_ratio: {4, 3}
    }

    assert {:ok, reopened} = Native.video_consumer_session_open(target_resource, stream_format)
    assert :ok = Native.video_consumer_session_close(reopened)
  end

  test "native consumer session keeps its exact target alive" do
    {owner, _dispatcher} = start_guarded_owner()
    assert {:ok, {fd, fd_resource}} = Native.video_interop_open_fd_for_test()
    parent = self()

    {holder, holder_monitor} =
      spawn_monitor(fn ->
        session = open_active_session_without_returning_target()
        :erlang.garbage_collect()
        Process.sleep(20)
        send(parent, {:session_without_target, session})
      end)

    assert_receive {:session_without_target, session}, 1_000
    assert_receive {:DOWN, ^holder_monitor, :process, ^holder, :normal}, 1_000

    assert {:ok, lease} = LeaseOwner.issue(owner, :target_retained)

    assert {:ok, :transferred} =
             Native.video_consumer_session_submit(session, canonical_frame(fd, lease))

    assert :ok = Native.video_consumer_session_close(session)
    assert_receive {:backend_released, :target_retained}, 1_000
    assert :ok = LeaseOwner.close(owner)
    assert is_reference(fd_resource)
  end

  test "native consumer session pending claim survives sender death until session close" do
    {owner, _dispatcher} = start_guarded_owner()
    assert {:ok, {fd, fd_resource}} = Native.video_interop_open_fd_for_test()
    assert {:ok, {session, target_resource}} = Native.video_consumer_session_open_for_test()
    test_pid = self()

    holder =
      spawn(fn ->
        assert {:ok, lease} = LeaseOwner.issue(owner, :pending_frame)
        result = Native.video_consumer_session_submit(session, canonical_frame(fd, lease))
        send(test_pid, {:consumer_submission, result})
        Process.sleep(:infinity)
      end)

    holder_monitor = Process.monitor(holder)
    assert_receive {:consumer_submission, {:ok, :transferred}}
    Process.exit(holder, :kill)
    assert_receive {:DOWN, ^holder_monitor, :process, ^holder, :killed}

    refute_receive {:backend_released, :pending_frame}, 20
    assert LeaseOwner.stats(owner).active_holders == 1

    assert :ok = Native.video_consumer_session_close(session)
    assert :ok = Native.video_consumer_session_close(session)
    assert_receive {:backend_released, :pending_frame}, 1_000
    eventually(fn -> LeaseOwner.stats(owner).active_holders == 0 end)
    assert LeaseOwner.stats(owner).duplicate_releases == 0
    assert :ok = LeaseOwner.close(owner)
    assert is_reference(fd_resource)
    assert is_reference(target_resource)
  end

  test "inactive canonical submissions release successfully and later reactivate" do
    {owner, _dispatcher} = start_guarded_owner()
    assert {:ok, {fd, fd_resource}} = Native.video_interop_open_fd_for_test()
    assert {:ok, {session, target_resource}} = Native.video_consumer_session_open_for_test()
    on_exit(fn -> assert :ok = Native.video_consumer_session_close(session) end)
    consumer = %VideoConsumerSession{ref: session, monitor: self()}

    assert {:ok, true} =
             Native.video_consumer_target_set_active_for_test(target_resource, false)

    assert {:ok, inactive_lease} = LeaseOwner.issue(owner, :inactive_canonical)
    assert :ok = VideoInterop.consume(consumer, canonical_frame(fd, inactive_lease))
    assert_receive {:backend_released, :inactive_canonical}, 1_000
    refute_receive {:backend_released, :inactive_canonical}, 20

    assert {:ok, {0, 1, 0}} =
             Native.video_consumer_target_pipeline_counts_for_test(target_resource)

    assert {:ok, true} =
             Native.video_consumer_target_set_active_for_test(target_resource, true)

    assert {:ok, active_lease} = LeaseOwner.issue(owner, :reactivated_canonical)
    assert :ok = VideoInterop.consume(consumer, canonical_frame(fd, active_lease))
    refute_receive {:backend_released, :reactivated_canonical}, 20

    assert {:ok, {1, 1, 1}} =
             Native.video_consumer_target_pipeline_counts_for_test(target_resource)

    assert {:ok, true} =
             Native.video_consumer_target_set_active_for_test(target_resource, false)

    assert_receive {:backend_released, :reactivated_canonical}, 1_000
    refute_receive {:backend_released, :reactivated_canonical}, 20

    assert {:ok, {1, 1, 0}} =
             Native.video_consumer_target_pipeline_counts_for_test(target_resource)

    assert :ok = Native.video_consumer_session_close(session)
    assert :ok = LeaseOwner.close(owner)
    assert is_reference(fd_resource)
    assert is_reference(target_resource)
  end

  test "invalid and stale canonical submissions remain caller-owned" do
    {owner, _dispatcher} = start_guarded_owner()
    assert {:ok, {fd, fd_resource}} = Native.video_interop_open_fd_for_test()
    assert {:ok, {session, target_resource}} = Native.video_consumer_session_open_for_test()
    on_exit(fn -> assert :ok = Native.video_consumer_session_close(session) end)

    assert {:ok, invalid_lease} = LeaseOwner.issue(owner, :invalid_preclaim)
    invalid_frame = %{canonical_frame(fd, invalid_lease) | coded_width: 63}

    assert {:error, {:caller_owned, reason}} =
             Native.video_consumer_session_submit(session, invalid_frame)

    assert reason =~ "exceeds coded size"
    refute_receive {:backend_released, :invalid_preclaim}, 20
    assert :ok = Lease.release(invalid_lease)
    assert_receive {:backend_released, :invalid_preclaim}, 1_000

    assert {:ok, true} = Native.video_consumer_target_replace_for_test(target_resource)
    assert {:ok, stale_lease} = LeaseOwner.issue(owner, :stale_preclaim)

    assert {:error, {:caller_owned, reason}} =
             Native.video_consumer_session_submit(session, canonical_frame(fd, stale_lease))

    assert reason =~ "stale video target incarnation"
    refute_receive {:backend_released, :stale_preclaim}, 20
    assert :ok = Lease.release(stale_lease)
    assert_receive {:backend_released, :stale_preclaim}, 1_000

    assert :ok = Native.video_consumer_session_close(session)
    assert LeaseOwner.stats(owner).duplicate_releases == 0
    assert :ok = LeaseOwner.close(owner)
    assert is_reference(fd_resource)
    assert is_reference(target_resource)
  end

  test "canonical submission and deactivation race transfers or drops exactly once" do
    {owner, _dispatcher} = start_guarded_owner()
    assert {:ok, {fd, fd_resource}} = Native.video_interop_open_fd_for_test()
    assert {:ok, {session, target_resource}} = Native.video_consumer_session_open_for_test()
    on_exit(fn -> assert :ok = Native.video_consumer_session_close(session) end)

    Enum.each(1..20, fn sequence ->
      assert {:ok, true} =
               Native.video_consumer_target_set_active_for_test(target_resource, true)

      token = {:inactive_race, sequence}
      assert {:ok, lease} = LeaseOwner.issue(owner, token)
      frame = canonical_frame(fd, lease)

      submitter =
        Task.async(fn ->
          receive do
            :go -> Native.video_consumer_session_submit(session, frame)
          end
        end)

      deactivator =
        Task.async(fn ->
          receive do
            :go -> Native.video_consumer_target_set_active_for_test(target_resource, false)
          end
        end)

      send(submitter.pid, :go)
      send(deactivator.pid, :go)

      assert Task.await(submitter) in [{:ok, :transferred}, {:ok, :released}]
      assert {:ok, true} = Task.await(deactivator)
      assert_receive {:backend_released, ^token}, 1_000
      refute_receive {:backend_released, ^token}, 5
      eventually(fn -> LeaseOwner.stats(owner).active_holders == 0 end)
    end)

    assert {:ok, {submitted, inactive_dropped, 0}} =
             Native.video_consumer_target_pipeline_counts_for_test(target_resource)

    assert submitted + inactive_dropped == 20
    assert :ok = Native.video_consumer_session_close(session)
    assert LeaseOwner.stats(owner).duplicate_releases == 0
    assert :ok = LeaseOwner.close(owner)
    assert is_reference(fd_resource)
    assert is_reference(target_resource)
  end

  test "consumer close timeout preserves its dispatcher root for an exact retry" do
    {owner, _dispatcher} = start_guarded_owner()
    assert {:ok, {fd, fd_resource}} = Native.video_interop_open_fd_for_test()
    assert {:ok, {session, target_resource}} = Native.video_consumer_session_open_for_test()
    assert {:ok, lease} = LeaseOwner.issue(owner, :prepared_close_timeout)

    assert {:ok, prepared} =
             Native.video_consumer_prepare_hold_for_test(session, canonical_frame(fd, lease))

    assert {:error, {:timeout, reason}} =
             Native.video_consumer_session_close_with_timeout_for_test(session, 0)

    assert reason =~ "timed out"
    assert Native.video_consumer_prepared_drop_for_test(prepared)
    refute Native.video_consumer_prepared_drop_for_test(prepared)
    assert :ok = Native.video_consumer_session_close(session)
    assert :ok = Lease.release(lease)
    assert_receive {:backend_released, :prepared_close_timeout}, 1_000
    assert :ok = LeaseOwner.close(owner)
    assert is_reference(fd_resource)
    assert is_reference(target_resource)
  end

  test "consumer close rejects preclaim and leaves the caller responsible" do
    {owner, _dispatcher} = start_guarded_owner()
    assert {:ok, {fd, fd_resource}} = Native.video_interop_open_fd_for_test()
    assert {:ok, {session, target_resource}} = Native.video_consumer_session_open_for_test()
    assert {:ok, lease} = LeaseOwner.issue(owner, :closed_preclaim)

    assert :ok = Native.video_consumer_session_close(session)

    assert {:error, {:caller_owned, "video consumer release dispatcher is closed"}} =
             Native.video_consumer_session_submit(session, canonical_frame(fd, lease))

    refute_receive {:backend_released, :closed_preclaim}, 20
    assert :ok = Lease.release(lease)
    assert_receive {:backend_released, :closed_preclaim}
    assert :ok = LeaseOwner.close(owner)
    assert is_reference(fd_resource)
    assert is_reference(target_resource)
  end

  test "consumer submission and close race either rejects preclaim or retires the claim" do
    {owner, _dispatcher} = start_guarded_owner()
    assert {:ok, {fd, fd_resource}} = Native.video_interop_open_fd_for_test()

    Enum.each(1..10, fn sequence ->
      assert {:ok, {session, target_resource}} = Native.video_consumer_session_open_for_test()
      token = {:raced_frame, sequence}
      assert {:ok, lease} = LeaseOwner.issue(owner, token)
      frame = canonical_frame(fd, lease)

      submitter =
        Task.async(fn ->
          receive do
            :go -> Native.video_consumer_session_submit(session, frame)
          end
        end)

      closer =
        Task.async(fn ->
          receive do
            :go -> Native.video_consumer_session_close(session)
          end
        end)

      send(submitter.pid, :go)
      send(closer.pid, :go)

      assert :ok = Task.await(closer)

      case Task.await(submitter) do
        {:ok, :transferred} ->
          :ok

        {:error, {:caller_owned, reason}} ->
          assert reason in [
                   "video consumer release dispatcher is closed",
                   "video-interop release dispatcher unavailable: dispatcher is Stopping"
                 ] or
                   String.starts_with?(
                     reason,
                     "video consumer stream is closed: renderer_epoch="
                   )

          assert :ok = Lease.release(lease)
      end

      assert_receive {:backend_released, ^token}, 1_000
      eventually(fn -> LeaseOwner.stats(owner).active_holders == 0 end)
      assert is_reference(target_resource)
    end)

    assert LeaseOwner.stats(owner).duplicate_releases == 0
    assert :ok = LeaseOwner.close(owner)
    assert is_reference(fd_resource)
  end

  test "explicit dispatcher close joins off-scheduler and makes a stale guard inert" do
    test_pid = self()
    token = make_ref()
    holder = make_ref()

    lifecycle =
      spawn(fn ->
        assert {:ok, dispatcher} = Native.headless_prime_release_dispatcher_new()

        assert {:ok, guard} =
                 Native.headless_prime_abandonment_guard_new(
                   test_pid,
                   token,
                   holder,
                   dispatcher
                 )

        assert Native.video_interop_abandonment_guard?(guard)
        refute Native.video_interop_abandonment_guard?(make_ref())
        refute AbandonmentGuard.valid?(AbandonmentGuard.new(make_ref(), Native))
        assert :ok = Native.headless_prime_release_dispatcher_close(dispatcher)

        assert {:error, "release dispatcher handle is closed"} =
                 Native.headless_prime_abandonment_guard_new(
                   test_pid,
                   make_ref(),
                   make_ref(),
                   dispatcher
                 )

        send(test_pid, {:dispatcher_guard_ready, self()})

        receive do
          :drop_dispatcher_and_guard -> {dispatcher, guard}
        end
      end)

    lifecycle_monitor = Process.monitor(lifecycle)
    assert_receive {:dispatcher_guard_ready, ^lifecycle}
    send(lifecycle, :drop_dispatcher_and_guard)
    assert_receive {:DOWN, ^lifecycle_monitor, :process, ^lifecycle, :normal}
    refute_receive {:video_interop_abandoned, ^token, ^holder}, 50
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

  test "consumer close uncertainty enters permanent fail-closed quarantine" do
    assert :draining = HeadlessPrimeSession.destination_close_shutdown_mode_for_test(nil)

    assert :quarantined =
             HeadlessPrimeSession.destination_close_shutdown_mode_for_test(
               {:consumer_close_failed, :timeout}
             )

    owner = self()

    state = %{
      mode: :quarantined,
      lease_owner: owner,
      destination_close_error: {:consumer_close_failed, :timeout}
    }

    assert {:noreply, ^state} =
             HeadlessPrimeSession.handle_info(
               {:video_interop_lease_owner_drained, owner},
               state
             )
  end

  test "inactive submission stays connected while terminal errors disconnect" do
    assert :keep = HeadlessPrimeSession.submission_error_action(:inactive)
    assert :disconnect = HeadlessPrimeSession.submission_error_action(:stale_target)
    assert :disconnect = HeadlessPrimeSession.submission_error_action(:invalid_frame)
  end

  defp forwarding_state(owner, destination) do
    %{
      mode: :open,
      destination: {:external, destination},
      frame_message: "emerge_skia_frame",
      lease_owner: owner
    }
  end

  defp raw_prime_frame(backend_token) do
    [
      {"backend_token", backend_token},
      {"descriptor", %Descriptor{objects: [], layers: []}},
      {"acquire_sync", :implicit},
      {"width", 64},
      {"height", 32},
      {"sequence", 1}
    ]
  end

  defp start_guarded_owner do
    assert {:ok, dispatcher} = Native.headless_prime_release_dispatcher_new()
    test_pid = self()

    owner =
      start_supervised!(
        {LeaseOwner,
         producer: self(),
         release: fn backend_token ->
           send(test_pid, {:backend_released, backend_token})
           :ok
         end,
         abandonment_guard_factory: {HeadlessPrimeSession, :new_abandonment_guard, [dispatcher]},
         max_active: 2,
         notify: self()}
      )

    on_exit(fn ->
      if Process.alive?(owner), do: assert(:ok = LeaseOwner.drain(owner, 1_000))
      assert :ok = Native.headless_prime_release_dispatcher_close(dispatcher)
    end)

    {owner, dispatcher}
  end

  defp canonical_frame(fd, lease) do
    %Frame{
      coded_width: 64,
      coded_height: 32,
      visible_rect: %Rect{x: 0, y: 0, width: 64, height: 32},
      storage: %Descriptor{
        objects: [%Object{fd: fd, size: 8_192, modifier: 0}],
        layers: [
          %Layer{
            fourcc: @abgr8888,
            planes: [%Plane{object_index: 0, offset: 0, pitch: 256}]
          }
        ]
      },
      acquire_sync: :implicit,
      lease: lease
    }
  end

  defp open_active_session_without_returning_target do
    assert {:ok, {session, target_resource}} = Native.video_consumer_session_open_for_test()

    assert {:ok, true} =
             Native.video_consumer_target_set_active_for_test(target_resource, true)

    session
  end

  defp native_thread_count(prefix) do
    "/proc/self/task/*/comm"
    |> Path.wildcard()
    |> Enum.count(fn path ->
      case File.read(path) do
        {:ok, name} -> String.starts_with?(String.trim(name), prefix)
        {:error, _reason} -> false
      end
    end)
  end

  defp eventually(assertion, attempts \\ 200)
  defp eventually(assertion, 0), do: assert(assertion.())

  defp eventually(assertion, attempts) do
    if assertion.() do
      :ok
    else
      Process.sleep(2)
      eventually(assertion, attempts - 1)
    end
  end
end
