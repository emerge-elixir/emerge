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
               9::unsigned-big-16>>
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
