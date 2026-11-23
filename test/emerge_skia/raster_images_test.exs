defmodule EmergeSkia.RasterImagesTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  use Emerge.Assets.Path, otp_app: :emerge
  alias Emerge.TestSupport.{BoxAssertions, Headless}

  @photo ~m"sample_assets/static.jpg"
  @turned ~m"test_assets/turn_right.jpeg"
  @wide ~m"test_assets/single_axis_wide.png"
  @grid "test_assets/alpha_grid.png"
  @inverse "test_assets/alpha_inverse.png"
  @size {320, 180}

  defp render(tree, {w, h}, opts \\ []) do
    EmergeSkia.TreeRenderer.render_to_pixels(
      tree,
      [otp_app: :emerge, width: w, height: h] ++ opts,
      3000
    )
  end

  defp pixel(bytes, width, x, y), do: binary_part(bytes, 4 * (y * width + x), 4)

  defp scene(w, src, opacity) do
    column([width(px(w))], [
      image([key(:media), width(fill()), Transform.alpha(opacity), Event.on_click(:image)], src),
      el(
        [
          key(:sibling),
          width(px(12)),
          height(px(9)),
          Background.color(:blue),
          Event.on_click(:sibling)
        ],
        none()
      )
    ])
  end

  test "PNG resolution wakes an idle viewport; each resize, source swap and undo matches cold pixels and hits" do
    steps = [
      {80, @wide, 1, 40},
      {300, @wide, 1, 150},
      {80, @wide, 1, 40},
      {80, @grid, 0.5, 48},
      {80, @inverse, 0.5, 48},
      {80, @inverse, 0, 48},
      {80, @wide, 1, 40}
    ]

    {pid, r} = Headless.mount(@size, fn -> scene(80, @wide, 1) end, [:pointer_button])

    for {{w, src, alpha, y}, i} <- Enum.with_index(steps) do
      build = fn -> scene(w, src, alpha) end
      expected = render(build.(), @size)
      if i > 0, do: send(pid, {:scene, build})
      assert Headless.pixels_matching(&(&1 == expected)) == expected
      assert pixel(expected, 320, 4, y) == <<0, 0, 255, 255>>
      if src == @wide, do: assert(pixel(expected, 320, w - 1, y - 1) == <<255, 0, 0, 255>>)
      BoxAssertions.click(r, {4.0, y + 3.0}, :sibling, {4.0, 3.0})
      BoxAssertions.click(r, {4.0, 3.0}, :image, {4.0, 3.0})
    end
  end

  test "progressive and EXIF-rotated JPEGs resize, swap with PNGs and hit at the submitted size" do
    size = {320, 230}

    steps = [
      {160, @photo, 105},
      {320, @photo, 210},
      {160, @photo, 105},
      {160, "sample_assets/fallback.jpg", 105},
      {160, @wide, 80},
      {8, @turned, 16},
      {160, @photo, 105}
    ]

    {pid, r} = Headless.mount(size, fn -> scene(160, @photo, 1) end, [:pointer_button])

    for {{w, src, y}, i} <- Enum.with_index(steps) do
      build = fn -> scene(w, src, 1) end
      # This viewport uses the live (full-resolution) default. One-shot
      # exports default to fitted decode, so opt out for this comparison.
      expected = render(build.(), size, assets: [decode_at_size: false])
      if i > 0, do: send(pid, {:scene, build})
      assert Headless.pixels_matching(&(&1 == expected)) == expected
      assert pixel(expected, 320, 4, y) == <<0, 0, 255, 255>>
      BoxAssertions.click(r, {4.0, y + 3.0}, :sibling, {4.0, 3.0})
    end
  end

  test "a fitted thumbnail grows to the native photo and reuses it on shrink" do
    size = {644, 434}
    small = fn -> scene(160, @photo, 1) end
    large = fn -> scene(640, @photo, 1) end
    fitted = render(small.(), size)
    assert fitted == render(small.(), size, assets: [decode_at_size: true])
    from_large = render(small.(), size, assets: [decode_at_size: false])
    refute fitted == from_large

    {pid, r} = Headless.mount(size, small, [:pointer_button], decode_at_size: true)
    assert Headless.pixels_matching(&(&1 == fitted)) == fitted
    BoxAssertions.click(r, {4.0, 108.0}, :sibling, {4.0, 3.0})

    full = render(large.(), size)
    send(pid, {:scene, large})
    assert Headless.pixels_matching(&(&1 == full)) == full
    BoxAssertions.click(r, {4.0, 423.0}, :sibling, {4.0, 3.0})

    send(pid, {:scene, small})
    assert Headless.pixels_matching(&(&1 == from_large)) == from_large
    BoxAssertions.click(r, {4.0, 108.0}, :sibling, {4.0, 3.0})
  end

  test "a fractional cover crop samples the same edge texels after sized decode and a quarter turn" do
    tree =
      el(
        [width(fill()), height(fill()), Background.color(:white), padding_xy(3, 2)],
        image([width(px(3)), height(px(4)), image_fit(:cover)], @turned)
      )

    # Rotate the six-by-five painted area of main's unrotated 4×3 cover
    # control, including its sampling bleed. The JPEG's 3/8 decode is 6×3.
    crop = ~w(
      30CA76FF30A678FF30707DFF303A75FF31166FFF
      4FCA81FF4EA67CFF4E7174FF4E3B75FF4E1776FF
      6CC98DFF6CA680FF6B716DFF6B3B73FF6B1777FF
      86C97FFF85A575FF857067FF853A70FF851676FF
      A2CA76FFA2A671FFA27069FFA13A78FFA11782FF
      B7CA71FFB8A66FFFB96F6CFFB73A80FFB6178EFF
    )

    white = fn n -> :binary.copy(<<255>>, n * 4) end
    rows = Enum.map(crop, &[white.(2), Base.decode16!(&1), white.(3)])
    assert render(tree, {10, 9}) == IO.iodata_to_binary([white.(10), rows, white.(20)])
  end

  test "intrinsic and single-axis PNG sizes apply the ratio inside the border box" do
    for {axes, w, h} <- [
          {[], 200, 100},
          {[width(px(70))], 70, 35},
          {[height(px(30))], 60, 30},
          {[width(px(80)), padding_xy(6, 3), Border.width(2)], 80, 42}
        ] do
      blue = el([width(px(4)), height(px(4)), Background.color(:blue)], none())
      scene = row([], [column([width(content())], [image(axes, @wide), blue]), blue])
      bytes = render(scene, @size)
      assert pixel(bytes, 320, w, 1) == <<0, 0, 255, 255>>
      assert pixel(bytes, 320, 1, h) == <<0, 0, 255, 255>>
    end
  end

  test "half-alpha texels retain their coverage; group opacity is applied after the image and background overlap" do
    image_at = fn attrs -> image([width(px(10)), height(px(6)) | attrs], @grid) end
    bytes = render(image_at.([Transform.alpha(0.5)]), {10, 6})
    assert pixel(bytes, 10, 2, 2) == <<64, 16, 0, 64>>
    assert pixel(bytes, 10, 6, 2) == <<0, 32, 128, 128>>
    assert pixel(bytes, 10, 0, 2) == <<0, 0, 0, 0>>
    picture = image_at.([])

    group = fn alpha ->
      el([Transform.alpha(alpha), Background.color(:red), padding(2)], picture)
    end

    solid = render(group.(1), {14, 10})
    composited = render(group.(0.5), {14, 10})
    assert composited == for(<<channel <- solid>>, into: <<>>, do: <<div(channel + 1, 2)>>)
    assert pixel(composited, 14, 0, 0) == <<128, 0, 0, 128>>
    contain = render(image([width(px(10)), height(px(10)), image_fit(:contain)], @grid), {10, 10})
    cover = render(image([width(px(10)), height(px(10)), image_fit(:cover)], @grid), {10, 10})
    refute contain == cover
    assert pixel(contain, 10, 2, 1) == <<0, 0, 0, 0>>
    refute pixel(cover, 10, 2, 1) == <<0, 0, 0, 0>>
  end

  test "a PNG may bleed at its own host edge, never through an outer column's clip" do
    build = fn outer ->
      column([width(px(outer))], [column([width(px(80))], [image([width(fill())], @wide)])])
    end

    {pid, _r} = Headless.mount(@size, fn -> build.(80) end, [])

    for {stage, outer, expected_edge} <- [
          {:mount, 80, <<0, 0, 0, 0>>},
          {:edit, 81, <<255, 0, 0, 255>>},
          {:undo, 80, <<0, 0, 0, 0>>}
        ] do
      if stage != :mount, do: send(pid, {:scene, fn -> build.(outer) end})
      bytes = render(build.(outer), @size)
      assert pixel(bytes, 320, 80, 10) == expected_edge
      assert Headless.pixels_matching(&(&1 == bytes)) == bytes
    end
  end

  test "snapshot does not wait for a PNG; await distinguishes a failed file from a ready image" do
    axes = [width(px(40)), height(px(24))]
    missing = image(axes, "test_assets/missing.png")
    ready = image(axes, @wide)
    pending = render(ready, {40, 24}, asset_mode: :snapshot)
    assert render(missing, {40, 24}, asset_mode: :snapshot) == pending
    failure = render(missing, {40, 24})
    assert pixel(failure, 40, 0, 0) == <<254, 242, 242, 255>>
    refute failure == pending
    assert pixel(render(ready, {40, 24}), 40, 20, 12) == <<255, 0, 0, 255>>
  end

  @tag :tmp_dir
  test "a logical name belongs to one renderer's app, even when both are running", %{
    tmp_dir: root
  } do
    for {app, file} <- [{:emerge_png_root_a, @wide.path}, {:emerge_png_root_b, @inverse}] do
      dir = Path.join(root, "#{app}-1.0.0")
      ebin = Path.join(dir, "ebin")
      File.mkdir_p!(ebin)
      File.mkdir_p!(Path.join(dir, "priv"))

      File.cp!(
        Application.app_dir(:emerge, Path.join("priv", file)),
        Path.join(dir, "priv/name.png")
      )

      true = Code.prepend_path(ebin)
      on_exit(fn -> Code.delete_path(ebin) end)
    end

    renderers =
      for app <- [:emerge_png_root_a, :emerge_png_root_b] do
        {:ok, r} =
          EmergeSkia.start(%Emerge.Config{
            output: %Emerge.Config.Headless{target: self(), size: {10, 6}, frame_message: app},
            renderer: [api: :raster],
            assets: [otp_app: app, cache: [max_entries: 0, max_bytes: 0]]
          })

        on_exit(fn -> EmergeSkia.stop(r) end)
        {app, r}
      end

    for {_, r} <- renderers,
        do:
          EmergeSkia.upload_tree(
            r,
            image([width(px(10)), height(px(6)), image_fit(:cover)], "name.png")
          )

    for {tag, rgba} <- [
          {"emerge_png_root_a", <<255, 0, 0, 255>>},
          {"emerge_png_root_b", <<0, 128, 255, 255>>}
        ] do
      expected = :binary.copy(rgba, 60)

      assert_receive {^tag, %VideoInterop.Frame{storage: %VideoInterop.Binary{data: ^expected}}},
                     3000
    end
  end
end
