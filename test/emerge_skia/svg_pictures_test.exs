defmodule EmergeSkia.SvgPicturesTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  use Emerge.Assets.Path, otp_app: :emerge
  alias Emerge.Engine.{DiffState, Patch, Serialization}
  alias Emerge.TestSupport.{BoxAssertions, Headless}
  alias EmergeSkia.{Native, TreeRenderer}

  defmodule View do
    use Emerge
    @impl true
    def mount(opts), do: {:ok, opts, %Emerge.Config{}}
    @impl true
    def render(_state), do: none()

    def sources(xml, png),
      do: [
        svg([], path("a.svg")),
        svg([], binary(xml)),
        image([], path("b.png")),
        image([], binary(png))
      ]
  end

  @wide ~m"test_assets/single_axis_wide.svg"
  @xml ~s(<svg xmlns="http://www.w3.org/2000/svg" width="12.5" height="6.25" viewBox="-10 20 20 10"><rect x="-10" y="20" width="10" height="10" fill="red"/><rect y="20" width="10" height="10" fill="blue"/></svg>)
  defp file(name), do: File.read!(Application.app_dir(:emerge, "priv/" <> name))

  defp pixels(tree, w, h, opts \\ []),
    do: TreeRenderer.render_to_pixels(tree, [otp_app: :emerge, width: w, height: h] ++ opts, 3000)

  defp rgba(b, w, x, y), do: binary_part(b, (y * w + x) * 4, 4)
  defp axes(w, h), do: [width(px(w)), height(px(h))]

  test "helpers declare sources without IO under either DSL" do
    png = file("test_assets/alpha_grid.png")

    assert View.sources(@xml, png) == [
             svg([], path("a.svg")),
             svg([], binary(@xml)),
             image([], path("b.png")),
             image([], binary(png))
           ]

    assert path("missing.svg") == "missing.svg"
    assert path("/missing.svg") == {:path, "/missing.svg"}

    for wrong <- [nil, 1, [], ""] do
      assert_raise ArgumentError, fn -> path(wrong) end
      assert_raise ArgumentError, fn -> binary(wrong) end
    end

    assert_raise ArgumentError, fn -> path(<<255>>) end
    assert_raise ArgumentError, fn -> path("a\0b") end
    assert_raise ArgumentError, fn -> binary(:binary.copy(<<0>>, 67_108_865)) end
    assert_raise ArgumentError, fn -> svg([], binary(:binary.copy(" ", 4_194_305))) end
    assert_raise ArgumentError, fn -> image([Svg.color(:red)], binary(@xml)) end
    assert_raise ArgumentError, fn -> el([Background.image(binary(png))], none()) end
  end

  test "one full definition; sparse edits refer only to the accepted declaration" do
    png = file("test_assets/alpha_grid.png")
    tree = fn w -> row([], for(i <- 1..3, do: image([key(i), width(px(w + i))], binary(png)))) end
    {full, s} = DiffState.diff_and_encode_binary(DiffState.new(), tree.(10))
    digest = :crypto.hash(:sha256, png)
    assert length(:binary.matches(full, png)) == 1
    assert length(:binary.matches(full, <<53, 4, digest::binary>>)) == 2
    assert Enum.all?(Serialization.decode(full).children, &(&1.attrs.image_src == binary(png)))
    resource = Native.tree_new()
    assert {:ok, ^full} = Native.tree_upload_roundtrip(resource, full)
    {edit, s} = DiffState.diff_and_encode_binary(s, tree.(12))
    assert :binary.match(edit, png) == :nomatch
    assert length(:binary.matches(edit, <<53, 4, digest::binary>>)) == 3
    assert {:ok, roundtrip} = Native.tree_patch_roundtrip(resource, edit)
    assert length(:binary.matches(roundtrip, png)) == 1

    assert Enum.all?(
             Serialization.decode(roundtrip).children,
             &(&1.attrs.image_src == binary(png))
           )

    {clear, s} = DiffState.diff_and_encode_binary(s, nil)
    assert {:ok, true} = Native.tree_patch(resource, clear)
    {reopen, _} = DiffState.diff_and_encode_binary(s, tree.(10))
    assert length(:binary.matches(reopen, png)) == 1
    broken = :binary.replace(reopen, <<53, 3, digest::binary>>, <<53, 3, 0::256>>)
    assert {:error, _} = Native.tree_upload(resource, broken)

    assert {:error, _} =
             Native.tree_upload(resource, binary_part(reopen, 0, byte_size(reopen) - 1))

    # A locally constructed digest reference cannot rely on a former mount or idle LRU.
    ref_only =
      Patch.encode(Patch.patch(0, {:node, 10, :image, %{image_src: {:digest, digest}}, []}))

    assert {:error, _} = Native.tree_upload(resource, ref_only)
    assert {:ok, ^reopen} = Native.tree_upload_roundtrip(resource, reopen)
  end

  test "framing and previously accepted bytes count towards the aggregate limit" do
    half = 32 * 1024 * 1024
    a = binary(:binary.copy("a", half))
    b = binary(:binary.copy("b", half))
    tree = fn second -> row([], [image([], a), image([], second)]) end

    assert_raise ArgumentError, ~r/EMRG exceeds/, fn ->
      DiffState.diff_and_encode_binary(DiffState.new(), tree.(b))
    end

    {full, state} = DiffState.diff_and_encode_binary(DiffState.new(), tree.(a))
    r = Native.tree_new()
    assert {:ok, true} = Native.tree_upload(r, full)
    {patch, _} = DiffState.diff_and_encode_binary(state, tree.(b))
    assert {:error, "patched declarations exceed storage limit"} = Native.tree_patch(r, patch)
    assert {:ok, ^full} = Native.tree_patch_roundtrip(r, <<>>)
  end

  test "fractional intrinsic, viewBox origin, contain and cover are independent of source transport" do
    svg = fn attrs -> svg(attrs, binary(@xml)) end
    transparent = <<0, 0, 0, 0>>
    red = <<255, 0, 0, 255>>
    blue = <<0, 0, 255, 255>>
    # 12.5 : 6.25 is 2:1. The scaled content occupies exactly these rows.
    contain = pixels(svg.(axes(20, 20)), 20, 20)

    for {mode, frame} <- [
          {:contain, contain},
          {:cover, pixels(svg.([image_fit(:cover) | axes(20, 20)]), 20, 20)}
        ] do
      expected =
        for y <- 0..19, x <- 0..19, into: <<>> do
          cond do
            mode == :contain and y not in 5..14 -> transparent
            x < 10 -> red
            true -> blue
          end
        end

      assert frame == expected
    end

    assert pixels(svg.([width(px(20))]), 20, 10) == binary_part(contain, 5 * 20 * 4, 10 * 20 * 4)
    sources = [@wide, path(@wide.path), binary(file(@wide.path))]
    [b, bare, inline] = Enum.map(sources, &pixels(image([height(px(12))], &1), 30, 12))
    assert b == bare and bare == inline
    assert rgba(b, 30, 10, 6) == red
  end

  test "PNG and oriented JPEG bytes preserve the file codecs, sizes and fitted/full samples" do
    for {name, w, h} <- [
          {"test_assets/alpha_grid.png", 10, 6},
          {"test_assets/turn_right.jpeg", 8, 16}
        ],
        sized <- [false, true] do
      scene = fn src, attrs ->
        column([width(content())], [
          image(attrs, src),
          el([width(px(4)), height(px(2)), Background.color(:blue)], none())
        ])
      end

      for {attrs, height} <- [{[], h}, {[width(px(w / 2))], div(h, 2)}] do
        opts = [assets: [decode_at_size: sized]]
        result = pixels(scene.(binary(file(name)), attrs), 20, 20, opts)
        assert result == pixels(scene.(path(name), attrs), 20, 20, opts)
        assert rgba(result, 20, 1, height) == <<0, 0, 255, 255>>
      end
    end

    bytes = binary(file("test_assets/alpha_grid.png"))
    bad = pixels(svg(axes(10, 6), bytes), 10, 6)
    assert bad == pixels(image(axes(10, 6), binary("test_assets/alpha_grid.png")), 10, 6)
    refute bad == pixels(image(axes(10, 6), bytes), 10, 6)
  end

  @tag :tmp_dir
  test "absolute files require authorization even when identical inline art is ready", %{
    tmp_dir: dir
  } do
    dir = Path.expand(dir)
    name = Path.join(dir, "live.svg")
    link = Path.join(dir, "link.svg")
    File.write!(name, @xml)
    File.ln_s!(name, link)
    scene = fn src -> row([], [svg(axes(20, 10), binary(@xml)), svg(axes(20, 10), src)]) end
    good = pixels(scene.(binary(@xml)), 40, 10)
    bad = pixels(scene.(path(name)), 40, 10)
    assert good != bad
    allow = [enabled: true, allowlist: [dir], extensions: [".svg"]]

    for {opts, p, expected} <- [
          {allow, name, good},
          {allow, link, bad},
          {[follow_symlinks: true] ++ allow, link, good},
          {Keyword.put(allow, :allowlist, []), name, bad},
          {Keyword.put(allow, :extensions, [".png"]), name, bad},
          {Keyword.put(allow, :max_file_size, 8), name, bad}
        ] do
      assert pixels(scene.(path(p)), 40, 10, assets: [runtime_paths: opts]) == expected
    end
  end

  test "a snapshot never opens the source; await settles both ready and failed SVGs" do
    ready = svg(axes(20, 10), binary(@xml))
    invalid = svg(axes(20, 10), binary("not an SVG"))
    snapshot = pixels(ready, 20, 10, asset_mode: :snapshot)
    assert snapshot == pixels(invalid, 20, 10, asset_mode: :snapshot)
    assert rgba(snapshot, 20, 0, 0) == <<0, 0, 0, 0>>
    assert rgba(pixels(ready, 20, 10, asset_mode: :await), 20, 0, 0) == <<255, 0, 0, 255>>
    assert rgba(pixels(invalid, 20, 10, asset_mode: :await), 20, 0, 0) == <<254, 242, 242, 255>>
  end

  test "template tint and group opacity apply once to overlapping translucent ink" do
    xml =
      ~S|<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4"><rect width="6" height="4" fill="red" opacity="0.5"/><rect x="2" width="6" height="4" fill="blue" opacity="0.5"/></svg>|

    art = svg([Svg.color(:green), Transform.alpha(0.5)], binary(xml))
    p = pixels(art, 8, 4)
    assert rgba(p, 8, 1, 2) == <<0, 64, 0, 64>>
    assert rgba(p, 8, 4, 2) == <<0, 96, 0, 96>>
    assert rgba(p, 8, 7, 2) == <<0, 64, 0, 64>>
  end

  test "luminance mask, clipped matrix-filtered strokes and pattern survive recording at two sizes" do
    xml = """
    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 24">
      <defs>
        <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="32" height="8">
          <rect width="12" height="8" fill="black"/><rect x="12" width="16" height="8" fill="white"/>
        </mask>
        <pattern id="tile" width="4" height="4" patternUnits="userSpaceOnUse"><rect width="2" height="4" fill="blue"/></pattern>
        <clipPath id="clip"><rect x="4" y="8" width="24" height="8"/></clipPath>
        <filter id="green" filterUnits="userSpaceOnUse" x="0" y="0" width="32" height="24" color-interpolation-filters="sRGB"><feColorMatrix values="0 0 0 0 0  0 0 0 0 1  0 0 0 0 0  0 0 0 1 0"/></filter>
      </defs>
      <rect width="32" height="8" mask="url(#m)" fill="red"/>
      <g clip-path="url(#clip)"><path d="M0 10H32M0 14H32" fill="none" stroke="red" stroke-width="2" filter="url(#green)"/></g>
      <rect y="16" width="32" height="8" fill="url(#tile)"/>
    </svg>
    """

    for scale <- [1, 3] do
      w = 32 * scale
      h = 24 * scale

      expected =
        for j <- 0..(h - 1), i <- 0..(w - 1), into: <<>> do
          x = div(i, scale)
          y = div(j, scale)

          cond do
            x in 12..27 and y < 8 -> <<255, 0, 0, 255>>
            x in 4..27 and y in [9, 10, 13, 14] -> <<0, 255, 0, 255>>
            y >= 16 and rem(x, 4) < 2 -> <<0, 0, 255, 255>>
            true -> <<0, 0, 0, 0>>
          end
        end

      frame = pixels(column([], [svg(axes(w, h), binary(xml))]), w, h)
      # Skia filters a picture-shader tile at fractional texel positions when
      # scaled; qualify the full mask/stroke region and tile interiors separately.
      assert binary_part(frame, 0, w * 16 * scale * 4) ==
               binary_part(expected, 0, w * 16 * scale * 4)

      for y <- (16 * scale)..(h - 1), x <- 0..(w - 1), scale == 1 or rem(x, scale) == 1 do
        assert rgba(frame, w, x, y) == rgba(expected, w, x, y)
      end
    end
  end

  test "template gradient is anchored to the content box; rounded and ancestor clips do not bleed" do
    paint = gradient([:red, :blue], 90)
    axes = axes(28, 28)
    reference = pixels(el([Background.color(paint) | axes], none()), 28, 28)
    art = pixels(svg([Svg.color(paint) | axes], binary(@xml)), 28, 28)

    assert binary_part(art, 7 * 28 * 4, 14 * 28 * 4) ==
             binary_part(reference, 7 * 28 * 4, 14 * 28 * 4)

    assert binary_part(art, 0, 7 * 28 * 4) == :binary.copy(<<0>>, 7 * 28 * 4)

    clipped =
      pixels(
        column([width(px(23))], [svg([image_fit(:cover), Border.rounded(8) | axes], @wide)]),
        28,
        28
      )

    assert rgba(clipped, 28, 0, 0) == <<0, 0, 0, 0>>
    assert rgba(clipped, 28, 22, 14) == <<255, 0, 0, 255>>

    assert Enum.all?(0..27, fn y ->
             binary_part(clipped, (y * 28 + 23) * 4, 5 * 4) == :binary.copy(<<0>>, 20)
           end)
  end

  test "bundled SVG text actually contributes ink; file and inline give identical buffers" do
    name = "test_assets/cache_complex.svg"
    xml = file(name)
    before = pixels(svg([], path(name)), 200, 200)
    assert before == pixels(image([], binary(xml)), 200, 200)
    no_text = pixels(svg([], binary(Regex.replace(~r/<text .*?<\/text>/s, xml, ""))), 200, 200)

    diff =
      for y <- 0..199,
          x <- 0..199,
          rgba(before, 200, x, y) != rgba(no_text, 200, x, y),
          do: {x, y}

    assert length(diff) > 300
    assert Enum.all?(diff, fn {x, y} -> x >= 30 and y in 158..181 end)
  end

  test "submitted blank then warm file/bytes remount has no placeholder or stale hits" do
    name = "test_assets/single_axis_wide.svg"

    scene = fn w, src ->
      column([width(px(w))], [
        svg([width(fill()), Event.on_click(:art)], src),
        el([width(px(6)), height(px(6)), Background.color(:blue), Event.on_click(:after)], none())
      ])
    end

    {v, r} = Headless.mount({64, 36}, fn -> scene.(40, path(name)) end, [:pointer_button])
    before = pixels(scene.(40, path(name)), 64, 36)
    Headless.pixels_matching(&(&1 == before))
    BoxAssertions.click(r, {2.0, 22.0}, :after, {2.0, 2.0})
    blank = fn -> el([width(fill()), height(fill()), Background.color(:green)], none()) end
    blank_pixels = pixels(blank.(), 64, 36)
    send(v, {:scene, blank})
    Headless.pixels_matching(&(&1 == blank_pixels))
    new = fn -> scene.(24, binary(file(name))) end
    send(v, {:scene, new})
    assert Headless.pixels() == pixels(new.(), 64, 36)
    BoxAssertions.click(r, {2.0, 14.0}, :after, {2.0, 2.0})
    BoxAssertions.click(r, {2.0, 2.0}, :art, {2.0, 2.0})
  end
end
