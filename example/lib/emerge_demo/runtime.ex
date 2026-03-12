defmodule EmergeDemo.Runtime do
  @moduledoc false

  use GenServer

  require Logger

  @type state :: map()

  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @impl true
  def init(opts) do
    Process.flag(:trap_exit, true)

    config = runtime_config(opts)

    {:ok,
     %{
       config: config,
       renderer: nil,
       video_target: nil,
       diff_state: nil,
       pipeline_pid: nil,
       pipeline_supervisor: nil,
       pipeline_monitor: nil,
       status: :booting,
       last_error: nil,
       stream_format: nil
     }, {:continue, :boot}}
  end

  @impl true
  def handle_continue(:boot, state) do
    case boot_demo(state) do
      {:ok, state} ->
        schedule_renderer_check(state)
        {:noreply, state}

      {:error, reason, state} ->
        Logger.error("failed to boot Emerge demo: #{inspect(reason)}")
        stop_vm_async(1)
        {:stop, {:shutdown, reason}, cleanup(state)}
    end
  end

  @impl true
  def handle_info({:wifi_pipeline, :initialized}, state) do
    {:noreply, state |> set_status(:starting_pipeline) |> refresh_tree()}
  end

  def handle_info({:wifi_pipeline, :playing}, state) do
    {:noreply, state |> set_status(:pipeline_playing) |> refresh_tree()}
  end

  def handle_info({:wifi_pipeline, :child_terminated, child}, state) do
    {:noreply,
     state
     |> put_error("pipeline child terminated: #{inspect(child)}")
     |> refresh_tree()}
  end

  def handle_info({:video_sink, :stream_format, format}, state) do
    {:noreply,
     state
     |> Map.put(:stream_format, format)
     |> set_status(:waiting_for_frames)
     |> refresh_tree()}
  end

  def handle_info({:video_sink, :start_of_stream}, state) do
    {:noreply, state |> set_status(:waiting_for_frames) |> refresh_tree()}
  end

  def handle_info({:video_sink, :first_frame_submitted, _pts}, state) do
    {:noreply, state |> set_status(:live) |> refresh_tree()}
  end

  def handle_info({:video_sink, :submit_failed, reason}, state) do
    {:noreply,
     state
     |> set_status(:pipeline_down)
     |> put_error(reason)
     |> refresh_tree()}
  end

  def handle_info({:video_sink, :end_of_stream}, state) do
    {:noreply,
     state
     |> set_status(:pipeline_down)
     |> put_error("end of stream")
     |> refresh_tree()}
  end

  def handle_info(
        {:DOWN, ref, :process, pid, reason},
        %{pipeline_monitor: ref, pipeline_pid: pid} = state
      ) do
    {:noreply,
     state
     |> Map.put(:pipeline_monitor, nil)
     |> Map.put(:pipeline_pid, nil)
     |> Map.put(:pipeline_supervisor, nil)
     |> set_status(:pipeline_down)
     |> put_error("pipeline exited: #{inspect(reason)}")
     |> refresh_tree()}
  end

  def handle_info({:EXIT, pid, _reason}, %{pipeline_supervisor: pid} = state) do
    {:noreply, state}
  end

  def handle_info({:EXIT, pid, _reason}, %{pipeline_pid: pid} = state) do
    {:noreply, state}
  end

  def handle_info(:check_renderer, state) do
    if renderer_running?(state.renderer) do
      schedule_renderer_check(state)
      {:noreply, state}
    else
      Logger.info("renderer stopped, shutting down Emerge demo")
      cleanup(state)
      System.stop(0)
      {:noreply, state}
    end
  end

  def handle_info(message, state) do
    _ = message
    {:noreply, state}
  end

  @impl true
  def terminate(_reason, state) do
    cleanup(state)
    :ok
  end

  defp boot_demo(state) do
    renderer_opts = Keyword.fetch!(state.config, :renderer)
    video_opts = Keyword.fetch!(state.config, :video)
    pipeline_opts = Keyword.fetch!(state.config, :pipeline)

    with {:ok, renderer} <- start_renderer(renderer_opts),
         {:ok, video_target} <- EmergeSkia.video_target(renderer, video_opts),
         {:ok, state} <- upload_initial_tree(state, renderer, video_target),
         {:ok, pipeline_supervisor, pipeline_pid} <- start_pipeline(video_target, pipeline_opts) do
      pipeline_monitor = Process.monitor(pipeline_pid)

      {:ok,
       %{
         state
         | renderer: renderer,
           video_target: video_target,
           pipeline_pid: pipeline_pid,
           pipeline_supervisor: pipeline_supervisor,
           pipeline_monitor: pipeline_monitor,
           status: :starting_pipeline
       }
       |> refresh_tree()}
    else
      {:error, reason} -> {:error, reason, state}
    end
  end

  defp runtime_config(opts) do
    app_config = Application.get_env(:emerge_demo, __MODULE__, [])

    renderer =
      app_config
      |> Keyword.get(:renderer, [])
      |> Keyword.merge(Keyword.get(opts, :renderer, []))

    video =
      app_config
      |> Keyword.get(:video, [])
      |> Keyword.merge(Keyword.get(opts, :video, []))

    pipeline =
      app_config
      |> Keyword.get(:pipeline, [])
      |> Keyword.merge(Keyword.get(opts, :pipeline, []))

    [
      renderer: renderer,
      video: video,
      pipeline: pipeline,
      renderer_check_interval_ms:
        Keyword.get(
          opts,
          :renderer_check_interval_ms,
          Keyword.get(app_config, :renderer_check_interval_ms, 500)
        )
    ]
  end

  defp start_renderer(renderer_opts) do
    opts = [otp_app: :emerge_demo] ++ renderer_opts

    case EmergeSkia.start(opts) do
      {:ok, renderer} -> {:ok, renderer}
      {:error, reason} -> {:error, {:renderer_start_failed, reason}}
      other -> {:error, {:unexpected_renderer_result, other}}
    end
  end

  defp upload_initial_tree(state, renderer, video_target) do
    tree = EmergeDemo.UI.build_tree(video_target, state, state.config)
    {diff_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)
    {:ok, %{state | diff_state: diff_state}}
  rescue
    error ->
      {:error, {:initial_tree_upload_failed, Exception.message(error)}}
  end

  defp start_pipeline(video_target, pipeline_opts) do
    case Membrane.Pipeline.start_link(
           EmergeDemo.WiFiPipeline2,
           Keyword.merge(pipeline_opts, video_target: video_target, notify_to: self())
         ) do
      {:ok, pipeline_supervisor, pipeline_pid} -> {:ok, pipeline_supervisor, pipeline_pid}
      {:error, reason} -> {:error, {:pipeline_start_failed, reason}}
    end
  end

  defp refresh_tree(
         %{renderer: renderer, video_target: video_target, diff_state: diff_state} = state
       )
       when not is_nil(renderer) and not is_nil(video_target) and not is_nil(diff_state) do
    if renderer_running?(renderer) do
      tree = EmergeDemo.UI.build_tree(video_target, state, state.config)
      {diff_state, _assigned} = EmergeSkia.patch_tree(renderer, diff_state, tree)
      %{state | diff_state: diff_state}
    else
      state
    end
  rescue
    error ->
      Logger.warning("failed to refresh demo tree: #{Exception.message(error)}")
      state
  end

  defp refresh_tree(state), do: state

  defp schedule_renderer_check(state) do
    Process.send_after(
      self(),
      :check_renderer,
      Keyword.fetch!(state.config, :renderer_check_interval_ms)
    )
  end

  defp renderer_running?(nil), do: false
  defp renderer_running?(renderer), do: EmergeSkia.running?(renderer)

  defp cleanup(state) do
    if is_pid(state.pipeline_pid) and Process.alive?(state.pipeline_pid) do
      _ = Membrane.Pipeline.terminate(state.pipeline_pid, timeout: 1_000)
    end

    if state.renderer && renderer_running?(state.renderer) do
      _ = EmergeSkia.stop(state.renderer)
    end

    state
  end

  defp stop_vm_async(status) do
    spawn(fn ->
      Process.sleep(50)
      System.stop(status)
    end)
  end

  defp set_status(state, status), do: %{state | status: status}
  defp put_error(state, error), do: %{state | last_error: error}
end
