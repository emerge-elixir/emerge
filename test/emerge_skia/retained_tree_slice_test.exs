defmodule EmergeSkia.RetainedTreeSliceTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias Emerge.Config
  alias VideoInterop.{Binary, Frame}

  alias Emerge.TestSupport.Headless

  defp start_renderer(fps \\ nil) do
    {:ok, renderer} =
      EmergeSkia.start(%Config{
        output: %Config.Headless{target: self(), size: {180, 170}},
        renderer: [api: :raster, target_fps: fps],
        assets: [otp_app: :emerge]
      })

    on_exit(fn -> EmergeSkia.stop(renderer) end)
    renderer
  end

  defp receive_pixels do
    assert_receive {:emerge_skia_frame,
                    %Frame{coded_width: 180, coded_height: 170, storage: %Binary{data: pixels}}},
                   5_000

    assert byte_size(pixels) == 180 * 170 * 4
    pixels
  end

  defp padded(s), do: el([padding(4), Font.family(:monospace)], text(s))

  test "headless text patches draw new glyphs and capture the latest output" do
    r = start_renderer()
    {state, _} = EmergeSkia.upload_tree(r, padded("AA"))
    aa = receive_pixels()
    {state, _} = EmergeSkia.patch_tree(r, state, padded("BB"))
    bb = receive_pixels()
    refute bb == aa
    {state, _} = EmergeSkia.patch_tree(r, state, padded("BBBB"))
    wide = receive_pixels()
    refute wide == bb
    {_, _} = EmergeSkia.patch_tree(r, state, padded("BBBB"))
    # An unchanged update need not produce another frame.
    assert {:ok, ^wide} = EmergeSkia.render_to_pixels(r)
    assert {:ok, <<137, "PNG\r\n", 26, 10, _::binary>>} = EmergeSkia.render_to_png(r)
  end

  test "binary formats describe their delivered bytes; capture uses the installed raster" do
    # Use a supported single box. The separate multi-row regression also
    # requires Row layout and remains a whole-product test.
    for {format, encoded} <- [
          rgba8888: <<100, 50, 25, 128>>,
          bgra8888: <<25, 50, 100, 128>>,
          rgb888: <<100, 50, 25>>
        ] do
      {:ok, r} =
        EmergeSkia.start(%Config{
          output: %Config.Headless{
            size: {3, 2},
            target: self(),
            frames: %Config.BinaryFrames{pixel_format: format}
          },
          renderer: [api: :raster],
          assets: [otp_app: :emerge]
        })

      try do
        assert {:error, _} = EmergeSkia.render_to_pixels(r)

        EmergeSkia.upload_tree(
          r,
          el(
            [width(px(3)), height(px(2)), Background.color({:color_rgba, {200, 100, 50, 128}})],
            none()
          )
        )

        assert_receive {:emerge_skia_frame,
                        %Frame{
                          format: %{storage: %{pixel_format: ^format}},
                          storage: %Binary{data: bytes, planes: [%{stride: stride}]}
                        } = frame},
                       1500

        assert :ok = VideoInterop.validate(frame)
        assert stride == 3 * byte_size(encoded)
        assert bytes == :binary.copy(encoded, 6)
        assert {:ok, <<100, 50, 25, 128>>} = EmergeSkia.render_to_pixels(r, region: {2, 1, 1, 1})

        assert {:ok, <<100, 50, 25>>} =
                 EmergeSkia.render_to_pixels(r, pixel_format: :rgb888, region: {2, 1, 1, 1})
      after
        EmergeSkia.stop(r)
      end
    end
  end

  defp wrapped(w) do
    chip = el([width(px(50)), height(px(30)), Background.color(color_rgb(0, 0, 255))], none())

    below =
      el(
        [
          width(fill()),
          height(px(40)),
          Background.color(color_rgb(255, 0, 0)),
          Event.on_click(:below)
        ],
        none()
      )

    column([width(px(w)), spacing(10)], [
      wrapped_row([width(fill()), spacing(10)], [chip, chip, chip]),
      below
    ])
  end

  defp pixel(pixels, x, y), do: binary_part(pixels, 4 * (y * 180 + x), 4)

  defp probe(renderer, y, hit?) do
    pos = {5.0, y * 1.0}

    for event <- [
          {:pointer_move, pos},
          {:pointer_button, {:left, :press, [], pos}},
          {:pointer_button, {:left, :release, [], pos}}
        ] do
      assert :ok = EmergeSkia.send_input(renderer, event)
      assert_receive {:raw, ^event}
    end

    if hit?,
      do: assert_receive({:application, {:below, %{x: 5.0, y: 5.0}}}),
      else: refute_receive({:application, _})
  end

  test "the wrapping sibling moves in both callbacks and pixels on the same viewport" do
    {pid, r} =
      Headless.mount({180, 170}, fn -> wrapped(170) end, [:pointer_move, :pointer_button, :key])

    wide = receive_pixels()
    assert pixel(wide, 5, 45) == <<255, 0, 0, 255>>
    assert pixel(wide, 5, 125) == <<0, 0, 0, 0>>

    probe(r, 45, true)
    probe(r, 125, false)
    send(pid, {:scene, fn -> wrapped(100) end})
    narrow = receive_pixels()
    assert pixel(narrow, 5, 45) == <<0, 0, 255, 255>>
    assert pixel(narrow, 5, 125) == <<255, 0, 0, 255>>

    probe(r, 45, false)
    probe(r, 125, true)
    send(pid, {:scene, fn -> wrapped(170) end})
    assert receive_pixels() == wide
    probe(r, 45, true)
    probe(r, 125, false)

    assert :ok = Headless.button(r, :release, 5, 45)
    assert_receive {:raw, {:pointer_button, {:left, :release, [], {5.0, 45.0}}}}
    refute_receive {:application, _}

    for action <- [:press, :release] do
      key = {:key, {:enter, action, [:ctrl]}}
      assert :ok = EmergeSkia.send_input(r, key)
      assert_receive {:raw, ^key}
    end
  end

  defp box(listen?, color) do
    listeners = if listen?, do: [Event.on_click(:box)], else: []
    el([width(px(20)), height(px(20)), Background.color(color) | listeners], none())
  end

  test "a press follows its mount through paint changes but not listener removal" do
    {pid, r} = Headless.mount({180, 170}, fn -> box(true, :red) end)
    first = receive_pixels()
    :ok = Headless.button(r, :press, 5, 5)
    send(pid, {:scene, fn -> box(true, :blue) end})
    blue = receive_pixels()
    refute blue == first
    :ok = Headless.button(r, :release, 5, 5)
    assert_receive {:application, {:box, %{x: 5.0, y: 5.0}}}

    :ok = Headless.button(r, :press, 5, 5)

    for listen? <- [false, true] do
      send(pid, {:scene, fn -> box(listen?, :blue) end})
      assert receive_pixels() == blue
    end

    :ok = Headless.button(r, :release, 5, 5)
    refute_receive {:application, _}
    :ok = Headless.button(r, :press, 5, 5)
    :ok = Headless.button(r, :release, 5, 5)
    assert_receive {:application, {:box, %{x: 5.0, y: 5.0}}}
  end

  test "a pending edit does not retarget input, and a full upload cancels an old press" do
    # A normal frame cap keeps the accepted patch pending. Public renderer
    # input and capture must still agree with the first frame, not the edit.
    r = start_renderer(1)
    :ok = EmergeSkia.set_input_target(r, self())
    {state, _} = EmergeSkia.upload_tree(r, wrapped(170))
    wide = receive_pixels()
    EmergeSkia.patch_tree(r, state, wrapped(100))
    :ok = Headless.button(r, :press, 5, 45)
    :ok = Headless.button(r, :release, 5, 45)
    assert_receive {:emerge_skia_event, {id, :click, %{x: 5.0, y: 5.0}}}
    assert {:ok, ^wide} = EmergeSkia.render_to_pixels(r)
    narrow = receive_pixels()
    refute narrow == wide
    :ok = Headless.button(r, :press, 5, 125)
    EmergeSkia.upload_tree(r, wrapped(100))
    assert receive_pixels() == narrow
    :ok = Headless.button(r, :release, 5, 125)
    refute_receive {:emerge_skia_event, {_, :click, _}}
    :ok = Headless.button(r, :press, 5, 125)
    :ok = Headless.button(r, :release, 5, 125)
    assert_receive {:emerge_skia_event, {^id, :click, %{x: 5.0, y: 5.0}}}
    assert :ok = EmergeSkia.stop(r)
    assert {:error, _} = Headless.move(r, 0, 0)
  end

  test "raw subscriptions do not gate listeners, and input cannot supply a resolved click" do
    {_, r} = Headless.mount({180, 170}, fn -> wrapped(170) end, [:key])
    receive_pixels()
    assert :ok = Headless.button(r, :press, 5, 45)
    assert :ok = Headless.button(r, :release, 5, 45)
    assert_receive {:application, {:below, %{x: 5.0, y: 5.0}}}
    refute_receive {:raw, _}

    for invalid <- [
          {:click, {5, 45}},
          {:pointer_move, {1.0e300, 0}},
          {:key, {:enter, :press, [:ctrl | :invalid]}}
        ] do
      assert {:error, _} = EmergeSkia.send_input(r, invalid)
    end
  end
end
