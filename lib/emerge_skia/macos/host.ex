defmodule EmergeSkia.Macos.Host do
  @moduledoc false

  use GenServer
  import Bitwise

  alias EmergeSkia.Macos.Renderer

  @name __MODULE__
  @protocol_name "emerge_skia_macos"
  @protocol_version 7
  @connect_retries 100
  @connect_retry_ms 50
  @host_socket_env "EMERGE_SKIA_MACOS_HOST_SOCKET"
  @host_binary_env "EMERGE_SKIA_MACOS_HOST_BINARY"

  @frame_init 1
  @frame_init_ok 2
  @frame_request 3
  @frame_reply 4
  @frame_notify 5
  @frame_error 6

  @request_start_session 0x0010
  @request_stop_session 0x0011
  @request_session_running 0x0012
  @request_upload_tree 0x0013
  @request_patch_tree 0x0014
  @request_shutdown_host 0x0015
  @request_set_input_mask 0x0016

  @notify_resized 0x0100
  @notify_focused 0x0101
  @notify_close_requested 0x0102
  @notify_log 0x0103
  @notify_cursor_pos 0x0104
  @notify_cursor_button 0x0105
  @notify_cursor_scroll 0x0106
  @notify_cursor_entered 0x0107
  @notify_key 0x0108
  @notify_text_commit 0x0109
  @notify_element_event 0x010A
  @notify_text_preedit 0x010B
  @notify_text_preedit_clear 0x010C
  @notify_running 0x010D

  @log_level_info 1
  @log_level_warning 2
  @log_level_error 3
  @macos_backend_auto 0
  @macos_backend_metal 1
  @macos_backend_raster 2
  @input_mask_key 0x01
  @input_mask_codepoint 0x02
  @input_mask_resize 0x40
  @input_mask_focus 0x80
  @input_mask_cursor_pos 0x04
  @input_mask_cursor_button 0x08
  @input_mask_cursor_scroll 0x10
  @input_mask_cursor_enter 0x20
  @input_mask_all 0xFF

  @type state :: %{
          socket: port(),
          socket_path: binary(),
          host_id: non_neg_integer(),
          host_pid: non_neg_integer(),
          port: port() | nil,
          launched?: boolean(),
          sessions: %{optional(pos_integer()) => map()},
          next_request_id: non_neg_integer(),
          pending_requests: %{optional(non_neg_integer()) => term()}
        }

  @spec start_session(map(), map()) :: {:ok, Renderer.t()} | {:error, term()}
  def start_session(native_opts, asset_config)
      when is_map(native_opts) and is_map(asset_config) do
    with :ok <- ensure_started() do
      GenServer.call(@name, {:start_session, native_opts, asset_config}, 15_000)
    end
  end

  @spec stop_session(Renderer.t()) :: :ok
  def stop_session(%Renderer{} = renderer) do
    case ensure_started() do
      :ok -> GenServer.call(@name, {:stop_session, renderer.session_id}, 5_000)
      {:error, _reason} -> :ok
    end
  end

  @spec running?(Renderer.t()) :: boolean()
  def running?(%Renderer{} = renderer) do
    case GenServer.whereis(@name) do
      nil ->
        false

      pid ->
        try do
          GenServer.call(pid, {:running, renderer.session_id}, 5_000)
        catch
          :exit, _reason -> false
        end
    end
  end

  @spec set_input_target(Renderer.t(), pid() | nil) :: :ok
  def set_input_target(%Renderer{} = renderer, pid) when is_pid(pid) or is_nil(pid) do
    case ensure_started() do
      :ok -> GenServer.call(@name, {:set_input_target, renderer.session_id, pid}, 5_000)
      {:error, _reason} -> :ok
    end
  end

  @spec set_log_target(Renderer.t(), pid() | nil) :: :ok
  def set_log_target(%Renderer{} = renderer, pid) when is_pid(pid) or is_nil(pid) do
    case ensure_started() do
      :ok -> GenServer.call(@name, {:set_log_target, renderer.session_id, pid}, 5_000)
      {:error, _reason} -> :ok
    end
  end

  @spec set_input_mask(Renderer.t(), non_neg_integer()) :: :ok
  def set_input_mask(%Renderer{} = renderer, mask) when is_integer(mask) and mask >= 0 do
    case ensure_started() do
      :ok -> GenServer.call(@name, {:set_input_mask, renderer.session_id, mask}, 5_000)
      {:error, _reason} -> :ok
    end
  end

  @spec upload_tree(Renderer.t(), binary()) :: :ok | {:error, term()}
  def upload_tree(%Renderer{} = renderer, bytes) when is_binary(bytes) do
    with :ok <- ensure_started() do
      GenServer.call(@name, {:upload_tree, renderer.session_id, bytes}, 15_000)
    end
  end

  @spec patch_tree(Renderer.t(), binary()) :: :ok | {:error, term()}
  def patch_tree(%Renderer{} = renderer, bytes) when is_binary(bytes) do
    with :ok <- ensure_started() do
      GenServer.call(@name, {:patch_tree, renderer.session_id, bytes}, 15_000)
    end
  end

  @spec shutdown() :: :ok
  def shutdown do
    case GenServer.whereis(@name) do
      nil ->
        :ok

      _pid ->
        GenServer.stop(@name, :normal, 5_000)
    end
  end

  @spec ensure_started() :: :ok | {:error, term()}
  def ensure_started do
    case GenServer.whereis(@name) do
      nil ->
        case GenServer.start_link(__MODULE__, %{}, name: @name) do
          {:ok, _pid} -> :ok
          {:error, {:already_started, _pid}} -> :ok
          {:error, reason} -> {:error, reason}
        end

      _pid ->
        :ok
    end
  end

  @impl true
  def init(%{}) do
    with {:ok, launch} <- prepare_host_launch(),
         {:ok, socket} <- connect_socket(launch.socket_path),
         {:ok, %{host_id: host_id, host_pid: host_pid}} <- init_handshake(socket),
         :ok <- :inet.setopts(socket, active: :once) do
      {:ok,
       %{
         socket: socket,
         socket_path: launch.socket_path,
         host_id: host_id,
         host_pid: host_pid,
         port: launch.port,
         launched?: launch.launched?,
         sessions: %{},
         next_request_id: 1,
         pending_requests: %{}
       }}
    else
      {:error, reason} -> {:stop, reason}
    end
  end

  @impl true
  def handle_call({:start_session, native_opts, asset_config}, from, state) do
    title = Map.fetch!(native_opts, :title)
    width = Map.fetch!(native_opts, :width)
    height = Map.fetch!(native_opts, :height)
    scroll_line_pixels = Map.fetch!(native_opts, :scroll_line_pixels)
    renderer_stats_log = Map.fetch!(native_opts, :renderer_stats_log)
    macos_backend = Map.fetch!(native_opts, :macos_backend)

    case queue_request(
           state,
           from,
           {:start_session, native_opts},
           0,
           @request_start_session,
           encode_start_session(
             title,
             width,
             height,
             scroll_line_pixels,
             renderer_stats_log,
             macos_backend,
             asset_config
           )
         ) do
      {:ok, state} ->
        {:noreply, state}

      {:error, reason} ->
        {:reply, {:error, reason}, state}
    end
  end

  def handle_call({:stop_session, session_id}, from, state) do
    case queue_request(
           state,
           from,
           {:stop_session, session_id},
           session_id,
           @request_stop_session,
           <<>>
         ) do
      {:ok, state} -> {:noreply, state}
      {:error, _reason} -> {:reply, :ok, mark_session_stopped(state, session_id)}
    end
  end

  def handle_call({:running, session_id}, from, state) do
    _ = from
    {:reply, session_running?(state, session_id), state}
  end

  def handle_call({:set_input_target, session_id, pid}, _from, state) do
    state =
      state
      |> update_session_metadata(session_id, :input_target, pid)
      |> flush_buffered_session(session_id)

    if is_pid(pid) do
      send(pid, {:emerge_skia_running, :heartbeat})
    end

    {:reply, :ok, state}
  end

  def handle_call({:set_log_target, session_id, pid}, _from, state) do
    {:reply, :ok,
     state
     |> update_session_metadata(session_id, :log_target, pid)
     |> flush_buffered_session(session_id)}
  end

  def handle_call({:set_input_mask, session_id, mask}, _from, state) do
    state =
      state
      |> update_session_metadata(session_id, :input_mask, mask)
      |> update_session_metadata(session_id, :input_ready, true)
      |> flush_buffered_session(session_id)

    case queue_request(
           state,
           nil,
           {:set_input_mask, session_id},
           session_id,
           @request_set_input_mask,
           <<mask::unsigned-big-32>>
         ) do
      {:ok, state} -> {:reply, :ok, state}
      {:error, _reason} -> {:reply, :ok, state}
    end
  end

  def handle_call({:upload_tree, session_id, bytes}, from, state) do
    case queue_request(
           state,
           from,
           {:upload_tree, session_id},
           session_id,
           @request_upload_tree,
           bytes
         ) do
      {:ok, state} -> {:noreply, state}
      {:error, reason} -> {:reply, {:error, reason}, state}
    end
  end

  def handle_call({:patch_tree, session_id, bytes}, from, state) do
    case queue_request(
           state,
           from,
           {:patch_tree, session_id},
           session_id,
           @request_patch_tree,
           bytes
         ) do
      {:ok, state} -> {:noreply, state}
      {:error, reason} -> {:reply, {:error, reason}, state}
    end
  end

  @impl true
  def handle_info({_port, {:exit_status, _status}}, state) do
    {:noreply, %{state | port: nil}}
  end

  def handle_info({_port, {:data, _data}}, state) do
    {:noreply, state}
  end

  def handle_info({:tcp, socket, data}, %{socket: socket} = state) do
    state = handle_socket_frame(data, state)

    case :inet.setopts(socket, active: :once) do
      :ok -> :ok
      {:error, _reason} -> :ok
    end

    {:noreply, state}
  end

  def handle_info({:tcp_closed, socket}, %{socket: socket} = state) do
    fail_pending_requests(state, "macOS host socket closed")
    {:stop, :normal, %{state | socket: nil}}
  end

  def handle_info({:tcp_error, socket, reason}, %{socket: socket} = state) do
    fail_pending_requests(state, format_socket_error(reason))
    {:stop, :normal, %{state | socket: nil}}
  end

  @impl true
  def terminate(_reason, state) do
    if state.launched? and is_port(state.socket) do
      _ =
        :gen_tcp.send(
          state.socket,
          encode_frame(@frame_request, 0, 0, @request_shutdown_host, <<>>)
        )
    end

    if is_port(state.socket) do
      _ = :gen_tcp.close(state.socket)
    end

    if is_port(state.port) do
      try do
        Port.close(state.port)
      rescue
        ArgumentError -> :ok
      end
    end

    if state.launched? do
      _ = File.rm(state.socket_path)
    end

    :ok
  end

  defp update_session_metadata(state, session_id, key, value) do
    sessions =
      Map.update(
        state.sessions,
        session_id,
        Map.put(base_session_state(), key, value),
        &Map.put(&1, key, value)
      )

    %{state | sessions: sessions}
  end

  defp prepare_host_launch do
    case System.get_env(@host_socket_env) do
      socket_path when is_binary(socket_path) and socket_path != "" ->
        {:ok, %{socket_path: socket_path, port: nil, launched?: false}}

      _ ->
        socket_path = default_socket_path()
        _ = File.rm(socket_path)

        with :ok <- ensure_host_binary_built(),
             {:ok, port} <- launch_host(socket_path) do
          {:ok, %{socket_path: socket_path, port: port, launched?: true}}
        end
    end
  end

  defp ensure_host_binary_built do
    host_binary = host_binary_path()

    if File.regular?(host_binary) do
      :ok
    else
      build_host_binary(host_binary)
    end
  end

  defp build_host_binary(host_binary) do
    cargo = System.find_executable("cargo")
    mise = System.find_executable("mise") || "/usr/local/bin/mise"
    project_root = project_root()

    {command, args} =
      if cargo do
        {cargo,
         [
           "build",
           "--manifest-path",
           Path.join(project_root, "native/emerge_skia/Cargo.toml"),
           "--bin",
           "macos_host",
           "--no-default-features",
           "--features",
           "macos"
         ]}
      else
        {mise,
         [
           "x",
           "--",
           "cargo",
           "build",
           "--manifest-path",
           Path.join(project_root, "native/emerge_skia/Cargo.toml"),
           "--bin",
           "macos_host",
           "--no-default-features",
           "--features",
           "macos"
         ]}
      end

    case System.cmd(command, args, stderr_to_stdout: true) do
      {_output, 0} ->
        if File.regular?(host_binary) do
          :ok
        else
          {:error, "macOS host build succeeded but binary was not found at #{host_binary}"}
        end

      {output, status} ->
        {:error, "failed to build macOS host (status #{status}):\n#{output}"}
    end
  end

  defp launch_host(socket_path) do
    host_binary = host_binary_path()

    port =
      Port.open(
        {:spawn_executable, host_binary},
        [
          :binary,
          :exit_status,
          :use_stdio,
          :hide,
          args: ["--socket", socket_path, "--monitor-stdin"]
        ]
      )

    {:ok, port}
  rescue
    error -> {:error, "failed to launch macOS host: #{Exception.message(error)}"}
  end

  defp connect_socket(socket_path) do
    connect_socket(socket_path, @connect_retries)
  end

  defp connect_socket(_socket_path, 0), do: {:error, "timed out connecting to macOS host socket"}

  defp connect_socket(socket_path, attempts_left) do
    case :gen_tcp.connect({:local, String.to_charlist(socket_path)}, 0, [
           :binary,
           active: false,
           packet: 4
         ]) do
      {:ok, socket} ->
        {:ok, socket}

      {:error, _reason} ->
        Process.sleep(@connect_retry_ms)
        connect_socket(socket_path, attempts_left - 1)
    end
  end

  defp init_handshake(socket) do
    init_payload = encode_init_payload()

    with :ok <- :gen_tcp.send(socket, encode_frame(@frame_init, 0, 0, 0, init_payload)),
         {:ok, response} <- :gen_tcp.recv(socket, 0, 10_000),
         {:ok, frame} <- decode_frame(response) do
      case frame do
        %{frame_type: @frame_init_ok, payload: payload} ->
          decode_init_ok_payload(payload)

        %{frame_type: @frame_error, payload: payload} ->
          {:error, decode_error_payload(payload)}

        other ->
          {:error, "unexpected init response: #{inspect(other)}"}
      end
    else
      {:error, reason} -> {:error, format_socket_error(reason)}
    end
  end

  defp queue_request(state, from, request, session_id, tag, payload)
       when is_port(state.socket) and is_binary(payload) do
    request_id = state.next_request_id

    case :gen_tcp.send(
           state.socket,
           encode_frame(@frame_request, request_id, session_id, tag, payload)
         ) do
      :ok ->
        {:ok,
         %{
           state
           | next_request_id: request_id + 1,
             pending_requests: Map.put(state.pending_requests, request_id, {request, from})
         }}

      {:error, reason} ->
        {:error, format_socket_error(reason)}
    end
  end

  defp handle_socket_frame(data, state) do
    case decode_frame(data) do
      {:ok, %{frame_type: @frame_reply} = frame} ->
        handle_reply_frame(frame, state)

      {:ok, %{frame_type: @frame_notify} = frame} ->
        handle_notify_frame(frame, state)

      {:ok, %{frame_type: @frame_error} = frame} ->
        handle_error_frame(frame, state)

      {:ok, _other} ->
        state

      {:error, _reason} ->
        state
    end
  end

  defp handle_reply_frame(
         %{request_id: request_id, session_id: session_id, tag: tag, payload: payload},
         state
       ) do
    case Map.pop(state.pending_requests, request_id) do
      {{request, from}, pending_requests} ->
        state = %{state | pending_requests: pending_requests}
        handle_reply_request(request, from, session_id, tag, payload, state)

      {nil, _pending_requests} ->
        state
    end
  end

  defp handle_reply_request(
         {:start_session, _native_opts},
         from,
         session_id,
         @request_start_session,
         payload,
         state
       ) do
    case payload do
      <<backend_tag>> ->
        selected_backend = decode_macos_backend_tag(backend_tag)

        renderer = %Renderer{
          session_id: session_id,
          host_id: state.host_id,
          host_pid: state.host_pid,
          macos_backend: selected_backend
        }

        sessions =
          Map.update(
            state.sessions,
            session_id,
            %{base_session_state() | running: true},
            &Map.put(&1, :running, true)
          )

        GenServer.reply(from, {:ok, renderer})

        state
        |> Map.put(:sessions, sessions)
        |> flush_buffered_session(session_id)

      _ ->
        GenServer.reply(from, {:error, "invalid start_session reply payload"})
        state
    end
  end

  defp handle_reply_request(
         {:stop_session, session_id},
         from,
         _reply_session_id,
         @request_stop_session,
         <<>>,
         state
       ) do
    GenServer.reply(from, :ok)
    mark_session_stopped(state, session_id)
  end

  defp handle_reply_request(
         {:running, session_id},
         from,
         _reply_session_id,
         @request_session_running,
         <<running_flag>>,
         state
       )
       when running_flag in 0..1 do
    running? = running_flag == 1
    GenServer.reply(from, running?)
    update_session_metadata(state, session_id, :running, running?)
  end

  defp handle_reply_request(
         {:upload_tree, session_id},
         from,
         _reply_session_id,
         @request_upload_tree,
         <<>>,
         state
       ) do
    GenServer.reply(from, :ok)

    state
    |> update_session_metadata(session_id, :input_ready, true)
    |> flush_buffered_session(session_id)
  end

  defp handle_reply_request(
         {:patch_tree, session_id},
         from,
         _reply_session_id,
         @request_patch_tree,
         <<>>,
         state
       ) do
    GenServer.reply(from, :ok)

    state
    |> update_session_metadata(session_id, :input_ready, true)
    |> flush_buffered_session(session_id)
  end

  defp handle_reply_request(
         {:set_input_mask, _session_id},
         _from,
         _reply_session_id,
         @request_set_input_mask,
         <<>>,
         state
       ) do
    state
  end

  defp handle_reply_request(
         {_request, _session_id},
         from,
         _reply_session_id,
         _tag,
         _payload,
         state
       ) do
    GenServer.reply(from, {:error, "unexpected macOS host reply"})
    state
  end

  defp handle_error_frame(%{request_id: request_id, payload: payload}, state) do
    message = decode_error_payload(payload)

    case Map.pop(state.pending_requests, request_id) do
      {{request, from}, pending_requests} ->
        state = %{state | pending_requests: pending_requests}
        reply_pending_error(request, from, message)
        state

      {nil, _pending_requests} ->
        state
    end
  end

  defp handle_notify_frame(
         %{session_id: session_id, tag: @notify_resized, payload: payload},
         state
       ) do
    case payload do
      <<width::unsigned-big-32, height::unsigned-big-32, scale_bits::unsigned-big-32>> ->
        <<scale_factor::float-32>> = <<scale_bits::unsigned-big-32>>

        state
        |> buffer_resize(session_id, width, height, scale_factor)
        |> flush_buffered_session(session_id)

      _ ->
        state
    end
  end

  defp handle_notify_frame(
         %{session_id: session_id, tag: @notify_focused, payload: <<focused>>},
         state
       )
       when focused in 0..1 do
    state
    |> buffer_focus(session_id, focused == 1)
    |> flush_buffered_session(session_id)
  end

  defp handle_notify_frame(%{session_id: session_id, tag: @notify_close_requested}, state) do
    state
    |> buffer_close(session_id)
    |> flush_buffered_session(session_id)
  end

  defp handle_notify_frame(%{session_id: session_id, tag: @notify_log, payload: payload}, state) do
    case decode_log_payload(payload) do
      {:ok, log} ->
        state
        |> buffer_log(session_id, log)
        |> flush_buffered_session(session_id)

      :error ->
        state
    end
  end

  defp handle_notify_frame(
         %{session_id: session_id, tag: @notify_cursor_pos, payload: payload},
         state
       ) do
    case payload do
      <<x_bits::unsigned-big-32, y_bits::unsigned-big-32>> ->
        <<x::float-32>> = <<x_bits::unsigned-big-32>>
        <<y::float-32>> = <<y_bits::unsigned-big-32>>
        maybe_dispatch_input(state, session_id, @input_mask_cursor_pos, {:cursor_pos, {x, y}})

      _ ->
        state
    end
  end

  defp handle_notify_frame(
         %{session_id: session_id, tag: @notify_cursor_button, payload: payload},
         state
       ) do
    case payload do
      <<button_tag, action, mods_bits, x_bits::unsigned-big-32, y_bits::unsigned-big-32>> ->
        <<x::float-32>> = <<x_bits::unsigned-big-32>>
        <<y::float-32>> = <<y_bits::unsigned-big-32>>

        maybe_dispatch_input(
          state,
          session_id,
          @input_mask_cursor_button,
          {:cursor_button, {decode_button(button_tag), action, decode_mods(mods_bits), {x, y}}}
        )

      _ ->
        state
    end
  end

  defp handle_notify_frame(
         %{session_id: session_id, tag: @notify_cursor_scroll, payload: payload},
         state
       ) do
    case payload do
      <<dx_bits::unsigned-big-32, dy_bits::unsigned-big-32, x_bits::unsigned-big-32,
        y_bits::unsigned-big-32>> ->
        <<dx::float-32>> = <<dx_bits::unsigned-big-32>>
        <<dy::float-32>> = <<dy_bits::unsigned-big-32>>
        <<x::float-32>> = <<x_bits::unsigned-big-32>>
        <<y::float-32>> = <<y_bits::unsigned-big-32>>

        maybe_dispatch_input(
          state,
          session_id,
          @input_mask_cursor_scroll,
          {:cursor_scroll, {{dx, dy}, {x, y}}}
        )

      _ ->
        state
    end
  end

  defp handle_notify_frame(
         %{session_id: session_id, tag: @notify_cursor_entered, payload: <<entered>>},
         state
       )
       when entered in 0..1 do
    maybe_dispatch_input(
      state,
      session_id,
      @input_mask_cursor_enter,
      {:cursor_entered, entered == 1}
    )
  end

  defp handle_notify_frame(%{session_id: session_id, tag: @notify_key, payload: payload}, state) do
    case decode_key_payload(payload) do
      {:ok, event} -> maybe_dispatch_input(state, session_id, @input_mask_key, event)
      :error -> state
    end
  end

  defp handle_notify_frame(
         %{session_id: session_id, tag: @notify_text_commit, payload: payload},
         state
       ) do
    case decode_text_commit_payload(payload) do
      {:ok, event} -> maybe_dispatch_input(state, session_id, @input_mask_codepoint, event)
      :error -> state
    end
  end

  defp handle_notify_frame(
         %{session_id: session_id, tag: @notify_text_preedit, payload: payload},
         state
       ) do
    case decode_text_preedit_payload(payload) do
      {:ok, event} -> maybe_dispatch_input(state, session_id, @input_mask_codepoint, event)
      :error -> state
    end
  end

  defp handle_notify_frame(
         %{session_id: session_id, tag: @notify_text_preedit_clear, payload: <<>>},
         state
       ) do
    maybe_dispatch_input(state, session_id, @input_mask_codepoint, :text_preedit_clear)
  end

  defp handle_notify_frame(%{session_id: session_id, tag: @notify_running, payload: <<>>}, state) do
    maybe_dispatch_running(state, session_id)
  end

  defp handle_notify_frame(
         %{session_id: session_id, tag: @notify_element_event, payload: payload},
         state
       ) do
    case decode_element_event_payload(payload) do
      {:ok, event} ->
        state
        |> buffer_element_event(session_id, event)
        |> flush_buffered_session(session_id)

      :error ->
        state
    end
  end

  defp handle_notify_frame(_frame, state), do: state

  defp reply_pending_error({:start_session, _native_opts}, from, message),
    do: GenServer.reply(from, {:error, message})

  defp reply_pending_error({:upload_tree, _session_id}, from, message),
    do: GenServer.reply(from, {:error, message})

  defp reply_pending_error({:patch_tree, _session_id}, from, message),
    do: GenServer.reply(from, {:error, message})

  defp reply_pending_error({:set_input_mask, _session_id}, _from, _message), do: :ok

  defp reply_pending_error({:running, _session_id}, from, _message),
    do: GenServer.reply(from, false)

  defp reply_pending_error({:stop_session, _session_id}, from, _message),
    do: GenServer.reply(from, :ok)

  defp fail_pending_requests(state, message) do
    Enum.each(state.pending_requests, fn {_request_id, {request, from}} ->
      reply_pending_error(request, from, message)
    end)
  end

  defp flush_buffered_session(state, session_id) do
    case Map.fetch(state.sessions, session_id) do
      {:ok, session} ->
        {state, session} = flush_buffered_logs(state, session_id, session)
        {state, session} = flush_buffered_close(state, session_id, session)
        {state, session} = flush_buffered_element_events(state, session_id, session)
        {state, session} = flush_buffered_resize(state, session_id, session)
        {state, session} = flush_buffered_focus(state, session_id, session)
        put_session(state, session_id, session)

      :error ->
        state
    end
  end

  defp flush_buffered_logs(state, _session_id, %{log_target: nil} = session), do: {state, session}

  defp flush_buffered_logs(
         state,
         _session_id,
         %{log_target: log_target, pending_logs: logs} = session
       ) do
    Enum.each(logs, fn {level, source, message} ->
      send(log_target, {:emerge_skia_log, level, source, message})
    end)

    {state, %{session | pending_logs: []}}
  end

  defp flush_buffered_close(state, _session_id, %{input_target: nil} = session),
    do: {state, session}

  defp flush_buffered_close(
         state,
         _session_id,
         %{input_target: input_target, pending_close: true} = session
       ) do
    send(input_target, {:emerge_skia_close, :window_close_requested})
    {state, %{session | pending_close: false}}
  end

  defp flush_buffered_close(state, _session_id, session), do: {state, session}

  defp flush_buffered_element_events(state, _session_id, %{input_target: nil} = session),
    do: {state, session}

  defp flush_buffered_element_events(
         state,
         _session_id,
         %{input_target: input_target, pending_element_events: events} = session
       ) do
    Enum.each(events, &send(input_target, {:emerge_skia_event, &1}))
    {state, %{session | pending_element_events: []}}
  end

  defp flush_buffered_resize(state, _session_id, %{input_target: nil} = session),
    do: {state, session}

  defp flush_buffered_resize(state, _session_id, %{input_ready: false} = session),
    do: {state, session}

  defp flush_buffered_resize(state, _session_id, %{pending_resize: nil} = session),
    do: {state, session}

  defp flush_buffered_resize(
         state,
         _session_id,
         %{
           input_target: input_target,
           input_mask: mask,
           pending_resize: {width, height, scale_factor}
         } = session
       ) do
    if (mask &&& @input_mask_resize) != 0 do
      send(input_target, {:emerge_skia_event, {:resized, {width, height, scale_factor}}})
    end

    {state, %{session | pending_resize: nil}}
  end

  defp flush_buffered_focus(state, _session_id, %{input_target: nil} = session),
    do: {state, session}

  defp flush_buffered_focus(state, _session_id, %{input_ready: false} = session),
    do: {state, session}

  defp flush_buffered_focus(state, _session_id, %{pending_focus: nil} = session),
    do: {state, session}

  defp flush_buffered_focus(
         state,
         _session_id,
         %{input_target: input_target, input_mask: mask, pending_focus: focused} = session
       ) do
    if (mask &&& @input_mask_focus) != 0 do
      send(input_target, {:emerge_skia_event, {:focused, focused}})
    end

    {state, %{session | pending_focus: nil}}
  end

  defp buffer_resize(state, session_id, width, height, scale_factor) do
    update_session(state, session_id, fn session ->
      %{session | pending_resize: {width, height, scale_factor}}
    end)
  end

  defp buffer_focus(state, session_id, focused) do
    update_session(state, session_id, fn session ->
      %{session | pending_focus: focused}
    end)
  end

  defp buffer_close(state, session_id) do
    state
    |> mark_session_stopped(session_id)
    |> update_session(session_id, fn session -> %{session | pending_close: true} end)
  end

  defp buffer_log(state, session_id, {level, source, message}) do
    update_session(state, session_id, fn session ->
      %{session | pending_logs: session.pending_logs ++ [{level, source, message}]}
    end)
  end

  defp buffer_element_event(state, session_id, event) do
    update_session(state, session_id, fn session ->
      %{session | pending_element_events: session.pending_element_events ++ [event]}
    end)
  end

  defp maybe_dispatch_input(state, session_id, mask_bit, event) do
    case Map.fetch(state.sessions, session_id) do
      {:ok, %{input_target: pid, input_ready: true, input_mask: mask}} when is_pid(pid) ->
        if (mask &&& mask_bit) != 0 do
          send(pid, {:emerge_skia_event, event})
        end

        state

      _ ->
        state
    end
  end

  defp maybe_dispatch_running(state, session_id) do
    case Map.fetch(state.sessions, session_id) do
      {:ok, %{input_target: pid}} when is_pid(pid) ->
        send(pid, {:emerge_skia_running, :heartbeat})
        state

      _ ->
        state
    end
  end

  defp put_session(state, session_id, session) do
    %{state | sessions: Map.put(state.sessions, session_id, session)}
  end

  defp update_session(state, session_id, fun) when is_function(fun, 1) do
    sessions =
      Map.update(
        state.sessions,
        session_id,
        fun.(base_session_state()),
        fun
      )

    %{state | sessions: sessions}
  end

  defp mark_session_stopped(state, session_id) do
    update_session(state, session_id, &Map.put(&1, :running, false))
  end

  defp session_running?(state, session_id) do
    case Map.fetch(state.sessions, session_id) do
      {:ok, %{running: running?}} -> running?
      :error -> false
    end
  end

  defp encode_init_payload do
    protocol_name = IO.iodata_to_binary(@protocol_name)

    <<byte_size(protocol_name)::unsigned-big-16, protocol_name::binary,
      @protocol_version::unsigned-big-16>>
  end

  defp decode_init_ok_payload(payload) do
    with {:ok, {protocol_name, version, host_id, host_pid}} <- decode_init_ok_tuple(payload),
         true <- protocol_name == @protocol_name,
         true <- version == @protocol_version do
      {:ok, %{host_id: host_id, host_pid: host_pid}}
    else
      false -> {:error, "unsupported macOS host init response"}
      {:error, reason} -> {:error, reason}
    end
  end

  defp decode_init_ok_tuple(<<name_len::unsigned-big-16, rest::binary>>)
       when byte_size(rest) >= name_len + 2 + 8 + 4 do
    <<protocol_name::binary-size(name_len), version::unsigned-big-16, host_id::unsigned-big-64,
      host_pid::unsigned-big-32>> = rest

    {:ok, {protocol_name, version, host_id, host_pid}}
  rescue
    MatchError -> {:error, "invalid init_ok payload"}
  end

  defp decode_init_ok_tuple(_payload), do: {:error, "invalid init_ok payload"}

  defp encode_frame(frame_type, request_id, session_id, tag, payload) when is_binary(payload) do
    <<frame_type, request_id::unsigned-big-32, session_id::unsigned-big-64, tag::unsigned-big-16,
      payload::binary>>
  end

  defp decode_frame(
         <<frame_type, request_id::unsigned-big-32, session_id::unsigned-big-64,
           tag::unsigned-big-16, payload::binary>>
       ) do
    {:ok,
     %{
       frame_type: frame_type,
       request_id: request_id,
       session_id: session_id,
       tag: tag,
       payload: payload
     }}
  end

  defp decode_frame(_data), do: {:error, "invalid frame"}

  defp decode_error_payload(payload) when is_binary(payload), do: payload

  defp decode_log_payload(<<level_tag, source_len::unsigned-big-32, rest::binary>>)
       when byte_size(rest) >= source_len + 4 do
    <<source::binary-size(source_len), message_len::unsigned-big-32,
      message::binary-size(message_len)>> = rest

    {:ok, {decode_log_level(level_tag), source, message}}
  rescue
    MatchError -> :error
  end

  defp decode_log_payload(_payload), do: :error

  defp decode_log_level(@log_level_info), do: :info
  defp decode_log_level(@log_level_warning), do: :warning
  defp decode_log_level(@log_level_error), do: :error
  defp decode_log_level(_other), do: :info

  defp decode_key_payload(<<key_len::unsigned-big-32, rest::binary>>)
       when byte_size(rest) >= key_len + 2 do
    <<key::binary-size(key_len), action, mods_bits>> = rest
    {:ok, {:key, {String.to_atom(key), action, decode_mods(mods_bits)}}}
  rescue
    MatchError -> :error
  end

  defp decode_key_payload(_payload), do: :error

  defp decode_text_commit_payload(<<text_len::unsigned-big-32, rest::binary>>)
       when byte_size(rest) >= text_len + 1 do
    <<text::binary-size(text_len), mods_bits>> = rest
    {:ok, {:text_commit, {text, decode_mods(mods_bits)}}}
  rescue
    MatchError -> :error
  end

  defp decode_text_commit_payload(_payload), do: :error

  defp decode_text_preedit_payload(<<text_len::unsigned-big-32, rest::binary>>)
       when byte_size(rest) >= text_len + 1 do
    <<text::binary-size(text_len), has_cursor, cursor_rest::binary>> = rest

    case {has_cursor, cursor_rest} do
      {0, <<>>} ->
        {:ok, {:text_preedit, {text, nil}}}

      {1, <<start::unsigned-big-32, ending::unsigned-big-32>>} ->
        {:ok, {:text_preedit, {text, {start, ending}}}}

      _ ->
        :error
    end
  rescue
    MatchError -> :error
  end

  defp decode_text_preedit_payload(_payload), do: :error

  defp decode_element_event_payload(
         <<kind_tag, has_payload, id_len::unsigned-big-32, rest::binary>>
       )
       when byte_size(rest) >= id_len + 4 do
    <<id::binary-size(id_len), payload_len::unsigned-big-32, payload::binary-size(payload_len)>> =
      rest

    event =
      case has_payload do
        0 -> {id, decode_element_event_kind(kind_tag)}
        1 -> {id, decode_element_event_kind(kind_tag), payload}
      end

    {:ok, event}
  rescue
    MatchError -> :error
  end

  defp decode_element_event_payload(_payload), do: :error

  defp decode_button(1), do: :left
  defp decode_button(2), do: :right
  defp decode_button(3), do: :middle
  defp decode_button(_other), do: :middle

  defp decode_mods(bits) when is_integer(bits) do
    []
    |> maybe_prepend((bits &&& 0x01) != 0, :shift)
    |> maybe_prepend((bits &&& 0x02) != 0, :ctrl)
    |> maybe_prepend((bits &&& 0x04) != 0, :alt)
    |> maybe_prepend((bits &&& 0x08) != 0, :meta)
    |> Enum.reverse()
  end

  defp decode_element_event_kind(1), do: :click
  defp decode_element_event_kind(2), do: :press
  defp decode_element_event_kind(3), do: :swipe_up
  defp decode_element_event_kind(4), do: :swipe_down
  defp decode_element_event_kind(5), do: :swipe_left
  defp decode_element_event_kind(6), do: :swipe_right
  defp decode_element_event_kind(7), do: :key_down
  defp decode_element_event_kind(8), do: :key_up
  defp decode_element_event_kind(9), do: :key_press
  defp decode_element_event_kind(10), do: :virtual_key_hold
  defp decode_element_event_kind(11), do: :mouse_down
  defp decode_element_event_kind(12), do: :mouse_up
  defp decode_element_event_kind(13), do: :mouse_enter
  defp decode_element_event_kind(14), do: :mouse_leave
  defp decode_element_event_kind(15), do: :mouse_move
  defp decode_element_event_kind(16), do: :focus
  defp decode_element_event_kind(17), do: :blur
  defp decode_element_event_kind(18), do: :change
  defp decode_element_event_kind(_other), do: :press

  defp maybe_prepend(list, true, value), do: [value | list]
  defp maybe_prepend(list, false, _value), do: list

  defp encode_start_session(
         title,
         width,
         height,
         scroll_line_pixels,
         renderer_stats_log,
         macos_backend,
         asset_config
       ) do
    title = IO.iodata_to_binary(title)
    priv_dir = Map.fetch!(asset_config, :priv_dir)
    sources = [priv_dir]
    allowlist = Map.fetch!(asset_config, :runtime_allowlist)
    extensions = Map.fetch!(asset_config, :runtime_extensions)
    fonts = Map.fetch!(asset_config, :fonts)
    runtime_enabled = if Map.fetch!(asset_config, :runtime_enabled), do: 1, else: 0

    runtime_follow_symlinks =
      if Map.fetch!(asset_config, :runtime_follow_symlinks), do: 1, else: 0

    max_file_size = Map.fetch!(asset_config, :runtime_max_file_size)
    renderer_stats_log = if renderer_stats_log, do: 1, else: 0

    <<byte_size(title)::unsigned-big-32, title::binary, width::unsigned-big-32,
      height::unsigned-big-32, scroll_line_pixels::float-big-32, renderer_stats_log,
      encode_macos_backend_tag(macos_backend), encode_string_list(sources)::binary,
      runtime_enabled, encode_string_list(allowlist)::binary, runtime_follow_symlinks,
      max_file_size::unsigned-big-64, encode_string_list(extensions)::binary,
      encode_fonts(fonts, priv_dir)::binary>>
  end

  defp encode_string_list(list) when is_list(list) do
    encoded = Enum.map(list, &encode_string/1)
    IO.iodata_to_binary([<<length(list)::unsigned-big-32>>, encoded])
  end

  defp encode_fonts(fonts, priv_dir) when is_list(fonts) and is_binary(priv_dir) do
    encoded =
      Enum.map(fonts, fn font ->
        family = Map.fetch!(font, :family)
        path = Path.join(priv_dir, Map.fetch!(font, :source))
        weight = Map.fetch!(font, :weight)
        italic = if Map.fetch!(font, :italic), do: 1, else: 0

        [encode_string(family), encode_string(path), <<weight::unsigned-big-16, italic>>]
      end)

    IO.iodata_to_binary([<<length(fonts)::unsigned-big-32>>, encoded])
  end

  defp encode_string(value) when is_binary(value) do
    <<byte_size(value)::unsigned-big-32, value::binary>>
  end

  defp host_binary_path do
    case System.get_env(@host_binary_env) do
      path when is_binary(path) and path != "" ->
        path

      _ ->
        priv_binary = Path.join(project_root(), "priv/native/macos_host")

        if File.regular?(priv_binary) do
          priv_binary
        else
          Path.join(project_root(), "native/emerge_skia/target/debug/macos_host")
        end
    end
  end

  defp project_root do
    Path.expand("../../..", __DIR__)
  end

  defp default_socket_path do
    Path.join(System.tmp_dir!(), "emerge_skia_macos_#{System.unique_integer([:positive])}.sock")
  end

  defp format_socket_error(:closed), do: "macOS host connection closed"
  defp format_socket_error(reason), do: "macOS host socket error: #{inspect(reason)}"

  defp encode_macos_backend_tag("auto"), do: @macos_backend_auto
  defp encode_macos_backend_tag("metal"), do: @macos_backend_metal
  defp encode_macos_backend_tag("raster"), do: @macos_backend_raster

  defp decode_macos_backend_tag(@macos_backend_metal), do: :metal
  defp decode_macos_backend_tag(@macos_backend_raster), do: :raster

  defp decode_macos_backend_tag(other) do
    raise "unexpected macOS backend tag: #{inspect(other)}"
  end

  defp base_session_state do
    %{
      running: false,
      input_target: nil,
      log_target: nil,
      input_mask: @input_mask_all,
      input_ready: false,
      pending_resize: nil,
      pending_focus: nil,
      pending_close: false,
      pending_logs: [],
      pending_element_events: []
    }
  end
end
