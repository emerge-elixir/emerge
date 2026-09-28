defmodule Emerge.AnimationChangeTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias Emerge.Engine.{AttrCodec, AttrValidation, Serialization}
  alias EmergeSkia.Native

  test "change keeps ordinary targets and independent field policies" do
    node =
      el(
        [
          Animation.change([width(fill())], 1000, :linear),
          Animation.change([height(px(60))], 200, :ease_out)
        ],
        none()
      )

    assert node.attrs.width == :fill
    assert node.attrs.height == {:px, 60}

    assert node.attrs.animate_change == %{
             width: %{duration: 1000, curve: :linear},
             height: %{duration: 200, curve: :ease_out}
           }

    assert AttrCodec.decode_attrs(AttrCodec.encode_attrs(node.attrs)) ==
             Emerge.Engine.Tree.Attrs.strip_runtime_attrs(node.attrs)
  end

  test "one change call assigns a shared policy to multiple attributes" do
    node = el([Animation.change([width(content()), height(px(60))], 1000, :linear)], text("S"))

    assert node.attrs.width == :content
    assert node.attrs.height == {:px, 60}

    assert node.attrs.animate_change == %{
             width: %{duration: 1000, curve: :linear},
             height: %{duration: 1000, curve: :linear}
           }
  end

  test "list entries keep last-write ordering within and across change calls" do
    ExUnit.CaptureIO.capture_io(:stderr, fn ->
      node =
        el(
          [
            Animation.change(
              [width(px(40)), width(fill()), height(px(60)), spacing(4), spacing_xy(8, 12)],
              1000,
              :linear
            ),
            Animation.change([height(px(80))], 200, :ease_out),
            width(px(70))
          ],
          none()
        )

      assert node.attrs.width == {:px, 70}
      assert node.attrs.height == {:px, 80}
      assert node.attrs.spacing_xy == {8, 12}
      refute Map.has_key?(node.attrs, :spacing)

      assert node.attrs.animate_change == %{
               height: %{duration: 200, curve: :ease_out},
               spacing_xy: %{duration: 1000, curve: :linear}
             }
    end)
  end

  test "empty change lists are no-ops and malformed lists fail eagerly" do
    assert el([Animation.change([], 1000, :linear)], none()).attrs == el([], none()).attrs

    for attrs <- [
          nil,
          width(fill()),
          %{width: :fill},
          [nil],
          [width(fill()), nil],
          [[width(fill())]]
        ] do
      assert_raise ArgumentError, fn -> Animation.change(attrs, 1000, :linear) end
    end

    assert_raise ArgumentError, fn -> Animation.change([], 0, :linear) end
    assert_raise ArgumentError, fn -> Animation.change([width(fill())], 1000, :invalid) end

    # Raw public attr tuples go through the same list validation.
    assert_raise ArgumentError, fn ->
      el([{:animate_change, {width(fill()), 1000, :linear}}], none())
    end

    assert_raise ArgumentError, ~r/cannot own the same field/, fn ->
      el(
        [
          Animation.animate([[height(px(20))], [height(px(60))]], 1000, :linear),
          Animation.change([width(fill()), height(px(60))], 1000, :linear)
        ],
        none()
      )
    end
  end

  test "plain attributes clear policies and spacing aliases obey last write" do
    warnings =
      ExUnit.CaptureIO.capture_io(:stderr, fn ->
        node =
          el(
            [
              Animation.change([width(fill())], 1000, :linear),
              width(px(70)),
              Animation.change([spacing_xy(10, 20)], 500, :linear),
              spacing(5)
            ],
            none()
          )

        refute Map.has_key?(node.attrs, :animate_change)
        refute Map.has_key?(node.attrs, :spacing_xy)
        assert node.attrs.spacing == 5
      end)

    assert warnings =~ "last value wins"

    node =
      el(
        [
          Animation.change([spacing(10)], 500, :linear),
          Animation.change([spacing_xy(20, 30)], 1000, :ease_in)
        ],
        none()
      )

    assert node.attrs.animate_change == %{spacing_xy: %{duration: 1000, curve: :ease_in}}
    refute Map.has_key?(node.attrs, :spacing)
  end

  test "invalid policies and conflicting owners fail before encoding" do
    for attr <- [nil, {:width, nil}, {:on_change, :event}],
        do: assert_raise(ArgumentError, fn -> Animation.change([attr], 1000, :linear) end)

    for duration <- [0, -1, :bad],
        do:
          assert_raise(ArgumentError, fn ->
            Animation.change([width(fill())], duration, :linear)
          end)

    for attrs <- [
          %{animate_change: %{width: %{duration: 1000, curve: :linear}}},
          %{width: :fill, animate_change: %{width: %{duration: 1000}}},
          %{
            spacing: 4,
            spacing_xy: {4, 4},
            animate_change: %{
              spacing: %{duration: 1, curve: :linear},
              spacing_xy: %{duration: 1, curve: :linear}
            }
          }
        ],
        do: assert_raise(ArgumentError, fn -> AttrCodec.encode_attrs(attrs) end)

    assert_raise ArgumentError, ~r/cannot own the same field/, fn ->
      el(
        [
          Animation.animate([[width(px(40))], [width(fill())]], 1000, :linear),
          Animation.change([width(fill())], 500, :linear)
        ],
        none()
      )
    end
  end

  test "changes preserve non-length compatibility checks" do
    before = %{padding: 4}

    after_attrs = %{
      padding: {1, 2, 3, 4},
      animate_change: %{padding: %{duration: 1000, curve: :linear}}
    }

    assert_raise ArgumentError, ~r/padding/, fn ->
      AttrValidation.validate_change_update!(before, after_attrs)
    end

    assert :ok =
             AttrValidation.validate_change_update!(%{width: {:px, 40}}, %{
               width: :fill,
               animate_change: %{width: %{duration: 1000, curve: :linear}}
             })
  end

  test "min/max stay static and are rejected as animation endpoints or change sources" do
    for field <- [:width, :height], bound <- [:min, :max] do
      value = {bound, {:px, 50}, :fill}
      node = el([{field, value}], none())
      assert node.attrs[field] == value

      assert AttrCodec.decode_attrs(AttrCodec.encode_attrs(%{field => value})) == %{
               field => value
             }

      for owner <- [:animate, :animate_enter, :animate_exit],
          frames <- [
            [%{field => value}, %{field => :fill}],
            [%{field => :fill}, %{field => value}]
          ] do
        assert_raise ArgumentError, ~r/cannot animate min\/max/, fn ->
          AttrCodec.encode_attrs(%{
            owner => %{keyframes: frames, duration: 1000, curve: :linear, repeat: :once}
          })
        end
      end

      assert_raise ArgumentError, ~r/cannot animate min\/max/, fn ->
        Animation.change([{field, value}], 1000, :linear)
      end

      assert_raise ArgumentError, ~r/cannot animate min\/max/, fn ->
        AttrValidation.validate_change_update!(%{field => value}, %{
          field => :fill,
          :animate_change => %{field => %{duration: 1000, curve: :linear}}
        })
      end
    end
  end

  test "public change policies survive EMRG native decoding and first mount is immediate" do
    node = el([Animation.change([width(px(70)), height(px(30))], 1000, :linear)], none())
    {binary, assigned} = Serialization.encode(node)
    assert {:ok, ^binary} = Native.tree_roundtrip(binary)
    resource = Native.tree_new()
    assert {:ok, true} = Native.tree_upload(resource, binary)
    assert {:ok, frames} = Native.tree_layout(resource, 600, 100, 1.0)
    id = Emerge.Engine.NodeId.encode(assigned.id)
    assert {^id, _x, _y, 70.0, 30.0} = Enum.find(frames, &(elem(&1, 0) == id))
  end
end
