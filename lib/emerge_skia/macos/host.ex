defmodule EmergeSkia.Macos.Host do
  @moduledoc false

  use GenServer
  alias EmergeSkia.Macos.Renderer
  alias EmergeSkia.Macos.{Launcher, Protocol, Session}
  alias Emerge.Runtime.Viewport.Renderer, as: ViewportRenderer

  @name __MODULE__
  @connect_retries 100
  @connect_retry_ms 50

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
  @request_measure_text 0x0017
  @request_load_font 0x0018
  @request_configure_assets 0x0019
  @request_render_tree_to_pixels 0x001A
  @request_render_tree_to_png 0x001B

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

  @spec measure_text(String.t(), float()) ::
          {float(), float(), float(), float()} | no_return()
  def measure_text(text, font_size) when is_binary(text) and is_float(font_size) do
    with :ok <- ensure_started() do
      case GenServer.call(@name, {:measure_text, text, font_size}, 15_000) do
        {:ok, metrics} -> metrics
        {:error, reason} -> raise "measure_text failed: #{reason}"
      end
    end
  end

  @spec load_font(String.t(), non_neg_integer(), boolean(), binary()) :: :ok | {:error, term()}
  def load_font(family, weight, italic, data)
      when is_binary(family) and is_integer(weight) and is_boolean(italic) and is_binary(data) do
    with :ok <- ensure_started() do
      GenServer.call(@name, {:load_font, family, weight, italic, data}, 15_000)
    end
  end

  @spec configure_assets(Renderer.t(), map()) :: :ok | {:error, term()}
  def configure_assets(%Renderer{} = renderer, asset_config) when is_map(asset_config) do
    with :ok <- ensure_started() do
      GenServer.call(@name, {:configure_assets, renderer.session_id, asset_config}, 15_000)
    end
  end

  @spec render_tree_to_pixels(binary(), map(), map()) :: binary() | {:error, term()}
  def render_tree_to_pixels(bytes, raster_opts, asset_config)
      when is_binary(bytes) and is_map(raster_opts) and is_map(asset_config) do
    with :ok <- ensure_started() do
      case GenServer.call(
             @name,
             {:render_tree_to_pixels, bytes, raster_opts, asset_config},
             30_000
           ) do
        {:ok, binary} -> binary
        {:error, reason} -> {:error, reason}
      end
    end
  end

  @spec render_tree_to_png(binary(), map(), map()) :: binary() | {:error, term()}
  def render_tree_to_png(bytes, raster_opts, asset_config)
      when is_binary(bytes) and is_map(raster_opts) and is_map(asset_config) do
    with :ok <- ensure_started() do
      case GenServer.call(@name, {:render_tree_to_png, bytes, raster_opts, asset_config}, 30_000) do
        {:ok, binary} -> binary
        {:error, reason} -> {:error, reason}
      end
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
           Protocol.encode_start_session(
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
      {:error, _reason} -> {:reply, :ok, Session.mark_stopped(state, session_id, @input_mask_all)}
    end
  end

  def handle_call({:running, session_id}, from, state) do
    _ = from
    {:reply, Session.running?(state, session_id), state}
  end

  def handle_call({:set_input_target, session_id, pid}, _from, state) do
    state =
      state
      |> Session.update_metadata(session_id, :input_target, pid, @input_mask_all)
      |> Session.flush(session_id, @input_mask_resize, @input_mask_focus)

    if is_pid(pid) do
      send(pid, ViewportRenderer.heartbeat_message())
    end

    {:reply, :ok, state}
  end

  def handle_call({:set_log_target, session_id, pid}, _from, state) do
    {:reply, :ok,
     state
     |> Session.update_metadata(session_id, :log_target, pid, @input_mask_all)
     |> Session.flush(session_id, @input_mask_resize, @input_mask_focus)}
  end

  def handle_call({:set_input_mask, session_id, mask}, _from, state) do
    state =
      state
      |> Session.update_metadata(session_id, :input_mask, mask, @input_mask_all)
      |> Session.update_metadata(session_id, :input_ready, true, @input_mask_all)
      |> Session.flush(session_id, @input_mask_resize, @input_mask_focus)

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

  def handle_call({:measure_text, text, font_size}, from, state) do
    case queue_request(
           state,
           from,
           {:measure_text},
           0,
           @request_measure_text,
           Protocol.encode_measure_text(text, font_size)
         ) do
      {:ok, state} -> {:noreply, state}
      {:error, reason} -> {:reply, {:error, reason}, state}
    end
  end

  def handle_call({:load_font, family, weight, italic, data}, from, state) do
    case queue_request(
           state,
           from,
           {:load_font},
           0,
           @request_load_font,
           Protocol.encode_load_font(family, weight, italic, data)
         ) do
      {:ok, state} -> {:noreply, state}
      {:error, reason} -> {:reply, {:error, reason}, state}
    end
  end

  def handle_call({:configure_assets, session_id, asset_config}, from, state) do
    case queue_request(
           state,
           from,
           {:configure_assets, session_id},
           session_id,
           @request_configure_assets,
           Protocol.encode_configure_assets(asset_config)
         ) do
      {:ok, state} -> {:noreply, state}
      {:error, reason} -> {:reply, {:error, reason}, state}
    end
  end

  def handle_call({:render_tree_to_pixels, bytes, raster_opts, asset_config}, from, state) do
    case queue_request(
           state,
           from,
           {:render_tree_to_pixels},
           0,
           @request_render_tree_to_pixels,
           Protocol.encode_offscreen_request(bytes, raster_opts, asset_config)
         ) do
      {:ok, state} -> {:noreply, state}
      {:error, reason} -> {:reply, {:error, reason}, state}
    end
  end

  def handle_call({:render_tree_to_png, bytes, raster_opts, asset_config}, from, state) do
    case queue_request(
           state,
           from,
           {:render_tree_to_png},
           0,
           @request_render_tree_to_png,
           Protocol.encode_offscreen_request(bytes, raster_opts, asset_config)
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
    fail_pending_requests(state, Protocol.format_socket_error(reason))
    {:stop, :normal, %{state | socket: nil}}
  end

  @impl true
  def terminate(_reason, state) do
    if state.launched? and is_port(state.socket) do
      _ =
        :gen_tcp.send(
          state.socket,
          Protocol.encode_frame(@frame_request, 0, 0, @request_shutdown_host, <<>>)
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

  defp prepare_host_launch, do: Launcher.prepare()

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
    init_payload = Protocol.encode_init_payload()

    with :ok <- :gen_tcp.send(socket, Protocol.encode_frame(@frame_init, 0, 0, 0, init_payload)),
         {:ok, response} <- :gen_tcp.recv(socket, 0, 10_000),
         {:ok, frame} <- Protocol.decode_frame(response) do
      case frame do
        %{frame_type: @frame_init_ok, payload: payload} ->
          Protocol.decode_init_ok_payload(payload)

        %{frame_type: @frame_error, payload: payload} ->
          {:error, Protocol.decode_error_payload(payload)}

        other ->
          {:error, "unexpected init response: #{inspect(other)}"}
      end
    else
      {:error, reason} -> {:error, Protocol.format_socket_error(reason)}
    end
  end

  defp queue_request(state, from, request, session_id, tag, payload)
       when is_port(state.socket) and is_binary(payload) do
    request_id = state.next_request_id

    case :gen_tcp.send(
           state.socket,
           Protocol.encode_frame(@frame_request, request_id, session_id, tag, payload)
         ) do
      :ok ->
        {:ok,
         %{
           state
           | next_request_id: request_id + 1,
             pending_requests: Map.put(state.pending_requests, request_id, {request, from})
         }}

      {:error, reason} ->
        {:error, Protocol.format_socket_error(reason)}
    end
  end

  defp handle_socket_frame(data, state) do
    case Protocol.decode_frame(data) do
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
        selected_backend = Protocol.decode_macos_backend_tag(backend_tag)

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
            %{Session.base_state(@input_mask_all) | running: true},
            &Map.put(&1, :running, true)
          )

        GenServer.reply(from, {:ok, renderer})

        state
        |> Map.put(:sessions, sessions)
        |> Session.flush(session_id, @input_mask_resize, @input_mask_focus)

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
    Session.mark_stopped(state, session_id, @input_mask_all)
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
    Session.update_metadata(state, session_id, :running, running?, @input_mask_all)
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
    |> Session.update_metadata(session_id, :input_ready, true, @input_mask_all)
    |> Session.flush(session_id, @input_mask_resize, @input_mask_focus)
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
    |> Session.update_metadata(session_id, :input_ready, true, @input_mask_all)
    |> Session.flush(session_id, @input_mask_resize, @input_mask_focus)
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

  defp handle_reply_request({:measure_text}, from, 0, @request_measure_text, payload, state) do
    case Protocol.decode_measure_text_reply(payload) do
      {:ok, metrics} -> GenServer.reply(from, {:ok, metrics})
      :error -> GenServer.reply(from, {:error, "invalid measure_text reply payload"})
    end

    state
  end

  defp handle_reply_request({:load_font}, from, 0, @request_load_font, <<>>, state) do
    GenServer.reply(from, :ok)
    state
  end

  defp handle_reply_request(
         {:configure_assets, _session_id},
         from,
         _reply_session_id,
         @request_configure_assets,
         <<>>,
         state
       ) do
    GenServer.reply(from, :ok)
    state
  end

  defp handle_reply_request(
         {:render_tree_to_pixels},
         from,
         0,
         @request_render_tree_to_pixels,
         payload,
         state
       ) do
    case Protocol.decode_binary_reply(payload) do
      {:ok, binary} -> GenServer.reply(from, {:ok, binary})
      :error -> GenServer.reply(from, {:error, "invalid render_tree_to_pixels reply payload"})
    end

    state
  end

  defp handle_reply_request(
         {:render_tree_to_png},
         from,
         0,
         @request_render_tree_to_png,
         payload,
         state
       ) do
    case Protocol.decode_binary_reply(payload) do
      {:ok, binary} -> GenServer.reply(from, {:ok, binary})
      :error -> GenServer.reply(from, {:error, "invalid render_tree_to_png reply payload"})
    end

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
    message = Protocol.decode_error_payload(payload)

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
        |> Session.buffer_resize(session_id, width, height, scale_factor, @input_mask_all)
        |> Session.flush(session_id, @input_mask_resize, @input_mask_focus)

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
    |> Session.buffer_focus(session_id, focused == 1, @input_mask_all)
    |> Session.flush(session_id, @input_mask_resize, @input_mask_focus)
  end

  defp handle_notify_frame(%{session_id: session_id, tag: @notify_close_requested}, state) do
    state
    |> Session.buffer_close(session_id, @input_mask_all)
    |> Session.flush(session_id, @input_mask_resize, @input_mask_focus)
  end

  defp handle_notify_frame(%{session_id: session_id, tag: @notify_log, payload: payload}, state) do
    case Protocol.decode_log_payload(payload) do
      {:ok, log} ->
        state
        |> Session.buffer_log(session_id, log, @input_mask_all)
        |> Session.flush(session_id, @input_mask_resize, @input_mask_focus)

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

        Session.maybe_dispatch_input(
          state,
          session_id,
          @input_mask_cursor_pos,
          {:cursor_pos, {x, y}}
        )

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

        Session.maybe_dispatch_input(
          state,
          session_id,
          @input_mask_cursor_button,
          {:cursor_button,
           {Protocol.decode_button(button_tag), action, Protocol.decode_mods(mods_bits), {x, y}}}
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

        Session.maybe_dispatch_input(
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
    Session.maybe_dispatch_input(
      state,
      session_id,
      @input_mask_cursor_enter,
      {:cursor_entered, entered == 1}
    )
  end

  defp handle_notify_frame(%{session_id: session_id, tag: @notify_key, payload: payload}, state) do
    case Protocol.decode_key_payload(payload) do
      {:ok, event} -> Session.maybe_dispatch_input(state, session_id, @input_mask_key, event)
      :error -> state
    end
  end

  defp handle_notify_frame(
         %{session_id: session_id, tag: @notify_text_commit, payload: payload},
         state
       ) do
    case Protocol.decode_text_commit_payload(payload) do
      {:ok, event} ->
        Session.maybe_dispatch_input(state, session_id, @input_mask_codepoint, event)

      :error ->
        state
    end
  end

  defp handle_notify_frame(
         %{session_id: session_id, tag: @notify_text_preedit, payload: payload},
         state
       ) do
    case Protocol.decode_text_preedit_payload(payload) do
      {:ok, event} ->
        Session.maybe_dispatch_input(state, session_id, @input_mask_codepoint, event)

      :error ->
        state
    end
  end

  defp handle_notify_frame(
         %{session_id: session_id, tag: @notify_text_preedit_clear, payload: <<>>},
         state
       ) do
    Session.maybe_dispatch_input(state, session_id, @input_mask_codepoint, :text_preedit_clear)
  end

  defp handle_notify_frame(%{session_id: session_id, tag: @notify_running, payload: <<>>}, state) do
    Session.maybe_dispatch_running(state, session_id)
  end

  defp handle_notify_frame(
         %{session_id: session_id, tag: @notify_element_event, payload: payload},
         state
       ) do
    case Protocol.decode_element_event_payload(payload) do
      {:ok, event} ->
        state
        |> Session.buffer_element_event(session_id, event, @input_mask_all)
        |> Session.flush(session_id, @input_mask_resize, @input_mask_focus)

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

  defp reply_pending_error({:measure_text}, from, message),
    do: GenServer.reply(from, {:error, message})

  defp reply_pending_error({:load_font}, from, message),
    do: GenServer.reply(from, {:error, message})

  defp reply_pending_error({:configure_assets, _session_id}, from, message),
    do: GenServer.reply(from, {:error, message})

  defp reply_pending_error({:render_tree_to_pixels}, from, message),
    do: GenServer.reply(from, {:error, message})

  defp reply_pending_error({:render_tree_to_png}, from, message),
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
end
