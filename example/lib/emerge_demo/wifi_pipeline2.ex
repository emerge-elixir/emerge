defmodule EmergeDemo.WiFiPipeline2 do
  @moduledoc false

  use Membrane.Pipeline

  require Membrane.Pad

  alias Membrane.{H265, RTP}
  alias NervesWifibroadcast.Membrane.Radio.Source, as: RadioSource
  alias NervesWifibroadcast.Membrane.WFB.{Decrypt, PayloadUnwrap, ReorderFec}

  @impl true
  def handle_init(_ctx, opts) do
    notify_to = Keyword.get(opts, :notify_to)
    framerate = Keyword.get(opts, :framerate, {60, 1})
    interfaces = Keyword.get(opts, :interfaces, ["wlan0", "wlan1"])
    key_path = Keyword.fetch!(opts, :key_path)
    link_id = Keyword.get(opts, :link_id, 7_669_206)
    decoder = Keyword.get(opts, :decoder, "/dev/dri/renderD129")
    video_target = Keyword.fetch!(opts, :video_target)

    spec = [
      child(:source, %RadioSource{interfaces: interfaces, link_id: link_id, radio_port: 0})
      |> via_out(Membrane.Pad.ref(:output, 0))
      |> child(:decrypt, %Decrypt{key_path: key_path, min_epoch: 0})
      |> child(:reorder_fec, %ReorderFec{ring_size: 40})
      |> child(:payload_unwrap, PayloadUnwrap)
      |> child(:rtp_parser, %RTP.Parser{secure?: false})
      |> child(:jitter_buffer, %RTP.JitterBuffer{clock_rate: 90_000, latency: 2})
      |> child(:depayloader, RTP.H265.Depayloader)
      |> child(:video_parser, %H265.Parser{
        generate_best_effort_timestamps: %{framerate: framerate}
      })
      |> child(:video_decoder, %H265.PrimeDecoder{
        hw_device: decoder,
        decoder: :vaapi,
        output: :prime
      })
      |> child(:video_player, %EmergeDemo.VideoSink{target: video_target, notify_to: notify_to})
    ]

    notify(notify_to, {:wifi_pipeline, :initialized})
    {[spec: spec], %{notify_to: notify_to}}
  end

  @impl true
  def handle_playing(_ctx, state) do
    notify(state.notify_to, {:wifi_pipeline, :playing})
    {[], state}
  end

  @impl true
  def handle_child_terminated(child, _ctx, state) do
    notify(state.notify_to, {:wifi_pipeline, :child_terminated, child})
    {[], state}
  end

  defp notify(nil, _message), do: :ok
  defp notify(pid, message) when is_pid(pid), do: send(pid, message)
end
