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

  defp turned(degrees, host, radius, source) do
    size = [width(px(100)), height(px(100))]
    centered = [center_x(), center_y()]
    attrs = [Transform.rotate(degrees), Event.on_click(:art), image_fit(:cover) | size]
    media = fn a -> image(a, source) end

    box =
      case host do
        :roomy ->
          media.(centered ++ attrs)

        :borrow ->
          el([Border.rounded(radius) | centered ++ size], media.(attrs))

        :own ->
          media.([Border.rounded(radius) | centered ++ attrs])

        :stationary ->
          column(centered ++ size, [el([Border.rounded(radius) | size], media.(attrs))])
      end

    el([width(fill()), height(fill()), Background.color(:white), Event.on_click(:host)], box)
  end

  test "sparse turns and undo match fresh mounts and the inverse-mapped silhouette" do
    size = {180, 180}
    red = <<255, 0, 0, 255>>
    white = <<255, 255, 255, 255>>
    {pid, r} = Headless.mount(size, fn -> turned(0, :roomy, 0, @wide) end, [:pointer_button])

    for {angle, i} <- Enum.with_index([0, 30, 45, 90, -45, 0]) do
      scene = fn -> turned(angle, :roomy, 0, @wide) end
      expected = render(scene.(), size, assets: [decode_at_size: false])
      if i > 0, do: send(pid, {:scene, scene})
      assert Headless.pixels_matching(&(&1 == expected)) == expected
      theta = angle * :math.pi() / 180
      {c, s} = {:math.cos(theta), :math.sin(theta)}
      # Only ignore the two-pixel band at the analytical edge (bleed + AA).
      for x <- 0..179, y <- 0..179 do
        {dx, dy} = {x + 0.5 - 90, y + 0.5 - 90}
        edge = Kernel.max(abs(c * dx + s * dy), abs(c * dy - s * dx))

        if edge < 48 or edge > 52 do
          assert pixel(expected, 180, x, y) == if(edge < 48, do: red, else: white)
        end
      end
    end

    for {host, radius, y, hit} <- [
          {:borrow, 8, 30, true},
          {:borrow, 24, 25, false},
          {:own, 24, 25, false},
          {:own, 8, 30, true},
          {:stationary, 8, 30, false}
        ] do
      scene = fn -> turned(45, host, radius, @wide) end
      send(pid, {:scene, scene})
      expected = render(scene.(), size, assets: [decode_at_size: false])
      assert Headless.pixels_matching(&(&1 == expected)) == expected
      assert pixel(expected, 180, 90, y) == if(hit, do: red, else: white)

      if hit do
        for phase <- [:press, :release], do: BoxAssertions.button(r, phase, {90.0, y * 1.0})
        assert_receive {:application, {:art, %{x: lx, y: ly}}}
        assert_in_delta lx, 7.574, 0.002
        assert_in_delta ly, 7.574, 0.002
      else
        BoxAssertions.click(r, {90.0, y * 1.0}, :host, {90.0, y * 1.0})
      end

      BoxAssertions.click(r, {44.0, 44.0}, :host, {44.0, 44.0})
    end
  end

  test "paint scale composes about the image center, including reflection and collapse" do
    scene = fn s, a ->
      el(
        [width(px(40)), height(px(40)), Background.color(:white), Event.on_click(:host)],
        image(
          [
            width(px(18)),
            height(px(16)),
            center_x(),
            center_y(),
            Transform.scale(s),
            Transform.rotate(a),
            Event.on_click(:art)
          ],
          "test_assets/media.bmp"
        )
      )
    end

    {v, r} = Headless.mount({40, 40}, fn -> scene.(1, 0) end, [:pointer_button])
    red = <<255, 0, 0, 255>>
    blue = <<0, 0, 255, 255>>
    white = <<255, 255, 255, 255>>

    for {s, a, color, point, target, local} <- [
          {1, 0, red, {14.0, 20.0}, :art, {3.0, 8.0}},
          {-1, 0, blue, {14.0, 20.0}, :art, {15.0, 8.0}},
          {0, 0, white, {14.0, 20.0}, :host, {14.0, 20.0}},
          {2, 0, red, {14.0, 20.0}, :art, {6.0, 8.0}},
          {2, 90, red, {20.0, 14.0}, :art, {6.0, 8.0}}
        ] do
      build = fn -> scene.(s, a) end
      if s != 1, do: send(v, {:scene, build})
      b = render(build.(), {40, 40}, assets: [decode_at_size: false])
      assert Headless.pixels_matching(&(&1 == b)) == b
      {x, y} = point
      assert pixel(b, 40, round(x), round(y)) == color
      BoxAssertions.click(r, point, target, local)
    end
  end

  test "a cropped oriented JPEG and its border turn together inside group opacity" do
    frame = fn angle ->
      render(
        el(
          [width(fill()), height(fill()), Transform.alpha(0.5)],
          image(
            [
              center_x(),
              center_y(),
              width(px(20)),
              height(px(12)),
              Border.width(2),
              Border.color(:blue),
              image_fit(:cover),
              Transform.rotate(angle)
            ],
            @turned
          )
        ),
        {40, 40}
      )
    end

    a = frame.(0)
    b = frame.(90)
    assert pixel(a, 40, 10, 20) == <<0, 0, 128, 128>>
    refute pixel(a, 40, 14, 17) == pixel(a, 40, 24, 20)

    for x <- 0..39, y <- 0..39 do
      assert pixel(a, 40, x, y) == pixel(b, 40, 39 - y, x)
    end

    assert frame.(-360) == a
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

  test "a Row's definite inner height resolves automatic image width before sibling placement" do
    size = {240, 80}

    scene = fn src, h, attrs ->
      row([width(content()), height(px(h))], [
        image([height(fill()), Event.on_click(:image) | attrs], src),
        el(
          [width(px(8)), height(px(8)), Background.color(:blue), Event.on_click(:sibling)],
          none()
        )
      ])
    end

    {pid, r} = Headless.mount(size, fn -> scene.(@wide, 40, []) end, [:pointer_button])

    for {{src, h, attrs, x}, i} <-
          Enum.with_index([
            {@wide, 40, [], 80},
            {@wide, 60, [], 120},
            {@wide, 40, [], 80},
            {"test_assets/single_axis_wide.svg", 60, [width(content())], 120},
            {@turned, 60, [width(content()), padding_xy(4, 2), Border.width(2)], 38}
          ]) do
      build = fn -> scene.(src, h, attrs) end
      expected = render(build.(), size, assets: [decode_at_size: false])
      if i > 0, do: send(pid, {:scene, build})
      assert Headless.pixels_matching(&(&1 == expected)) == expected
      assert pixel(expected, 240, x, 3) == <<0, 0, 255, 255>>
      BoxAssertions.click(r, {x + 4.0, 3.0}, :sibling, {4.0, 3.0})
      if x > 84, do: BoxAssertions.click(r, {84.0, 3.0}, :image, {84.0, 3.0})
    end

    nested = row([width(px(120)), height(px(48)), padding(4)], [image([height(fill())], @wide)])

    for scale <- [1, 2] do
      p = render(nested, {240, 120}, scale: scale)
      assert pixel(p, 240, 83 * scale, 7 * scale) == <<255, 0, 0, 255>>
      assert pixel(p, 240, 87 * scale, 7 * scale) == <<0, 0, 0, 0>>
    end
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

  @tag :tmp_dir
  test "worker-decoded file and inline rasters honor format, dimensions and authorization", %{
    tmp_dir: dir
  } do
    axes = [width(px(9)), height(px(8))]

    for ext <- ["bmp", "gif"], sized <- [true, false] do
      name = "test_assets/media." <> ext
      bytes = File.read!(Application.app_dir(:emerge, "priv/" <> name))
      abs = Path.join(Path.expand(dir), "copy." <> ext)
      File.write!(abs, bytes)
      allow = [enabled: true, allowlist: [Path.dirname(abs)], extensions: ["." <> ext]]
      opts = [assets: [decode_at_size: sized, runtime_paths: allow]]

      expected =
        for y <- 0..7, x <- 0..8, into: <<>> do
          cond do
            x == 4 and rem(y, 2) == 0 -> <<0, 0, 0, if(ext == "gif", do: 0, else: 255)>>
            y < 2 -> <<0, 255, 0, 255>>
            x < 5 -> <<255, 0, 0, 255>>
            true -> <<0, 0, 255, 255>>
          end
        end

      for src <- [name, binary(bytes), path(abs)] do
        assert render(image(axes, src), {9, 8}, opts) == expected
      end

      for mode <- [:contain, :cover] do
        small = fn src -> image([image_fit(mode), width(px(4)), height(px(3))], src) end
        assert render(small.(name), {4, 3}, opts) == render(small.(binary(bytes)), {4, 3}, opts)
      end

      failed = render(image(axes, "missing.png"), {9, 8}, opts)
      assert render(svg(axes, binary(bytes)), {9, 8}, opts) == failed

      assert render(
               image(axes, binary(binary_part(bytes, 0, byte_size(bytes) - 1))),
               {9, 8},
               opts
             ) == failed

      warm = fn src -> row([], [image(axes, binary(bytes)), image(axes, src)]) end
      assert render(warm.(path(abs)), {18, 8}) == render(warm.("missing.png"), {18, 8})
      wrong = Path.join(Path.dirname(abs), "wrong.png")
      File.write!(wrong, bytes)

      assert render(warm.(path(wrong)), {18, 8},
               assets: [runtime_paths: Keyword.put(allow, :extensions, [".png"])]
             ) == render(warm.("missing.png"), {18, 8})
    end
  end

  test "a GIF sequence renders frame zero, and an oversized logical screen settles to failed art" do
    src = "test_assets/media.gif"
    good = render(image([], src), {9, 8})
    assert render(image([], "test_assets/sequence.gif"), {9, 8}) == good

    <<sig::binary-size(6), _::32, tail::binary>> =
      File.read!(Application.app_dir(:emerge, "priv/" <> src))

    oversized = binary(<<sig::binary, 65_535::little-16, 32_768::little-16, tail::binary>>)
    assert render(image([], oversized), {9, 8}) == render(image([], "missing.gif"), {9, 8})
  end

  test "bottom-up padded BGR and a top-down DIB agree; dimensions are checked before decoding" do
    b = File.read!(Application.app_dir(:emerge, "priv/test_assets/media.bmp"))
    <<head::binary-size(22), 8::little-32, tail::binary-size(28), rows::binary>> = b

    top_down =
      IO.iodata_to_binary([
        head,
        <<-8::little-signed-32>>,
        tail,
        Enum.reverse(for <<row::binary-size(28) <- rows>>, do: row)
      ])

    scene = fn src -> image([width(px(9)), height(px(8))], src) end
    good = render(scene.(binary(b)), {9, 8})
    assert render(scene.(binary(top_down)), {9, 8}) == good

    for bad <- [
          <<head::binary, 0::32, tail::binary, rows::binary>>,
          <<head::binary, 32_769::little-32, tail::binary, rows::binary>>
        ] do
      assert render(scene.(binary(bad)), {9, 8}) == render(scene.("missing.png"), {9, 8})
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
