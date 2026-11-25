defmodule EmergeSkia.LiveAdmissionTest do
  use ExUnit.Case, async: false
  alias Emerge.Engine.Patch
  alias EmergeSkia.Native
  @max 64 * 1024 * 1024

  setup do
    {:ok, r} =
      EmergeSkia.start(%Emerge.Config{
        output: %Emerge.Config.Headless{target: self(), size: {4, 4}},
        renderer: [api: :raster],
        assets: [otp_app: :emerge]
      })

    on_exit(fn -> EmergeSkia.stop(r) end)
    %{r: r}
  end

  test "a bit-offset maximal wire is accepted; exceeding it or truncating it is atomic", %{r: r} do
    len = @max - 83
    payload = :binary.copy(<<0>>, len)
    digest = :crypto.hash(:sha256, payload)

    bits =
      <<1::1, "EMRG", 10, 0::64, 1::64, 1::32, 1::64, 9, 3, len + 40::32, 1::16, 53, 3,
        digest::binary, len::32, payload::binary, 0::32, 0::7>>

    <<_::1, wire::binary-size(@max), _::7>> = bits
    assert :ok = Native.renderer_upload(r, wire)
    assert {:ok, {s, 1}} = Native.renderer_revision_nif(r, nil)
    assert {:error, "edit exceeds storage limit"} = Native.renderer_patch(r, wire <> <<0>>)
    assert {:error, _} = Native.renderer_patch(r, binary_part(wire, 0, 40))
    assert {:ok, {^s, 1}} = Native.renderer_revision_nif(r, nil)
    assert :ok = Native.renderer_patch(r, Patch.encode(Patch.patch(1, nil)))
  end

  test "the final click on an installed epoch precedes its retirement on the relay", %{r: r} do
    :ok = Native.set_input_target(r, self())
    :ok = Native.set_input_mask(r, 0)

    wire =
      Patch.encode(
        Patch.patch(0, {:node, 1, :el, %{width: :fill, height: :fill, on_click: true}, []})
      )

    assert {:ok, {s, 1} = base} = Native.submit_revision(r, nil, wire, true, self())
    assert_receive {:emerge_skia_frame, _}, 2_000

    for phase <- [:press, :release],
        do: assert(:ok == Native.send_input(r, {:pointer_button, {:left, phase, [], {1, 1}}}))

    assert {:ok, {^s, 2}} = Native.submit_revision(r, base, <<>>, true, self())
    assert_receive {:emerge_skia_frame, _}, 2_000

    receive do
      {:emerge_skia_routed, 1, {<<1::64>>, :click, %{x: 1.0, y: 1.0}}} -> :ok
      {:emerge_skia_retire_routes, ^s, 1} -> flunk("retired before the final click")
    after
      2_000 -> flunk("no click on the installed epoch")
    end

    assert_receive {:emerge_skia_retire_routes, ^s, 1}, 2_000
    refute_received {:emerge_skia_retire_routes, ^s, 1}
  end

  test "input admission bounds modifier lists and text without dropping later controls", %{r: r} do
    four = [:shift, :ctrl, :alt, :meta]
    assert :ok = Native.send_input(r, {:pointer_button, {:left, :press, four, {0, 0}}})

    for mods <- [four ++ [:ctrl], [:ctrl | :not_a_list]] do
      assert {:error, "invalid or unsupported raw input"} =
               Native.send_input(r, {:pointer_button, {:left, :release, mods, {0, 0}}})
    end

    assert {:error, "invalid or unsupported raw input"} =
             Native.send_input(r, {:text_commit, {:binary.copy("a", 65_537), []}})

    assert :ok = Native.send_input(r, {:pointer_button, {:left, :release, [], {0, 0}}})
  end

  test "an inline reference may precede its definition in this publication, never in a later one",
       %{r: r} do
    png = File.read!(Application.app_dir(:emerge, "priv/test_assets/alpha_grid.png"))
    digest = :crypto.hash(:sha256, png)
    ref = {:node, 2, :image, %{image_src: {:digest, digest}}, []}

    full =
      Patch.encode(
        Patch.patch(
          0,
          {:node, 1, :row, %{},
           [child: ref, child: {:node, 3, :image, %{image_src: {:bytes, png}}, []}]}
        )
      )

    assert {:error, "inline image digest mismatch"} =
             Native.renderer_upload(
               r,
               :binary.replace(full, <<53, 3, digest::binary>>, <<53, 3, 0::256>>)
             )

    assert {:error, "undefined inline image reference"} =
             Native.renderer_upload(r, Patch.encode(Patch.patch(0, ref)))

    assert {:ok, {s, 0}} = Native.renderer_revision_nif(r, nil)
    assert :ok = Native.renderer_upload(r, full)
    assert {:ok, {^s, 1}} = Native.renderer_revision_nif(r, nil)
  end
end
