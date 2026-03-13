defmodule EmergeSkiaTest do
  use ExUnit.Case
  doctest EmergeSkia
  import Emerge.UI

  test "render_to_pixels returns RGBA binary" do
    tree = el([width(px(10)), height(px(10)), Emerge.UI.Background.color(:red)], none())

    pixels =
      EmergeSkia.render_to_pixels(tree, otp_app: :emerge, width: 10, height: 10)

    # 10x10 pixels, 4 bytes each = 400 bytes
    assert byte_size(pixels) == 400
  end

  test "render_to_pixels supports snapshot placeholders" do
    tree = image("demo_images/missing.jpg", [width(px(32)), height(px(24))])

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
    good_tree = image("demo_images/static.jpg", [width(px(32)), height(px(24))])
    bad_tree = image("demo_images/missing.jpg", [width(px(32)), height(px(24))])

    good =
      EmergeSkia.render_to_pixels(good_tree, otp_app: :emerge, width: 32, height: 24)

    bad =
      EmergeSkia.render_to_pixels(bad_tree, otp_app: :emerge, width: 32, height: 24)

    assert byte_size(good) == 32 * 24 * 4
    assert byte_size(bad) == 32 * 24 * 4
    refute good == bad
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
end
