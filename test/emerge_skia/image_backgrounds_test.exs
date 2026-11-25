defmodule EmergeSkia.ImageBackgroundsTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.{BoxAssertions, Headless}

  defp frame(t, w, h, opts \\ []),
    do:
      EmergeSkia.TreeRenderer.render_to_pixels(
        t,
        [otp_app: :emerge, width: w, height: h] ++ opts,
        3000
      )

  defp px(b, w, x, y), do: binary_part(b, 4 * (w * y + x), 4)

  defp child(w, h, color, listener),
    do:
      el(
        [
          width(px(w)),
          height(px(h)),
          center_x(),
          center_y(),
          Background.color(color),
          Event.on_click(listener)
        ],
        none()
      )

  defp scene(background) do
    row([], [
      el(
        [
          width(px(40)),
          height(px(32)),
          padding(3),
          Border.rounded(8),
          Event.on_click(:box),
          background
        ],
        child(8, 6, :green, :child)
      ),
      el([width(px(6)), height(px(6)), Background.color(:blue), Event.on_click(:next)], none())
    ])
  end

  test "file backgrounds replace and undo without a layout slot or a listener of their own" do
    jpeg = Background.image("test_assets/turn_right.jpeg")
    svg = "test_assets/single_axis_wide.svg"

    steps = [
      jpeg,
      Background.uncropped(svg),
      Background.image(svg),
      Background.color(:purple),
      jpeg
    ]

    {v, r} = Headless.mount({60, 48}, fn -> scene(jpeg) end, [:pointer_button])

    for {art, i} <- Enum.with_index(steps) do
      tree = fn -> scene(art) end
      b = frame(tree.(), 60, 48, assets: [decode_at_size: false])
      if i != 0, do: send(v, {:scene, tree})
      assert Headless.pixels_matching(&(&1 == b)) == b
      assert px(b, 60, 0, 0) == <<0, 0, 0, 0>>
      assert px(b, 60, 20, 16) == <<0, 255, 0, 255>>
      assert px(b, 60, 41, 3) == <<0, 0, 255, 255>>

      if i in [1, 2],
        do: assert(px(b, 60, 20, 1) == if(i == 1, do: <<0, 0, 0, 0>>, else: <<255, 0, 0, 255>>))

      BoxAssertions.click(r, {20.0, 16.0}, :child, {4.0, 3.0})
      BoxAssertions.click(r, {10.0, 10.0}, :box, {10.0, 10.0})
      BoxAssertions.click(r, {42.0, 3.0}, :next, {2.0, 3.0})
    end

    # Without declared axes it is the child, not the 200x100 source, that sizes the host.
    tree =
      row([], [
        el([Background.image(svg)], child(8, 6, :green, :child)),
        child(6, 6, :blue, :next)
      ])

    assert px(frame(tree, 60, 48), 60, 8, 3) == <<0, 0, 255, 255>>
  end

  test "a file background is a block declaration, not a second Image source or a paragraph span" do
    attrs = [width(px(20)), height(px(20)), Background.image("test_assets/single_axis_wide.svg")]

    for host <- [
          el(attrs, none()),
          row(attrs, []),
          column(attrs, []),
          wrapped_row(attrs, []),
          wrapped_column(attrs, [])
        ] do
      assert px(frame(host, 20, 20), 20, 10, 10) == <<255, 0, 0, 255>>
    end

    alias Emerge.Engine.DiffState
    alias EmergeSkia.Native
    {full, base} = DiffState.diff_and_encode_binary(DiffState.new(), el(attrs, none()))
    r = Native.tree_new()
    assert {:ok, ^full} = Native.tree_upload_roundtrip(r, full)

    for tree <- [image(attrs, "test_assets/media.gif"), paragraph([], [el(attrs, text("span"))])] do
      {bad, _} = DiffState.diff_and_encode_binary(base, tree)
      assert {:error, _} = Native.tree_patch(r, bad)
      assert {:ok, ^full} = Native.tree_patch_roundtrip(r, <<>>)
    end
  end

  test "a file paints at the same layer as a solid fill between outer and inset shadows" do
    tree = fn bg ->
      el(
        [width(px(60)), height(px(48)), padding(8)],
        el(
          [
            width(px(40)),
            height(px(32)),
            Border.width(2),
            Border.color(:purple),
            Border.rounded(8),
            Border.shadow(color: :blue, size: 4, blur: 0, offset: {0, 0}),
            Border.inner_shadow(color: :black, size: 5, blur: 0, offset: {0, 0}),
            bg
          ],
          child(6, 6, :green, :child)
        )
      )
    end

    solid = frame(tree.(Background.color(:red)), 60, 48)
    art = frame(tree.(Background.image("test_assets/single_axis_wide.svg")), 60, 48)

    for {x, y, color} <- [
          {6, 24, <<0, 0, 255, 255>>},
          {11, 24, <<0, 0, 0, 255>>},
          {20, 24, <<255, 0, 0, 255>>},
          {28, 24, <<0, 255, 0, 255>>}
        ] do
      assert px(solid, 60, x, y) == color
      assert px(art, 60, x, y) == color
    end

    assert px(art, 60, 8, 24) == px(solid, 60, 8, 24)
    assert px(art, 60, 8, 8) == px(solid, 60, 8, 8)
  end

  @tag :tmp_dir
  test "a warm picture does not authorize a background path; snapshot and await remain distinct",
       %{tmp_dir: dir} do
    name = "test_assets/single_axis_wide.svg"
    abs = Path.expand("copy.svg", dir)
    xml = File.read!(Application.app_dir(:emerge, "priv/" <> name))
    File.write!(abs, xml)

    scene = fn src ->
      row([], [
        svg([width(px(12))], binary(xml)),
        el([width(px(24)), height(px(18)), Background.image(src)], none())
      ])
    end

    allowed = [assets: [runtime_paths: [enabled: true, allowlist: [abs], extensions: [".svg"]]]]
    good = frame(scene.(name), 40, 18)
    assert frame(scene.(path(abs)), 40, 18, allowed) == good
    assert px(good, 40, 14, 2) == <<255, 0, 0, 255>>
    denied = frame(scene.(path(abs)), 40, 18)
    assert denied == frame(scene.("missing.svg"), 40, 18)
    refute good == denied
    pending = frame(scene.(name), 40, 18, asset_mode: :snapshot)
    assert pending == frame(scene.("missing.svg"), 40, 18, asset_mode: :snapshot)
    refute pending == denied
    {v, _} = Headless.mount({40, 18}, fn -> scene.(name) end)
    Headless.pixels_matching(&(&1 == good))
    send(v, {:scene, fn -> scene.(path(abs)) end})
    assert Headless.pixels_matching(&(&1 == denied)) == denied
  end

  @tag :tmp_dir
  test "a huge intrinsic SVG tile fails without preventing a small cover draw", %{tmp_dir: dir} do
    abs = Path.expand("tile.svg", dir)

    File.write!(
      abs,
      ~s(<svg xmlns="http://www.w3.org/2000/svg" width="4097" height="4097"><rect width="4097" height="4097" fill="red"/></svg>)
    )

    allowed = [assets: [runtime_paths: [enabled: true, allowlist: [abs], extensions: [".svg"]]]]

    box = fn src, fit ->
      el([width(px(20)), height(px(20)), Background.image(src, fit: fit)], none())
    end

    assert frame(box.(path(abs), :cover), 20, 20, allowed) ==
             :binary.copy(<<255, 0, 0, 255>>, 400)

    assert frame(box.(path(abs), :repeat), 20, 20, allowed) ==
             frame(box.("missing.svg", :repeat), 20, 20)
  end

  test "tile modes repeat only their declared axes and leave no internal seams at fractional display scale" do
    for {src, tw, th} <- [{"sample_assets/tile_quad.svg", 2, 2}, {"test_assets/media.bmp", 9, 8}],
        scale <- [1, 1.5],
        fit <- [:repeat, :repeat_x, :repeat_y] do
      tile = frame(image([], src), tw, th)
      tree = el([width(px(24)), height(px(20)), Background.image(src, fit: fit)], none())
      {w, h} = {round(24 * scale), round(20 * scale)}
      actual = frame(tree, w, h, scale: scale)

      expected =
        for y <- 0..(h - 1), x <- 0..(w - 1), into: <<>> do
          if (fit == :repeat_x and y >= th) or (fit == :repeat_y and x >= tw),
            do: <<0, 0, 0, 0>>,
            else: px(tile, tw, rem(x, tw), rem(y, th))
        end

      assert actual == expected
    end
  end
end
