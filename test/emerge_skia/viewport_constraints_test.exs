defmodule EmergeSkia.ViewportConstraintsTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  defp panel(w, h, name) do
    el(
      [
        key(:panel),
        width(w),
        height(h),
        padding(6),
        Background.color(:red),
        Event.on_click(name)
      ],
      el([width(px(30)), height(px(16)), Background.color(:blue)], none())
    )
  end

  defp frame(w, h) do
    for y <- 0..79, x <- 0..119, into: <<>> do
      cond do
        x in 6..35 and y in 6..21 -> <<0, 0, 255, 255>>
        x < w and y < h -> <<255, 0, 0, 255>>
        true -> <<0::32>>
      end
    end
  end

  defp button(r, phase, x, y) do
    assert :ok = Headless.button(r, phase, x, y)
    assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {^x, ^y}}}}
  end

  defp click(r, x, y, name) do
    for phase <- [:press, :release], do: button(r, phase, x, y)
    assert_receive {:application, {^name, %{x: ^x, y: ^y}}}
  end

  test "a zero-padding El and the viewport give the same explicit width and height offers" do
    for nested <- [true, false],
        {w, h, ew, eh} <- [
          {fill(3), px(28), 120, 28},
          {px(42), fill(0.5), 42, 80},
          {fill(3), fill(0.5), 120, 80},
          {min(content(), fill(3)), max(content(), fill(0.5)), 42, 80}
        ] do
      scene = fn ->
        a = panel(w, h, :cold)
        if nested, do: el([width(px(120)), height(px(80))], a), else: a
      end

      {_, r} = Headless.mount({120, 80}, scene, [:pointer_button])
      assert Headless.pixels() == frame(ew, eh)
      click(r, ew - 2.0, eh - 2.0, :cold)
    end
  end

  test "a WrappedRow resolves a height cap after its width changes, on either incoming edge" do
    cap = min(content(), fill(3))

    for nested <- [false, true] do
      build = fn width_policy ->
        chip = fn -> el([width(px(36)), height(content())], el([height(px(12))], none())) end

        line =
          wrapped_row(
            [
              width(width_policy),
              height(cap),
              padding(2),
              spacing(4),
              Background.color(:red),
              Event.on_click(:line)
            ],
            [chip.(), chip.()]
          )

        if nested, do: el([width(px(80)), height(px(80))], line), else: line
      end

      {p, r} = Headless.mount({80, 80}, fn -> build.(fill(3)) end, [:pointer_button])

      for {w, h, edit} <- [{80, 16, nil}, {79, 32, min(px(79), fill(3))}, {80, 16, fill(3)}] do
        if edit, do: send(p, {:scene, fn -> build.(edit) end})
        row = :binary.copy(<<255, 0, 0, 255>>, w) <> :binary.copy(<<0::32>>, 80 - w)
        assert Headless.pixels() == :binary.copy(row, h) <> :binary.copy(<<0::32>>, 80 * (80 - h))
        click(r, 60.0, h - 3.0, :line)
      end
    end
  end

  test "a content-capped Column charges the new height policy even when its child's fit stays 28" do
    scene = fn h ->
      chip = fn -> el([width(px(38)), height(px(12))], none()) end

      column(
        [
          width(px(76)),
          height(min(content(), fill(3))),
          Background.color(:red),
          Event.on_click(:column)
        ],
        [
          wrapped_row([width(px(76)), height(h), spacing(4)], [chip.(), chip.()])
        ]
      )
    end

    {p, r} = Headless.mount({76, 80}, fn -> scene.(fill()) end, [:pointer_button])

    bands = fn h ->
      :binary.copy(<<255, 0, 0, 255>>, 76 * h) <> :binary.copy(<<0::32>>, 76 * (80 - h))
    end

    assert Headless.pixels() == bands.(12)
    click(r, 5.0, 5.0, :column)
    send(p, {:scene, fn -> scene.(content()) end})
    assert Headless.pixels() == bands.(28)
    click(r, 5.0, 20.0, :column)
    send(p, {:scene, fn -> scene.(fill()) end})
    assert Headless.pixels() == bands.(12)
  end

  test "a continuous press follows a keyed child promoted to the viewport and back" do
    scene = fn nested ->
      a =
        el(
          [
            key(:panel),
            width(fill(3)),
            height(fill(0.5)),
            Background.color(:red),
            Event.on_click(if(nested, do: :nested, else: :promoted))
          ],
          none()
        )

      if nested do
        row([key(:row), width(px(50)), height(px(20)), scale(2), Background.color(:blue)], [
          el([key(:spacer), width(px(10))], none()),
          a
        ])
      else
        a
      end
    end

    {p, r} = Headless.mount({120, 80}, fn -> scene.(true) end, [:pointer_button])

    pixels = fn nested ->
      for y <- 0..79, x <- 0..119, into: <<>> do
        cond do
          not nested -> <<255, 0, 0, 255>>
          y >= 40 or x >= 100 -> <<0::32>>
          x < 20 -> <<0, 0, 255, 255>>
          true -> <<255, 0, 0, 255>>
        end
      end
    end

    assert Headless.pixels() == pixels.(true)
    button(r, :press, 30.0, 10.0)
    send(p, {:scene, fn -> scene.(false) end})
    assert Headless.pixels() == pixels.(false)
    button(r, :release, 110.0, 70.0)
    assert_receive {:application, {:promoted, %{x: 110.0, y: 70.0}}}
    button(r, :press, 110.0, 70.0)
    send(p, {:scene, fn -> scene.(true) end})
    assert Headless.pixels() == pixels.(true)
    button(r, :release, 30.0, 10.0)
    assert_receive {:application, {:nested, %{x: 5.0, y: 5.0}}}
  end

  for axis <- [:width, :height, :both] do
    test "#{axis}: a sparse root edit publishes pixels, target and callback on the same frame" do
      {p, r} =
        Headless.mount({120, 80}, fn -> panel(px(42), px(28), :small) end, [:pointer_button],
          target_fps: 1
        )

      small = frame(42, 28)
      assert Headless.pixels() == small

      {w, h, ew, eh, x, y} =
        case unquote(axis) do
          :width -> {fill(3), px(28), 120, 28, 76.0, 10.0}
          :height -> {px(42), fill(0.5), 42, 80, 10.0, 40.0}
          :both -> {fill(3), fill(0.5), 120, 80, 76.0, 40.0}
        end

      send(
        p,
        {:scene,
         fn ->
           send(self(), :built)
           panel(w, h, :grown)
         end}
      )

      assert_receive {:application, :built}
      assert {:ok, ^small} = EmergeSkia.render_to_pixels(r)
      click(r, 4.0, 4.0, :small)
      for phase <- [:press, :release], do: button(r, phase, x, y)
      assert Headless.pixels(2500) == frame(ew, eh)
      click(r, x, y, :grown)
      refute_received {:application, {:grown, _}}
      refute_received {:application, {:small, _}}
    end
  end
end
