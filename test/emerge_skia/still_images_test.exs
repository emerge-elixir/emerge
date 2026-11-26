defmodule EmergeSkia.StillImagesTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  @dir "test_assets/stills/"
  defp bytes(name), do: File.read!(Application.app_dir(:emerge, "priv/" <> @dir <> name))

  defp frame(tree, {w, h}, sized, opts \\ []) do
    EmergeSkia.TreeRenderer.render_to_pixels(
      tree,
      Keyword.merge(
        [width: w, height: h, otp_app: :emerge, assets: [decode_at_size: sized]],
        opts
      ),
      3000
    )
  end

  defp box({w, h}), do: [width(px(w)), height(px(h))]

  @tag :tmp_dir
  test "indexed BMPs match the PNG over full and fitted file, runtime and inline paths", %{
    tmp_dir: tmp
  } do
    for name <- ~w(indexed1.bmp indexed4.bmp indexed8.bmp swatch.bmp), sized <- [true, false] do
      check_source(name, "swatch.png", {16, 8}, sized, tmp)
    end
  end

  @tag :tmp_dir
  test "still WebP uses the same source authorization and the same file-only decorations", %{
    tmp_dir: tmp
  } do
    for sized <- [true, false] do
      check_source("lossless.webp", "lossless.png", {16, 8}, sized, tmp)
    end
  end

  test "lossless opaque, alpha and EXIF stills equal the independently decoded PNGs" do
    for {file, dims} <- [{"lossless", {16, 8}}, {"clear", {10, 6}}, {"turn", {6, 10}}],
        color <- [:white, :black],
        sized <- [true, false] do
      scene = fn ext ->
        el([Background.color(color) | box(dims)], image([], @dir <> file <> ext))
      end

      assert frame(scene.(".webp"), dims, sized) == frame(scene.(".png"), dims, sized)
      refute frame(scene.(".png"), dims, sized) == frame(scene.(".missing.png"), dims, sized)
    end
  end

  test "lossy stills preserve alpha and agree on decoded interior colors" do
    for {name, {w, h} = dims, tolerance} <- [{"lossy", {16, 8}, 1}, {"alpha", {10, 6}, 20}],
        sized <- [false, true] do
      render = fn file -> frame(image([], @dir <> file), dims, sized) end
      actual = render.(name <> ".webp")
      reference = render.(name <> ".png")
      refute actual == render.("missing.png")
      alpha = fn bytes -> for <<_::24, a <- bytes>>, into: <<>>, do: <<a>> end
      assert alpha.(actual) == alpha.(reference)

      for x <- [2, w - 3], y <- [2, div(h, 2)], channel <- 0..2 do
        i = 4 * (w * y + x) + channel
        assert abs(:binary.at(actual, i) - :binary.at(reference, i)) <= tolerance
      end
    end
  end

  test "animated WebP and metadata-valid but undecodable WebP settle to failed art" do
    # Keep valid VP8L dimensions and only three bytes of the entropy stream.
    <<_::binary-size(20), prefix::binary-size(8), _::binary>> = bytes("lossless.webp")
    short = <<"RIFF", 20::little-32, "WEBPVP8L", 8::little-32, prefix::binary>>

    for sized <- [false, true] do
      fail = frame(image(box({16, 8}), "missing.png"), {16, 8}, sized)

      for src <- [@dir <> "animated.webp", binary(bytes("animated.webp")), binary(short)] do
        assert frame(image(box({16, 8}), src), {16, 8}, sized) == fail
      end
    end
  end

  defp check_source(name, png, size, sized, tmp) do
    data = bytes(name)
    abs = Path.join(Path.expand(tmp), name)
    File.write!(abs, data)
    allow = [assets: [decode_at_size: sized, runtime_paths: [enabled: true, allowlist: [abs]]]]

    for {w, h} = dims <- [size, {6, 4}], fit <- [:cover, :contain] do
      leaf = fn src -> image([image_fit(fit) | box(dims)], src) end
      good = frame(leaf.(@dir <> name), dims, sized)
      # A fitted WebP may be scaled by the codec rather than Skia's PNG resampler.
      if dims == size or not sized or Path.extname(name) == ".bmp" do
        assert good == frame(leaf.(@dir <> png), dims, sized)
      end

      bad = frame(leaf.("missing.png"), dims, sized)
      refute good == bad

      for src <- [@dir <> name, binary(data), path(abs)] do
        assert frame(leaf.(src), {w, h}, sized, allow) == good
      end

      assert frame(svg(box(dims), binary(data)), dims, sized) == bad
    end

    pair = fn src -> row([], [image(box(size), binary(data)), image(box(size), src)]) end
    {w, h} = size

    assert frame(pair.(path(abs)), {2 * w, h}, sized) ==
             frame(pair.("missing.png"), {2 * w, h}, sized)

    wrong = Path.join(tmp, "wrong.png") |> Path.expand()
    File.write!(wrong, data)

    wrong_opts = [
      assets: [decode_at_size: sized, runtime_paths: [enabled: true, allowlist: [wrong]]]
    ]

    assert frame(pair.(path(wrong)), {2 * w, h}, sized, wrong_opts) ==
             frame(pair.("missing.png"), {2 * w, h}, sized)

    for fit <- [:contain, :cover, :repeat, :repeat_x, :repeat_y] do
      bg = fn file -> el([Background.image(@dir <> file, fit: fit) | box({23, 17})], none()) end
      assert frame(bg.(name), {23, 17}, sized) == frame(bg.(png), {23, 17}, sized)
    end

    leaf = image(box(size), @dir <> name)

    assert frame(leaf, size, sized, asset_mode: :snapshot) ==
             frame(image(box(size), "missing.png"), size, sized, asset_mode: :snapshot)

    refute frame(leaf, size, sized) == frame(leaf, size, sized, asset_mode: :snapshot)
  end
end
