defmodule EmergeSkiaTest do
  use ExUnit.Case
  doctest EmergeSkia

  test "render_to_pixels returns RGBA binary" do
    pixels = EmergeSkia.render_to_pixels(10, 10, [{:rect, 0, 0, 10, 10, 0xFF0000FF}])
    # 10x10 pixels, 4 bytes each = 400 bytes
    assert byte_size(pixels) == 400
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
end
