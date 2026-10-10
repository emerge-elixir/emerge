defmodule EmergeSkia.Macos.HostTest do
  use ExUnit.Case, async: true

  alias Emerge.Runtime.Viewport.Renderer, as: ViewportRenderer
  alias EmergeSkia.Macos.Host
  alias EmergeSkia.Macos.Protocol
  alias EmergeSkia.Macos.Session

  test "handle_call running uses cached session state" do
    state = %{sessions: %{1 => %{running: true}}}

    assert {:reply, true, ^state} = Host.handle_call({:running, 1}, self(), state)
  end

  test "handle_call running returns false for stopped or unknown sessions" do
    state = %{sessions: %{1 => %{running: false}}}

    assert {:reply, false, ^state} = Host.handle_call({:running, 1}, self(), state)
    assert {:reply, false, ^state} = Host.handle_call({:running, 2}, self(), state)
  end

  test "set_input_target sends generic renderer heartbeat" do
    heartbeat = ViewportRenderer.heartbeat_message()

    state = %{
      sessions: %{
        1 => %{
          running: true,
          input_target: nil,
          log_target: nil,
          input_mask: 0xFF,
          input_ready: false,
          pending_resize: nil,
          pending_focus: nil,
          pending_close: false,
          pending_logs: [],
          pending_element_events: []
        }
      }
    }

    assert {:reply, :ok, _state} = Host.handle_call({:set_input_target, 1, self()}, self(), state)
    assert_receive ^heartbeat
  end

  test "protocol frame and init fixtures match wire format" do
    request_frame =
      <<3, 0x11223344::unsigned-big-32, 0x0102030405060708::unsigned-big-64,
        0x5566::unsigned-big-16, "payload">>

    assert Protocol.encode_frame(
             3,
             0x11223344,
             0x0102030405060708,
             0x5566,
             "payload"
           ) == request_frame

    assert {:ok,
            %{
              frame_type: 3,
              request_id: 0x11223344,
              session_id: 0x0102030405060708,
              tag: 0x5566,
              payload: "payload"
            }} = Protocol.decode_frame(request_frame)

    assert Protocol.encode_init_payload() ==
             <<byte_size("emerge_skia_macos")::unsigned-big-16, "emerge_skia_macos",
               17::unsigned-big-16>>
  end

  test "protocol rejects hosts without live title support" do
    reply = fn version ->
      <<byte_size("emerge_skia_macos")::unsigned-big-16, "emerge_skia_macos",
        version::unsigned-big-16, 123::unsigned-big-64, 456::unsigned-big-32>>
    end

    assert {:ok, %{host_id: 123, host_pid: 456}} = Protocol.decode_init_ok_payload(reply.(17))

    for version <- [12, 13, 14, 15, 16] do
      assert {:error, "unsupported macOS host init response"} =
               Protocol.decode_init_ok_payload(reply.(version))
    end
  end

  test "window title wire payload uses UTF-8 byte length, including empty titles" do
    assert Protocol.encode_window_title("Hello 🌍") == <<10::unsigned-big-32, "Hello 🌍">>
    assert Protocol.encode_window_title("") == <<0::unsigned-big-32>>
  end

  test "title changes reject stale host identities, closed sessions and disconnected sockets" do
    state = %{
      host_id: 11,
      sessions: %{1 => %{running: true}, 2 => %{running: false}},
      socket: nil
    }

    for {host_id, session} <- [{10, 1}, {11, 2}, {11, 3}] do
      assert {:reply, {:error, :stale_renderer}, ^state} =
               Host.handle_call({:set_window_title, host_id, session, "new"}, self(), state)
    end

    assert {:reply, {:error, :host_not_connected}, ^state} =
             Host.handle_call({:set_window_title, 11, 1, "new"}, self(), state)
  end

  test "title changes wait for a session-matched AppKit acknowledgement" do
    {state, peer} = title_socket_state()
    tag = make_ref()

    {:noreply, pending} =
      Host.handle_call({:set_window_title, 11, 1, "Hello 🌍"}, {self(), tag}, state)

    refute_receive {^tag, _}, 20
    assert {:ok, request} = :gen_tcp.recv(peer, 0, 1_000)

    assert {:ok, %{frame_type: 3, request_id: 1, session_id: 1, tag: 0x001D, payload: payload}} =
             Protocol.decode_frame(request)

    assert payload == <<10::unsigned-big-32, "Hello 🌍">>
    reply = Protocol.encode_frame(4, 1, 1, 0x001D, <<>>)
    assert {:noreply, next} = Host.handle_info({:tcp, state.socket, reply}, pending)
    assert_receive {^tag, :ok}
    assert next.pending_requests == %{}
  end

  test "title request reports native errors, malformed acknowledgements and host loss" do
    for response <- [:native_error, :wrong_session, :wrong_tag, :wrong_payload, :closed] do
      {state, _peer} = title_socket_state()
      tag = make_ref()

      {:noreply, pending} =
        Host.handle_call({:set_window_title, 11, 1, "new"}, {self(), tag}, state)

      case response do
        :closed ->
          Host.handle_info({:tcp_closed, state.socket}, pending)

        other ->
          frame =
            case other do
              :native_error -> Protocol.encode_frame(6, 1, 1, 0x001D, "unknown session")
              :wrong_session -> Protocol.encode_frame(4, 1, 2, 0x001D, <<>>)
              :wrong_tag -> Protocol.encode_frame(4, 1, 1, 0x001C, <<>>)
              :wrong_payload -> Protocol.encode_frame(4, 1, 1, 0x001D, <<1>>)
            end

          Host.handle_info({:tcp, state.socket, frame}, pending)
      end

      assert_receive {^tag, {:error, _}}
    end
  end

  defp title_socket_state do
    opts = [:binary, packet: 4, active: false, ip: {127, 0, 0, 1}]
    {:ok, listener} = :gen_tcp.listen(0, opts)
    {:ok, {address, port}} = :inet.sockname(listener)
    {:ok, socket} = :gen_tcp.connect(address, port, opts, 1_000)
    {:ok, peer} = :gen_tcp.accept(listener, 1_000)
    :gen_tcp.close(listener)

    on_exit(fn ->
      :gen_tcp.close(socket)
      :gen_tcp.close(peer)
    end)

    {%{
       host_id: 11,
       sessions: %{1 => %{running: true}},
       socket: socket,
       next_request_id: 1,
       pending_requests: %{}
     }, peer}
  end

  test "protocol decodes raw input payloads" do
    key = "enter"

    assert {:ok, {:key, {:enter, 1, [:shift, :meta]}}} =
             Protocol.decode_key_payload(
               <<byte_size(key)::unsigned-big-32, key::binary, 1, 0x09>>
             )

    assert {:ok, {:text_commit, {"hello", [:ctrl]}}} =
             Protocol.decode_text_commit_payload(<<5::unsigned-big-32, "hello", 0x02>>)

    assert {:ok, {:text_preedit, {"compose", {1, 3}}}} =
             Protocol.decode_text_preedit_payload(
               <<7::unsigned-big-32, "compose", 1, 1::unsigned-big-32, 3::unsigned-big-32>>
             )

    assert {:ok, {:text_preedit, {"compose", nil}}} =
             Protocol.decode_text_preedit_payload(<<7::unsigned-big-32, "compose", 0>>)
  end

  test "offscreen protocol carries asset cache and decode policy" do
    payload =
      Protocol.encode_offscreen_request(
        "tree",
        %{
          width: 96,
          height: 96,
          scale: 1.0,
          asset_mode: "await",
          asset_timeout_ms: 30_000
        },
        %{
          priv_dir: "/tmp/priv",
          runtime_allowlist: [],
          runtime_extensions: [".jpg"],
          runtime_enabled: false,
          runtime_follow_symlinks: false,
          runtime_max_file_size: 25_000_000,
          cache_max_entries: 17,
          cache_max_bytes: 1_048_576,
          svg_tree_max_entries: 5,
          svg_tree_max_bytes: 2_097_152,
          decode_at_size: true,
          fonts: []
        }
      )

    assert binary_part(payload, byte_size(payload) - 37, 37) ==
             <<17::unsigned-big-64, 1_048_576::unsigned-big-64, 5::unsigned-big-64,
               2_097_152::unsigned-big-64, 1, 0::unsigned-big-32>>
  end

  test "protocol decodes canonical pointer button tags" do
    assert Protocol.decode_button(1) == :left
    assert Protocol.decode_button(2) == :right
    assert Protocol.decode_button(3) == :middle
    assert Protocol.decode_button(4) == :back
    assert Protocol.decode_button(5) == :forward
    assert Protocol.decode_button(6) == :other
    assert Protocol.decode_button(255) == :other
  end

  test "protocol decodes element event payloads" do
    id = <<131, 104, 2, 100, 0, 3, 116, 111, 100, 111>>

    assert {:ok, {^id, :mouse_move, "payload"}} =
             Protocol.decode_element_event_payload(
               <<15, 1, byte_size(id)::unsigned-big-32, id::binary, 7::unsigned-big-32,
                 "payload">>
             )

    assert {:ok, {^id, :click}} =
             Protocol.decode_element_event_payload(
               <<1, 0, byte_size(id)::unsigned-big-32, id::binary, 0::unsigned-big-32>>
             )
  end

  test "session buffers resize and focus until input is ready" do
    state = %{sessions: %{1 => Session.base_state(0xFF)}}

    state =
      state
      |> Session.buffer_resize(1, 640, 480, 2.0, 0xFF)
      |> Session.buffer_focus(1, true, 0xFF)
      |> Session.flush(1, 0x40, 0x80)

    refute_receive {:emerge_skia_event, _}

    state =
      state
      |> Session.update_metadata(1, :input_target, self(), 0xFF)
      |> Session.update_metadata(1, :input_ready, true, 0xFF)
      |> Session.flush(1, 0x40, 0x80)

    assert_receive {:emerge_skia_event, {:resized, {640, 480, 2.0}}}
    assert_receive {:emerge_skia_event, {:focused, true}}
    assert %{sessions: %{1 => %{pending_resize: nil, pending_focus: nil}}} = state
  end

  test "session buffers element events until input target exists" do
    state =
      %{sessions: %{1 => Session.base_state(0xFF)}}
      |> Session.buffer_element_event(1, {"todo", :mouse_enter}, 0xFF)
      |> Session.flush(1, 0x40, 0x80)

    refute_receive {:emerge_skia_event, _}

    state =
      state
      |> Session.update_metadata(1, :input_target, self(), 0xFF)
      |> Session.flush(1, 0x40, 0x80)

    assert_receive {:emerge_skia_event, {"todo", :mouse_enter}}
    assert %{sessions: %{1 => %{pending_element_events: []}}} = state
  end
end
