defmodule EmergeSkiaTest do
  use ExUnit.Case
  doctest EmergeSkia
  import Emerge.UI

  alias Emerge.UI.Svg

  defp rgba_at(pixels, width, x, y) do
    offset = (y * width + x) * 4
    <<_::binary-size(offset), r, g, b, a, _::binary>> = pixels
    {r, g, b, a}
  end

  test "render_to_pixels returns RGBA binary" do
    tree = el([width(px(10)), height(px(10)), Emerge.UI.Background.color(:red)], none())

    pixels =
      EmergeSkia.render_to_pixels(tree, otp_app: :emerge, width: 10, height: 10)

    # 10x10 pixels, 4 bytes each = 400 bytes
    assert byte_size(pixels) == 400
  end

  test "render_to_pixels supports snapshot placeholders" do
    tree = image([width(px(32)), height(px(24))], "demo_images/missing.jpg")

    snapshot =
      EmergeSkia.render_to_pixels(
        tree,
        otp_app: :emerge,
        width: 32,
        height: 24,
        asset_mode: :snapshot
      )

    awaited =
      EmergeSkia.render_to_pixels(tree, otp_app: :emerge, width: 32, height: 24)

    assert byte_size(snapshot) == 32 * 24 * 4
    assert byte_size(awaited) == 32 * 24 * 4
    refute snapshot == awaited
  end

  test "render_to_pixels await mode resolves logical image assets" do
    good_tree = image([width(px(32)), height(px(24))], "demo_images/static.jpg")
    bad_tree = image([width(px(32)), height(px(24))], "demo_images/missing.jpg")

    good =
      EmergeSkia.render_to_pixels(good_tree, otp_app: :emerge, width: 32, height: 24)

    bad =
      EmergeSkia.render_to_pixels(bad_tree, otp_app: :emerge, width: 32, height: 24)

    assert byte_size(good) == 32 * 24 * 4
    assert byte_size(bad) == 32 * 24 * 4
    refute good == bad
  end

  test "render_to_pixels resolves logical SVG image assets" do
    tree = image([width(px(8)), height(px(8)), image_fit(:cover)], "demo_images/tile_quad.svg")

    pixels = EmergeSkia.render_to_pixels(tree, otp_app: :emerge, width: 8, height: 8)

    assert byte_size(pixels) == 8 * 8 * 4
    assert rgba_at(pixels, 8, 1, 1) == {255, 0, 0, 255}
    assert rgba_at(pixels, 8, 6, 1) == {0, 255, 0, 255}
    assert rgba_at(pixels, 8, 1, 6) == {0, 0, 255, 255}
    assert rgba_at(pixels, 8, 6, 6) == {255, 255, 0, 255}
  end

  test "render_to_pixels svg/2 preserves original multicolor SVGs by default" do
    tree = svg([width(px(8)), height(px(8)), image_fit(:cover)], "demo_images/tile_quad.svg")

    pixels = EmergeSkia.render_to_pixels(tree, otp_app: :emerge, width: 8, height: 8)

    assert byte_size(pixels) == 8 * 8 * 4
    assert rgba_at(pixels, 8, 1, 1) == {255, 0, 0, 255}
    assert rgba_at(pixels, 8, 6, 1) == {0, 255, 0, 255}
    assert rgba_at(pixels, 8, 1, 6) == {0, 0, 255, 255}
    assert rgba_at(pixels, 8, 6, 6) == {255, 255, 0, 255}
  end

  test "render_to_pixels svg/2 applies template tint when Svg.color is set" do
    tree =
      svg(
        [
          width(px(8)),
          height(px(8)),
          image_fit(:cover),
          Svg.color({:color_rgb, {255, 255, 255}})
        ],
        "demo_images/tile_quad.svg"
      )

    pixels = EmergeSkia.render_to_pixels(tree, otp_app: :emerge, width: 8, height: 8)

    assert byte_size(pixels) == 8 * 8 * 4
    assert rgba_at(pixels, 8, 1, 1) == {255, 255, 255, 255}
    assert rgba_at(pixels, 8, 6, 1) == {255, 255, 255, 255}
    assert rgba_at(pixels, 8, 1, 6) == {255, 255, 255, 255}
    assert rgba_at(pixels, 8, 6, 6) == {255, 255, 255, 255}
  end

  test "render_to_pixels svg/2 fails when source resolves to raster" do
    bad_tree = svg([width(px(32)), height(px(24))], "demo_images/static.jpg")
    failed_tree = image([width(px(32)), height(px(24))], "demo_images/missing.jpg")

    bad = EmergeSkia.render_to_pixels(bad_tree, otp_app: :emerge, width: 32, height: 24)
    failed = EmergeSkia.render_to_pixels(failed_tree, otp_app: :emerge, width: 32, height: 24)

    assert byte_size(bad) == 32 * 24 * 4
    assert bad == failed
  end

  test "render_to_pixels resolves logical SVG background repeat assets" do
    tree =
      el(
        [
          width(px(8)),
          height(px(8)),
          Emerge.UI.Background.image("demo_images/tile_quad.svg", fit: :repeat)
        ],
        none()
      )

    pixels = EmergeSkia.render_to_pixels(tree, otp_app: :emerge, width: 8, height: 8)

    assert byte_size(pixels) == 8 * 8 * 4
    assert rgba_at(pixels, 8, 0, 0) == {255, 0, 0, 255}
    assert rgba_at(pixels, 8, 1, 0) == {0, 255, 0, 255}
    assert rgba_at(pixels, 8, 0, 1) == {0, 0, 255, 255}
    assert rgba_at(pixels, 8, 1, 1) == {255, 255, 0, 255}
    assert rgba_at(pixels, 8, 0, 0) == rgba_at(pixels, 8, 2, 0)
    assert rgba_at(pixels, 8, 0, 0) == rgba_at(pixels, 8, 0, 2)
  end

  test "input mask constants" do
    assert EmergeSkia.input_mask_key() == 0x01
    assert EmergeSkia.input_mask_codepoint() == 0x02
    assert EmergeSkia.input_mask_cursor_pos() == 0x04
    assert EmergeSkia.input_mask_cursor_button() == 0x08
    assert EmergeSkia.input_mask_cursor_scroll() == 0x10
    assert EmergeSkia.input_mask_cursor_enter() == 0x20
    assert EmergeSkia.input_mask_resize() == 0x40
    assert EmergeSkia.input_mask_focus() == 0x80
    assert EmergeSkia.input_mask_all() == 0xFF
  end

  test "start/1 requires otp_app option" do
    assert_raise ArgumentError, ~r/missing required :otp_app option/, fn ->
      EmergeSkia.start(title: "No otp app")
    end
  end

  test "start/1 validates otp_app type" do
    assert_raise ArgumentError, ~r/otp_app must be an atom/, fn ->
      EmergeSkia.start(otp_app: "emerge")
    end
  end

  test "start/1 rejects removed legacy window backends" do
    assert {:error, {:error, "backend :wayland_legacy has been removed; use :wayland"}} =
             EmergeSkia.start(otp_app: :emerge, backend: :wayland_legacy)

    assert {:error,
            {:error, "backend :x11 is no longer supported; use :wayland on a Wayland session"}} =
             EmergeSkia.start(otp_app: :emerge, backend: :x11)
  end

  test "legacy start arities raise explicit otp_app guidance" do
    assert_raise ArgumentError, ~r/requires explicit otp_app/, fn ->
      EmergeSkia.start()
    end

    assert_raise ArgumentError, ~r/no longer supported/, fn ->
      EmergeSkia.start("Legacy")
    end
  end

  test "start/1 validates assets.fonts source type" do
    assert_raise ArgumentError, ~r/assets\.fonts\[\]\.source must be a logical string path/, fn ->
      EmergeSkia.start(
        otp_app: :emerge,
        assets: [fonts: [[family: "my-font", source: {:path, "/tmp/font.ttf"}]]]
      )
    end
  end

  test "start/1 validates assets.fonts weight range" do
    assert_raise ArgumentError,
                 ~r/assets\.fonts\[\]\.weight must be an integer between 100 and 900/,
                 fn ->
                   EmergeSkia.start(
                     otp_app: :emerge,
                     assets: [
                       fonts: [[family: "my-font", source: "fonts/MyFont.ttf", weight: 50]]
                     ]
                   )
                 end
  end

  test "start/1 rejects duplicate font variants" do
    assert_raise ArgumentError, ~r/duplicate assets\.fonts entries/, fn ->
      EmergeSkia.start(
        otp_app: :emerge,
        assets: [
          fonts: [
            [family: "my-font", source: "fonts/MyFont-Regular.ttf", weight: 400],
            [family: "my-font", source: "fonts/MyFont-Regular2.ttf", weight: 400]
          ]
        ]
      )
    end
  end

  test "start/1 validates assets.fonts extension allowlist" do
    assert_raise ArgumentError, ~r/extension must be one of/, fn ->
      EmergeSkia.start(
        otp_app: :emerge,
        assets: [fonts: [[family: "my-font", source: "fonts/MyFont.woff2", weight: 400]]]
      )
    end
  end

  test "load_font_file/4 normalizes native ok tuple" do
    priv_dir = :code.priv_dir(:emerge) |> List.to_string()
    path = Path.join(priv_dir, "demo_fonts/Lobster-Regular.ttf")

    assert File.regular?(path)
    assert :ok = EmergeSkia.load_font_file("lobster-test", 400, false, path)
  end

  test "video_target/2 accepts :prime mode at the Elixir API layer" do
    assert_raise ArgumentError, ~r/argument error/, fn ->
      EmergeSkia.video_target(make_ref(), id: "preview", width: 64, height: 32, mode: :prime)
    end
  end
end
