# Invoked by RasterArtifactSmoke in an isolated VM, not directly via mix run.
[source_root, app_dir] = System.argv()
Mix.start()
Code.compile_file("mix.exs")
Application.put_env(:emerge, :compiled_backends, [])
Code.compile_file(Path.join(source_root, "lib/emerge_skia/build_config.ex"))
:code.add_patha(String.to_charlist(Path.join(app_dir, "ebin")))
false = Code.ensure_loaded?(Rustler)
Code.compile_file("lib/emerge_skia/native.ex")
tree = EmergeSkia.Native.tree_new()
true = EmergeSkia.Native.tree_is_empty(tree)

# Only artifact-specific behavior: this does not rerun the standard CI suite.
defmodule RasterArtifactProbe do
  @moduledoc false
  use Emerge.UI

  alias VideoInterop.{Binary, Frame}
  alias VideoInterop.Binary.{Format, Plane}

  def run do
    for {format, width, white} <- [
          {:bw1, 9, <<0xFF, 0x80, 0xFF, 0x80>>},
          {:gray2, 5, <<0xFF, 0xC0, 0xFF, 0xC0>>}
        ],
        dither <- [false, true] do
      renderer = start!(format, width, 2, dither)

      for {color, expected} <- [{:white, white}, {:black, <<0, 0, 0, 0>>}] do
        tree = el([width(px(width)), height(px(2)), Background.color(color)], none())
        {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)
        %Binary{planes: [%Plane{stride: 2}]} = await_frame!(format, &(&1 == expected))
      end

      :ok = EmergeSkia.stop(renderer)
      drain_frames()
    end

    # Verify registered fonts and packed text, including the dither policy path.
    renderer = start!(:gray2, 64, 32, true)

    tree =
      el(
        [
          width(px(64)),
          height(px(32)),
          Background.color(:white),
          Font.family("Probe"),
          Font.size(18),
          Font.color(:black)
        ],
        text("Raster")
      )

    {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)
    white = :binary.copy(<<255>>, 512)

    %Binary{planes: [%Plane{stride: 16}]} =
      await_frame!(
        :gray2,
        &(byte_size(&1) == 512 and &1 != white and &1 != :binary.copy(<<0>>, 512))
      )

    :ok = EmergeSkia.stop(renderer)
    IO.puts("raster probe passed")
  end

  defp start!(format, width, height, dither) do
    {:ok, renderer} =
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :headless,
        rendering_api: :raster,
        width: width,
        height: height,
        assets: [fonts: [[family: "Probe", source: "test_assets/Probe.ttf"]]],
        headless: [
          target: self(),
          pixel_format: format,
          bw1_polarity: :one_is_white,
          dither: dither
        ]
      )

    renderer
  end

  defp await_frame!(format, matches, deadline \\ System.monotonic_time(:millisecond) + 10_000) do
    remaining = Kernel.max(0, deadline - System.monotonic_time(:millisecond))

    receive do
      {:emerge_skia_frame,
       %Frame{
         format: %{storage: %Format{pixel_format: ^format}},
         storage: %Binary{data: data} = binary
       }} ->
        if matches.(data), do: binary, else: await_frame!(format, matches, deadline)
    after
      remaining -> raise "No matching #{format} frame from packaged NIF"
    end
  end

  defp drain_frames do
    receive do
      {:emerge_skia_frame, _} -> drain_frames()
    after
      0 -> :ok
    end
  end
end

RasterArtifactProbe.run()
