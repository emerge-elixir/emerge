defmodule EmergeSkia.Macos.Protocol do
  @moduledoc false

  import Bitwise

  @protocol_name "emerge_skia_macos"
  @protocol_version 7

  @log_level_info 1
  @log_level_warning 2
  @log_level_error 3

  @macos_backend_auto 0
  @macos_backend_metal 1
  @macos_backend_raster 2

  @asset_mode_await 0
  @asset_mode_snapshot 1

  def encode_init_payload do
    protocol_name = IO.iodata_to_binary(@protocol_name)

    <<byte_size(protocol_name)::unsigned-big-16, protocol_name::binary,
      @protocol_version::unsigned-big-16>>
  end

  def decode_init_ok_payload(payload) do
    with {:ok, {protocol_name, version, host_id, host_pid}} <- decode_init_ok_tuple(payload),
         true <- protocol_name == @protocol_name,
         true <- version == @protocol_version do
      {:ok, %{host_id: host_id, host_pid: host_pid}}
    else
      false -> {:error, "unsupported macOS host init response"}
      {:error, reason} -> {:error, reason}
    end
  end

  def encode_frame(frame_type, request_id, session_id, tag, payload) when is_binary(payload) do
    <<frame_type, request_id::unsigned-big-32, session_id::unsigned-big-64, tag::unsigned-big-16,
      payload::binary>>
  end

  def decode_frame(
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

  def decode_frame(_data), do: {:error, "invalid frame"}

  def decode_error_payload(payload) when is_binary(payload), do: payload

  def decode_log_payload(<<level_tag, source_len::unsigned-big-32, rest::binary>>)
      when byte_size(rest) >= source_len + 4 do
    <<source::binary-size(source_len), message_len::unsigned-big-32,
      message::binary-size(message_len)>> = rest

    {:ok, {decode_log_level(level_tag), source, message}}
  rescue
    MatchError -> :error
  end

  def decode_log_payload(_payload), do: :error

  def decode_key_payload(<<key_len::unsigned-big-32, rest::binary>>)
      when byte_size(rest) >= key_len + 2 do
    <<key::binary-size(key_len), action, mods_bits>> = rest
    {:ok, {:key, {String.to_atom(key), action, decode_mods(mods_bits)}}}
  rescue
    MatchError -> :error
  end

  def decode_key_payload(_payload), do: :error

  def decode_text_commit_payload(<<text_len::unsigned-big-32, rest::binary>>)
      when byte_size(rest) >= text_len + 1 do
    <<text::binary-size(text_len), mods_bits>> = rest
    {:ok, {:text_commit, {text, decode_mods(mods_bits)}}}
  rescue
    MatchError -> :error
  end

  def decode_text_commit_payload(_payload), do: :error

  def decode_text_preedit_payload(<<text_len::unsigned-big-32, rest::binary>>)
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

  def decode_text_preedit_payload(_payload), do: :error

  def decode_element_event_payload(
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

  def decode_element_event_payload(_payload), do: :error

  def decode_button(1), do: :left
  def decode_button(2), do: :right
  def decode_button(3), do: :middle
  def decode_button(_other), do: :middle

  def decode_mods(bits) when is_integer(bits) do
    []
    |> maybe_prepend((bits &&& 0x01) != 0, :shift)
    |> maybe_prepend((bits &&& 0x02) != 0, :ctrl)
    |> maybe_prepend((bits &&& 0x04) != 0, :alt)
    |> maybe_prepend((bits &&& 0x08) != 0, :meta)
    |> Enum.reverse()
  end

  def encode_start_session(
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
    asset_payload = encode_asset_config(asset_config)
    fonts = Map.fetch!(asset_config, :fonts)
    renderer_stats_log = if renderer_stats_log, do: 1, else: 0

    <<byte_size(title)::unsigned-big-32, title::binary, width::unsigned-big-32,
      height::unsigned-big-32, scroll_line_pixels::float-big-32, renderer_stats_log,
      encode_macos_backend_tag(macos_backend), asset_payload::binary,
      encode_fonts(fonts, priv_dir)::binary>>
  end

  def encode_configure_assets(asset_config) do
    encode_asset_config(asset_config)
  end

  def encode_measure_text(text, font_size) do
    text = IO.iodata_to_binary(text)
    <<byte_size(text)::unsigned-big-32, text::binary, font_size::float-big-32>>
  end

  def decode_measure_text_reply(
        <<width::float-big-32, line_height::float-big-32, ascent::float-big-32,
          descent::float-big-32>>
      ) do
    {:ok, {width, line_height, ascent, descent}}
  end

  def decode_measure_text_reply(_payload), do: :error

  def encode_load_font(family, weight, italic, data) when is_binary(data) do
    family = IO.iodata_to_binary(family)
    italic = if italic, do: 1, else: 0

    <<byte_size(family)::unsigned-big-32, family::binary, weight::unsigned-big-16, italic,
      byte_size(data)::unsigned-big-32, data::binary>>
  end

  def encode_offscreen_request(tree, raster_opts, asset_config) when is_binary(tree) do
    asset_mode = encode_asset_mode(Map.fetch!(raster_opts, :asset_mode))
    asset_timeout_ms = Map.fetch!(raster_opts, :asset_timeout_ms)
    asset_payload = encode_asset_config(asset_config)

    <<byte_size(tree)::unsigned-big-32, tree::binary,
      Map.fetch!(raster_opts, :width)::unsigned-big-32,
      Map.fetch!(raster_opts, :height)::unsigned-big-32,
      Map.fetch!(raster_opts, :scale)::float-big-32, asset_mode,
      asset_timeout_ms::unsigned-big-32, asset_payload::binary>>
  end

  def decode_binary_reply(<<len::unsigned-big-32, data::binary-size(len)>>), do: {:ok, data}
  def decode_binary_reply(_payload), do: :error

  def decode_macos_backend_tag(@macos_backend_metal), do: :metal
  def decode_macos_backend_tag(@macos_backend_raster), do: :raster

  def decode_macos_backend_tag(other) do
    raise "unexpected macOS backend tag: #{inspect(other)}"
  end

  def format_socket_error(:closed), do: "macOS host connection closed"
  def format_socket_error(reason), do: "macOS host socket error: #{inspect(reason)}"

  defp encode_asset_config(asset_config) do
    priv_dir = Map.fetch!(asset_config, :priv_dir)
    sources = [priv_dir]
    allowlist = Map.fetch!(asset_config, :runtime_allowlist)
    extensions = Map.fetch!(asset_config, :runtime_extensions)
    runtime_enabled = if Map.fetch!(asset_config, :runtime_enabled), do: 1, else: 0

    runtime_follow_symlinks =
      if Map.fetch!(asset_config, :runtime_follow_symlinks), do: 1, else: 0

    max_file_size = Map.fetch!(asset_config, :runtime_max_file_size)

    <<encode_string_list(sources)::binary, runtime_enabled, encode_string_list(allowlist)::binary,
      runtime_follow_symlinks, max_file_size::unsigned-big-64,
      encode_string_list(extensions)::binary>>
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

  defp decode_log_level(@log_level_info), do: :info
  defp decode_log_level(@log_level_warning), do: :warning
  defp decode_log_level(@log_level_error), do: :error
  defp decode_log_level(_other), do: :info

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

  defp encode_macos_backend_tag("auto"), do: @macos_backend_auto
  defp encode_macos_backend_tag("metal"), do: @macos_backend_metal
  defp encode_macos_backend_tag("raster"), do: @macos_backend_raster

  defp encode_asset_mode("await"), do: @asset_mode_await
  defp encode_asset_mode("snapshot"), do: @asset_mode_snapshot

  defp maybe_prepend(list, true, value), do: [value | list]
  defp maybe_prepend(list, false, _value), do: list
end
