defmodule Emerge.UITest do
  use ExUnit.Case, async: true
  use Emerge.UI

  import ExUnit.CaptureIO

  alias Emerge.Engine.Reconcile
  alias EmergeSkia.VideoTarget

  defmodule UsingEmergeUIComponent do
    use Emerge.UI

    def tree do
      el(
        [Background.color(color(:sky, 500)), Border.rounded(8), Font.color(color(:white))],
        Input.button([key(:save), Event.on_press(:save)], text("Save"))
      )
    end
  end

  test "use Emerge.UI imports color helpers and aliases UI modules" do
    element = UsingEmergeUIComponent.tree()

    assert element.attrs.background == {:color_rgb, {0, 166, 244}}
    assert element.attrs.border_radius == 8
    assert element.attrs.font_color == {:color_rgb, {255, 255, 255}}

    assert [%Emerge.Engine.Element{type: :el, children: [%Emerge.Engine.Element{type: :text}]}] =
             element.children
  end

  test "mouse_over stores decorative attrs" do
    element =
      el(
        [
          Interactive.mouse_over([
            Background.color(:red),
            Font.color(:white),
            Svg.color(:cyan),
            Font.size(18),
            Font.underline(),
            Font.strike(),
            Font.letter_spacing(1.5),
            Font.word_spacing(3),
            Transform.alpha(0.8),
            Transform.move_x(2)
          ])
        ],
        text("hi")
      )

    assert element.attrs.mouse_over == %{
             background: :red,
             font_color: :white,
             svg_color: :cyan,
             font_size: 18,
             font_underline: true,
             font_strike: true,
             font_letter_spacing: 1.5,
             font_word_spacing: 3,
             alpha: 0.8,
             move_x: 2
           }
  end

  test "interaction styles accept border and font attrs" do
    element =
      el(
        [
          Interactive.mouse_over([
            Border.rounded_each(1, 2, 3, 4),
            Border.width_each(5, 6, 7, 8),
            Border.dashed(),
            Font.family(:display),
            Font.bold(),
            Font.italic(),
            Font.center()
          ]),
          Interactive.focused([
            Border.rounded(9),
            Border.width(2),
            Border.dotted(),
            Font.family("mono"),
            Font.align_right()
          ])
        ],
        text("hi")
      )

    assert element.attrs.mouse_over.border_radius == {1, 2, 3, 4}
    assert element.attrs.mouse_over.border_width == {5, 6, 7, 8}
    assert element.attrs.mouse_over.border_style == :dashed
    assert element.attrs.mouse_over.font == :display
    assert element.attrs.mouse_over.font_weight == :bold
    assert element.attrs.mouse_over.font_style == :italic
    assert element.attrs.mouse_over.text_align == :center

    assert element.attrs.focused.border_radius == 9
    assert element.attrs.focused.border_width == 2
    assert element.attrs.focused.border_style == :dotted
    assert element.attrs.focused.font == "mono"
    assert element.attrs.focused.text_align == :right
  end

  test "font decoration and spacing helpers return attrs" do
    assert Font.underline() == {:font_underline, true}
    assert Font.strike() == {:font_strike, true}
    assert Font.letter_spacing(2.5) == {:font_letter_spacing, 2.5}
    assert Font.word_spacing(4) == {:font_word_spacing, 4}
  end

  test "font weight helpers return canonical attrs" do
    assert Font.thin() == {:font_weight, :thin}
    assert Font.extra_light() == {:font_weight, :extra_light}
    assert Font.light() == {:font_weight, :light}
    assert Font.regular() == {:font_weight, :regular}
    assert Font.normal() == {:font_weight, :regular}
    assert Font.medium() == {:font_weight, :medium}
    assert Font.semi_bold() == {:font_weight, :semi_bold}
    assert Font.bold() == {:font_weight, :bold}
    assert Font.extra_bold() == {:font_weight, :extra_bold}
    assert Font.black() == {:font_weight, :black}
  end

  test "font weight/1 maps the full numeric range" do
    assert Font.weight(100) == {:font_weight, :thin}
    assert Font.weight(200) == {:font_weight, :extra_light}
    assert Font.weight(300) == {:font_weight, :light}
    assert Font.weight(400) == {:font_weight, :regular}
    assert Font.weight(500) == {:font_weight, :medium}
    assert Font.weight(600) == {:font_weight, :semi_bold}
    assert Font.weight(700) == {:font_weight, :bold}
    assert Font.weight(800) == {:font_weight, :extra_bold}
    assert Font.weight(900) == {:font_weight, :black}
  end

  test "font weight/1 rejects unsupported values" do
    assert_raise ArgumentError, ~r/Font.weight\/1 expects one of/, fn ->
      Font.weight(0)
    end

    assert_raise ArgumentError, ~r/Font.weight\/1 expects one of/, fn ->
      Font.weight(150)
    end

    assert_raise ArgumentError, ~r/Font.weight\/1 expects one of/, fn ->
      Font.weight(950)
    end

    assert_raise ArgumentError, ~r/Font.weight\/1 expects one of/, fn ->
      Font.weight(:bold)
    end
  end

  test "focus_on_mount helper returns attr" do
    assert focus_on_mount() == {:focus_on_mount, true}
  end

  test "clip_nearby helper returns attr" do
    assert clip_nearby() == {:clip_nearby, true}
  end

  test "nearby mounts preserve attr-list definition order" do
    element =
      el(
        [
          Nearby.in_front(text("Front")),
          Nearby.above(text("Above")),
          Nearby.on_left(text("Left"))
        ],
        text("Host")
      )

    assert Enum.map(element.nearby, &elem(&1, 0)) == [:in_front, :above, :on_left]
  end

  test "size helpers return length values" do
    assert fill() == :fill
    assert fill(2) == {:fill, 2}
    assert width(fill(2)) == {:width, {:fill, 2}}
    assert height(fill(1.5)) == {:height, {:fill, 1.5}}
    assert min(px(50), fill()) == {:minimum, 50, :fill}
    assert max(px(120), shrink()) == {:maximum, 120, :content}
  end

  test "size helpers reject invalid fill and clamp values" do
    assert_raise ArgumentError, ~r/fill\/1 expects a positive number/, fn ->
      fill(0)
    end

    assert_raise ArgumentError, ~r/fill\/1 expects a positive number/, fn ->
      fill(-1)
    end

    assert_raise ArgumentError,
                 ~r/min\/2 expects the first argument to be px\(n\) with a non-negative number/,
                 fn ->
                   min(px(-10), fill())
                 end

    assert_raise ArgumentError,
                 ~r/max\/2 expects the first argument to be px\(n\) with a non-negative number/,
                 fn ->
                   max(px(-10), shrink())
                 end
  end

  test "length validation rejects invalid fill and clamp tuples" do
    assert_raise ArgumentError, ~r/fill weight to be a positive number/, fn ->
      el([width({:fill, 0})], text("bad"))
    end

    assert_raise ArgumentError, ~r/min length to be non-negative/, fn ->
      el([width({:minimum, -10, :fill})], text("bad"))
    end

    assert_raise ArgumentError, ~r/max length to be non-negative/, fn ->
      el([height({:maximum, -10, :content})], text("bad"))
    end
  end

  test "svg/2 accepts Svg.color and marks svg expectations" do
    element = svg([width(px(24)), height(px(24)), Svg.color(:white)], "icons/cloud.svg")

    assert element.type == :image
    assert element.attrs.image_src == "icons/cloud.svg"
    assert element.attrs.svg_color == :white
    assert element.attrs.svg_expected == true
  end

  test "UI attrs accept Emerge.UI.Color helper tuples" do
    element =
      el(
        [
          Background.color(color(:sky, 200, 0.3)),
          Border.color(color_rgb(1, 2, 3)),
          Font.color(color_rgba(4, 5, 6, 0.5))
        ],
        text("hi")
      )

    svg_element =
      svg([width(px(24)), height(px(24)), Svg.color(color(:white))], "icons/cloud.svg")

    assert element.attrs.background == {:color_rgba, {184, 230, 254, 77}}
    assert element.attrs.border_color == {:color_rgb, {1, 2, 3}}
    assert element.attrs.font_color == {:color_rgba, {4, 5, 6, 128}}
    assert svg_element.attrs.svg_color == {:color_rgb, {255, 255, 255}}
  end

  test "image/2 rejects Svg.color attrs" do
    assert_raise ArgumentError, ~r/image\/2 does not support attribute :svg_color/, fn ->
      image([width(px(24)), height(px(24)), Svg.color(:white)], "icons/cloud.svg")
    end
  end

  test "mouse_over rejects non-decorative attrs" do
    assert_raise ArgumentError, ~r/mouse_over only supports decorative attributes/, fn ->
      el([Interactive.mouse_over([width(fill())])], text("bad"))
    end
  end

  test "mouse_over rejects nested mouse_over" do
    assert_raise ArgumentError, ~r/mouse_over does not support nested mouse_over/, fn ->
      el([Interactive.mouse_over([Interactive.mouse_over([Transform.alpha(0.5)])])], text("bad"))
    end
  end

  test "focused and mouse_down store decorative attrs" do
    element =
      el(
        [
          Interactive.focused([
            Font.size(20),
            Font.color(:white),
            Transform.alpha(0.9),
            Border.glow(:cyan, 3)
          ]),
          Interactive.mouse_down([
            Background.color(:blue),
            Transform.move_y(-1),
            Border.inner_shadow(offset: {0, 1}, blur: 6, size: 1, color: :black)
          ])
        ],
        text("hi")
      )

    assert element.attrs.focused.font_size == 20
    assert element.attrs.focused.font_color == :white
    assert element.attrs.focused.alpha == 0.9

    assert element.attrs.mouse_down.background == :blue
    assert element.attrs.mouse_down.move_y == -1

    assert element.attrs.focused.box_shadow == [
             %{offset_x: 0, offset_y: 0, size: 3, blur: 6, color: :cyan, inset: false}
           ]

    assert element.attrs.mouse_down.box_shadow == [
             %{offset_x: 0, offset_y: 1, size: 1, blur: 6, color: :black, inset: true}
           ]
  end

  test "interaction styles ignore nil attrs" do
    element =
      el(
        [
          Interactive.mouse_over([nil, {:font_color, nil}, Font.underline()]),
          Interactive.focused([nil, Transform.alpha(0.9)]),
          Interactive.mouse_down([{:background, nil}, Transform.move_y(-1)])
        ],
        text("hi")
      )

    assert element.attrs.mouse_over == %{font_underline: true}
    assert element.attrs.focused == %{alpha: 0.9}
    assert element.attrs.mouse_down == %{move_y: -1}
  end

  test "animate stores normalized animation specs" do
    element =
      el(
        [
          Animation.animate(
            [
              [width(px(100)), padding_xy(12, 6), Transform.move_x(-20), Background.color(:red)],
              [
                width(px(160)),
                padding_each(8, 14, 10, 16),
                Transform.move_x(20),
                Background.color(:blue)
              ]
            ],
            420,
            :ease_in_out,
            3
          )
        ],
        text("hi")
      )

    assert element.attrs.animate == %{
             keyframes: [
               %{width: {:px, 100}, padding: {6, 12, 6, 12}, move_x: -20, background: :red},
               %{width: {:px, 160}, padding: {8, 14, 10, 16}, move_x: 20, background: :blue}
             ],
             duration: 420,
             curve: :ease_in_out,
             repeat: 3
           }
  end

  test "animate keyframes ignore nil attrs" do
    element =
      el(
        [
          Animation.animate(
            [
              [nil, width(px(100)), {:move_x, nil}],
              %{width: px(140), move_x: nil}
            ],
            200,
            :linear
          )
        ],
        text("hi")
      )

    assert element.attrs.animate == %{
             keyframes: [
               %{width: {:px, 100}},
               %{width: {:px, 140}}
             ],
             duration: 200,
             curve: :linear,
             repeat: :once
           }
  end

  test "animate_enter stores normalized animation specs" do
    element =
      el(
        [
          Animation.animate_enter(
            [
              [width(px(90)), Transform.alpha(0.2), Transform.move_y(8)],
              [width(px(140)), Transform.alpha(1.0), Transform.move_y(0)]
            ],
            260,
            :ease_out
          )
        ],
        text("hi")
      )

    assert element.attrs.animate_enter == %{
             keyframes: [
               %{width: {:px, 90}, alpha: 0.2, move_y: 8},
               %{width: {:px, 140}, alpha: 1.0, move_y: 0}
             ],
             duration: 260,
             curve: :ease_out,
             repeat: :once
           }
  end

  test "animate_exit stores normalized animation specs" do
    element =
      el(
        [
          Animation.animate_exit(
            [
              [width(px(140)), Transform.alpha(1.0), Transform.move_x(0)],
              [width(px(64)), Transform.alpha(0.0), Transform.move_x(-16)]
            ],
            220,
            :ease_in
          )
        ],
        text("bye")
      )

    assert element.attrs.animate_exit == %{
             keyframes: [
               %{width: {:px, 140}, alpha: 1.0, move_x: 0},
               %{width: {:px, 64}, alpha: 0.0, move_x: -16}
             ],
             duration: 220,
             curve: :ease_in,
             repeat: :once
           }
  end

  test "animate rejects mismatched keyframe attrs" do
    assert_raise ArgumentError, ~r/same attribute set/, fn ->
      el(
        [
          Animation.animate(
            [[width(px(100))], [width(px(120)), Transform.move_x(10)]],
            200,
            :linear
          )
        ],
        text("bad")
      )
    end
  end

  test "animate rejects incompatible width variants" do
    assert_raise ArgumentError, ~r/same length variant/, fn ->
      el([Animation.animate([[width(fill())], [width(px(120))]], 200, :linear)], text("bad"))
    end
  end

  test "animate_enter error messages name animate_enter" do
    assert_raise ArgumentError, ~r/animate_enter expects at least 2 keyframes/, fn ->
      el([Animation.animate_enter([[width(px(100))]], 200, :linear)], text("bad"))
    end
  end

  test "animate_exit only allows repeat once" do
    assert_raise ArgumentError, ~r/animate_exit expects :repeat to be :once/, fn ->
      el(
        [
          Animation.animate_exit(
            [[Transform.alpha(1.0)], [Transform.alpha(0.0)]],
            200,
            :linear,
            :loop
          )
        ],
        text("bad")
      )
    end
  end

  test "animate rejects tuple repeat counts" do
    assert_raise ArgumentError,
                 ~r/expects :repeat to be :once, :loop, or a positive integer/,
                 fn ->
                   el(
                     [
                       Animation.animate(
                         [[Transform.alpha(0.0)], [Transform.alpha(1.0)]],
                         200,
                         :linear,
                         {:times, 3}
                       )
                     ],
                     text("bad")
                   )
                 end
  end

  test "animate rejects zero repeat count" do
    assert_raise ArgumentError,
                 ~r/expects :repeat to be :once, :loop, or a positive integer/,
                 fn ->
                   el(
                     [
                       Animation.animate(
                         [[Transform.alpha(0.0)], [Transform.alpha(1.0)]],
                         200,
                         :linear,
                         0
                       )
                     ],
                     text("bad")
                   )
                 end
  end

  test "viewport root rejects animate_exit" do
    assert_raise ArgumentError, ~r/animate_exit is not allowed on the viewport root/, fn ->
      Reconcile.assign_ids(
        el(
          [
            Animation.animate_exit([[Transform.alpha(1.0)], [Transform.alpha(0.0)]], 200, :linear)
          ],
          text("bad")
        )
      )
    end
  end

  test "state styles append multiple box shadows" do
    element =
      el(
        [
          Interactive.focused([
            Border.glow(:cyan, 2),
            Border.glow(:blue, 3)
          ])
        ],
        text("hi")
      )

    assert element.attrs.focused.box_shadow == [
             %{offset_x: 0, offset_y: 0, size: 2, blur: 4, color: :cyan, inset: false},
             %{offset_x: 0, offset_y: 0, size: 3, blur: 6, color: :blue, inset: false}
           ]
  end

  test "direct state style maps normalize single box shadows" do
    shadow = %{offset_x: 0, offset_y: 1, blur: 6, size: 2, color: :black, inset: true}

    element = el([{:mouse_over, %{alpha: 0.75, box_shadow: shadow}}], text("hi"))

    assert element.attrs.mouse_over == %{
             alpha: 0.75,
             box_shadow: [shadow]
           }
  end

  test "direct state style maps validate nested values" do
    assert_raise ArgumentError, ~r/mouse_over expects :font_size to be a number/, fn ->
      el([{:mouse_over, %{font_size: "large"}}], text("bad"))
    end
  end

  test "focused rejects non-decorative attrs" do
    assert_raise ArgumentError, ~r/focused only supports decorative attributes/, fn ->
      el([Interactive.focused([width(fill())])], text("bad"))
    end
  end

  test "mouse_down rejects nested state styles" do
    assert_raise ArgumentError, ~r/mouse_down does not support nested focused/, fn ->
      el([Interactive.mouse_down([Interactive.focused([Transform.alpha(0.5)])])], text("bad"))
    end
  end

  test "paragraph creates a paragraph element with children" do
    element =
      paragraph([spacing(4), Font.size(16)], [
        text("Hello "),
        el([Font.bold()], text("world"))
      ])

    assert element.type == :paragraph
    assert element.attrs.spacing == 4
    assert element.attrs.font_size == 16
    assert length(element.children) == 2
  end

  test "paragraph/2 creates paragraph with explicit children list" do
    element = paragraph([], [text("Hello")])

    assert element.type == :paragraph
    assert element.attrs == %{__attrs_hash: Emerge.Engine.Tree.attrs_hash(%{})}
    assert length(element.children) == 1
  end

  test "row/2 accepts an empty children list" do
    element = row([], [])

    assert element.type == :row
    assert element.children == []
  end

  test "paragraph supports key attribute" do
    element = paragraph([key(:my_para), spacing(8)], [text("Hi")])

    assert element.type == :paragraph
    assert element.id == :my_para
    assert element.attrs.spacing == 8
  end

  test "text_column creates a text_column element with document defaults" do
    element =
      text_column([spacing(12)], [
        paragraph([spacing(4)], [text("First")]),
        paragraph([spacing(4)], [text("Second")])
      ])

    assert element.type == :text_column
    assert element.attrs.spacing == 12
    assert element.attrs.width == :fill
    assert element.attrs.height == :content
    assert length(element.children) == 2
  end

  test "text_column allows overriding default width and height" do
    element =
      text_column([width(px(420)), height(px(300))], [
        paragraph([], [text("Body")])
      ])

    assert element.type == :text_column
    assert element.attrs.width == {:px, 420}
    assert element.attrs.height == {:px, 300}
  end

  test "text_column supports key attribute" do
    element = text_column([key(:doc), spacing(10)], [paragraph([], [text("Hello")])])

    assert element.type == :text_column
    assert element.id == :doc
    assert element.attrs.spacing == 10
  end

  test "el/2 rejects a children list as the second argument" do
    assert_raise ArgumentError,
                 ~r/el\/2 expects the second argument to be a single child element, got a list/,
                 fn ->
                   el([], [text("Hello")])
                 end
  end

  test "row/2 requires the second argument to be a child element list" do
    assert_raise ArgumentError,
                 ~r/row\/2 expects the second argument to be a list of child elements, got:/,
                 fn ->
                   row([], text("Hello"))
                 end
  end

  test "row/2 validates every child entry" do
    assert_raise ArgumentError, ~r/row\/2 expects every child to be an Emerge element/, fn ->
      row([], [width(fill())])
    end
  end

  test "container constructors validate the attrs list" do
    assert_raise ArgumentError,
                 ~r/el\/2 expects the first argument to be a list of attributes, got:/,
                 fn ->
                   el(:bad_attrs, text("Hello"))
                 end
  end

  test "unknown attrs are rejected" do
    assert_raise ArgumentError, ~r/el\/2 does not support attribute :unknown_attr/, fn ->
      el([{:unknown_attr, 1}], text("Hello"))
    end
  end

  test "nil attrs are ignored in top-level attr lists" do
    element =
      el(
        [nil, {:background, nil}, Background.color(:red), {:below, nil}, Font.color(:white)],
        text("Hello")
      )

    assert element.attrs.background == :red
    assert element.attrs.font_color == :white
    assert element.nearby == []
  end

  test "unknown attrs with nil values are still rejected" do
    assert_raise ArgumentError, ~r/el\/2 does not support attribute :unknown_attr/, fn ->
      el([{:unknown_attr, nil}], text("Hello"))
    end
  end

  test "internal attrs are rejected from the public DSL" do
    assert_raise ArgumentError, ~r/id is not supported; use key instead/, fn ->
      el([{:id, :legacy}], text("Hello"))
    end
  end

  test "malformed attr entries are rejected" do
    assert_raise ArgumentError,
                 ~r/el\/2 expects attributes to be \{key, value\} tuples, got:/,
                 fn ->
                   el([:bad_attr], text("Hello"))
                 end
  end

  test "warns when an attribute key is overridden with a different value" do
    stderr =
      capture_io(:stderr, fn ->
        _ = el([align_left(), center_x()], text("warn"))
      end)

    assert stderr =~ "attribute :align_x is set multiple times"
    assert stderr =~ "last value wins"
  end

  test "does not warn when duplicate attribute uses the same value" do
    stderr =
      capture_io(:stderr, fn ->
        _ = el([align_left(), align_left()], text("same"))
      end)

    assert stderr == ""
  end

  test "warns only once per process for identical override signature" do
    stderr =
      capture_io(:stderr, fn ->
        _ = el([align_left(), center_x()], text("first"))
        _ = el([align_left(), center_x()], text("second"))
      end)

    assert length(Regex.scan(~r/attribute :align_x is set multiple times/, stderr)) == 1
  end

  # ============================================
  # Border.width_each
  # ============================================

  test "Border.width_each returns per-edge tuple" do
    assert Emerge.UI.Border.width_each(1, 2, 3, 4) == {:border_width, {1, 2, 3, 4}}
  end

  test "Border.width_each collapses uniform values" do
    assert Emerge.UI.Border.width_each(5, 5, 5, 5) == {:border_width, 5}
  end

  # ============================================
  # Border styles
  # ============================================

  test "Border.solid returns solid style" do
    assert Emerge.UI.Border.solid() == {:border_style, :solid}
  end

  test "Border.dashed returns dashed style" do
    assert Emerge.UI.Border.dashed() == {:border_style, :dashed}
  end

  test "Border.dotted returns dotted style" do
    assert Emerge.UI.Border.dotted() == {:border_style, :dotted}
  end

  # ============================================
  # Border.shadow / inner_shadow / glow
  # ============================================

  test "Border.shadow with defaults" do
    assert {:box_shadow, shadow} = Emerge.UI.Border.shadow()
    assert shadow == %{offset_x: 0, offset_y: 0, size: 0, blur: 10, color: :black, inset: false}
  end

  test "Border.shadow with options" do
    assert {:box_shadow, shadow} =
             Emerge.UI.Border.shadow(offset: {2, 3}, blur: 8, size: 4, color: :red)

    assert shadow == %{offset_x: 2, offset_y: 3, size: 4, blur: 8, color: :red, inset: false}
  end

  test "Border.inner_shadow with defaults" do
    assert {:box_shadow, shadow} = Emerge.UI.Border.inner_shadow()
    assert shadow.inset == true
    assert shadow.offset_x == 0
    assert shadow.offset_y == 0
  end

  test "Border.glow returns shadow with zero offset and doubled blur" do
    assert {:box_shadow, shadow} = Emerge.UI.Border.glow(:blue, 5)
    assert shadow == %{offset_x: 0, offset_y: 0, size: 5, blur: 10.0, color: :blue, inset: false}
  end

  # ============================================
  # Shadow accumulation
  # ============================================

  test "multiple shadows accumulate into list" do
    element =
      el(
        [
          Emerge.UI.Border.shadow(offset: {1, 1}, blur: 4, color: :black),
          Emerge.UI.Border.shadow(offset: {2, 2}, blur: 8, color: :red)
        ],
        text("shadows")
      )

    shadows = element.attrs.box_shadow
    assert length(shadows) == 2
    assert Enum.at(shadows, 0).color == :black
    assert Enum.at(shadows, 1).color == :red
  end

  test "text_column default width and height overrides do not emit warnings" do
    stderr =
      capture_io(:stderr, fn ->
        _ = text_column([width(px(420)), height(px(300))], [paragraph([], [text("Body")])])
      end)

    assert stderr == ""
  end

  test "image creates an image element" do
    element = image([width(px(120)), height(px(80)), image_fit(:cover)], "img_logo")

    assert element.type == :image
    assert element.attrs.image_src == "img_logo"
    assert element.attrs.image_fit == :cover
    assert element.attrs.width == {:px, 120}
    assert element.attrs.height == {:px, 80}
    assert element.children == []
  end

  test "video creates a video element" do
    target = %VideoTarget{id: "preview", width: 640, height: 360, mode: :prime, ref: make_ref()}

    element = video([width(px(160)), image_fit(:cover)], target)

    assert element.type == :video
    assert element.attrs.video_target == "preview"
    assert element.attrs.image_size == {640, 360}
    assert element.attrs.image_fit == :cover
    assert element.attrs.width == {:px, 160}
    assert element.children == []
  end

  test "on_change helper returns attr tuple" do
    assert Event.on_change({self(), :changed}) == {:on_change, {self(), :changed}}
  end

  test "on_press helper returns attr tuple" do
    assert Event.on_press({self(), :pressed}) == {:on_press, {self(), :pressed}}
  end

  test "swipe event helpers return attr tuples" do
    assert Event.on_swipe_up({self(), :swiped_up}) == {:on_swipe_up, {self(), :swiped_up}}
    assert Event.on_swipe_down({self(), :swiped_down}) == {:on_swipe_down, {self(), :swiped_down}}
    assert Event.on_swipe_left({self(), :swiped_left}) == {:on_swipe_left, {self(), :swiped_left}}

    assert Event.on_swipe_right({self(), :swiped_right}) ==
             {:on_swipe_right, {self(), :swiped_right}}
  end

  test "focus event helpers return attr tuples" do
    assert Event.on_focus({self(), :focused}) == {:on_focus, {self(), :focused}}
    assert Event.on_blur({self(), :blurred}) == {:on_blur, {self(), :blurred}}
  end

  test "key event helpers return normalized bindings" do
    assert {:on_key_down, binding} = Event.on_key_down(:enter, :submitted)
    assert binding.key == :enter
    assert binding.mods == []
    assert binding.match == :exact
    assert binding.payload == {self(), :submitted}
    assert binding.route == Event.key_route_id(:key_down, :enter, [], :exact)

    assert {:on_key_up, up_binding} =
             Event.on_key_up([key: :digit_1, mods: [:ctrl], match: :all], :released)

    assert up_binding.key == :digit_1
    assert up_binding.mods == [:ctrl]
    assert up_binding.match == :all
    assert up_binding.payload == {self(), :released}
    assert up_binding.route == Event.key_route_id(:key_up, :digit_1, [:ctrl], :all)

    assert {:on_key_press, press_binding} = Event.on_key_press(:space, :cycled)
    assert press_binding.key == :space
    assert press_binding.mods == []
    assert press_binding.match == :exact
    assert press_binding.payload == {self(), :cycled}
    assert press_binding.route == Event.key_route_id(:key_press, :space, [], :exact)
  end

  test "virtual_key helper returns normalized spec" do
    assert {:virtual_key, spec} =
             Event.virtual_key(
               tap: {:text_and_key, "A", :a, [:shift]},
               hold: {:event, {self(), :show_alternates}},
               hold_ms: 280,
               repeat_ms: 55
             )

    assert spec == %{
             tap: {:text_and_key, "A", :a, [:shift]},
             hold: {:event, {self(), :show_alternates}},
             hold_ms: 280,
             repeat_ms: 55
           }
  end

  test "virtual_key helper applies defaults" do
    assert {:virtual_key, spec} = Event.virtual_key(tap: {:key, :enter, []})

    assert spec == %{
             tap: {:key, :enter, []},
             hold: nil,
             hold_ms: 350,
             repeat_ms: 40
           }
  end

  test "virtual_key rejects on_click and on_press conflicts" do
    assert_raise ArgumentError,
                 ~r/does not allow :virtual_key together with :on_click or :on_press/,
                 fn ->
                   Input.button(
                     [Event.virtual_key(tap: {:text, "a"}), Event.on_press(:pressed)],
                     text("A")
                   )
                 end

    assert_raise ArgumentError,
                 ~r/does not allow :virtual_key together with :on_click or :on_press/,
                 fn ->
                   el([Event.virtual_key(tap: {:text, "a"}), Event.on_click(:clicked)], text("A"))
                 end
  end

  test "Input.text creates a text_input element" do
    element =
      Emerge.UI.Input.text(
        [
          key(:search),
          width(px(240)),
          Event.on_change({self(), :search_changed})
        ],
        "hello"
      )

    assert element.type == :text_input
    assert element.id == :search
    assert element.attrs.content == "hello"
    assert element.attrs.width == {:px, 240}
    assert element.attrs.on_change == {self(), :search_changed}
    assert element.children == []
  end

  test "Input.button creates an el with a child and handlers" do
    element =
      Emerge.UI.Input.button(
        [
          key(:save_btn),
          width(px(160)),
          Event.on_press({self(), :save_pressed}),
          Event.on_focus({self(), :save_focused}),
          Event.on_blur({self(), :save_blurred})
        ],
        text("Save")
      )

    assert element.type == :el
    assert element.id == :save_btn
    assert element.attrs.width == {:px, 160}
    assert element.attrs.on_press == {self(), :save_pressed}
    assert element.attrs.on_focus == {self(), :save_focused}
    assert element.attrs.on_blur == {self(), :save_blurred}
    assert length(element.children) == 1

    [label] = element.children
    assert label.type == :text
    assert label.attrs.content == "Save"
  end

  test "Input.button accumulates multiple key listeners" do
    element =
      Emerge.UI.Input.button(
        [
          key(:save_btn),
          Event.on_key_down(:enter, :submit),
          Event.on_key_down([key: :space, match: :all], :submit_with_mods),
          Event.on_key_up(:escape, :cancel),
          Event.on_key_press(:space, :cycle)
        ],
        text("Save")
      )

    assert [%{key: :enter}, %{key: :space}] = element.attrs.on_key_down
    assert [%{key: :escape}] = element.attrs.on_key_up
    assert [%{key: :space}] = element.attrs.on_key_press
  end

  test "event helpers wrap local messages with self" do
    assert Event.on_press(:save_pressed) == {:on_press, {self(), :save_pressed}}
    assert Event.on_swipe_left(:swiped_left) == {:on_swipe_left, {self(), :swiped_left}}
    assert Event.on_change(:changed) == {:on_change, {self(), :changed}}
    assert Event.on_click(:clicked) == {:on_click, {self(), :clicked}}
  end

  test "event helpers preserve explicit pid payloads" do
    pid = self()
    assert Event.on_press({pid, :save_pressed}) == {:on_press, {pid, :save_pressed}}
    assert Event.on_swipe_right({pid, :swiped_right}) == {:on_swipe_right, {pid, :swiped_right}}
    assert Event.on_change({pid, :changed}) == {:on_change, {pid, :changed}}
  end

  test "image/2 validates attrs are first" do
    assert_raise ArgumentError,
                 ~r/image\/2 expects the first argument to be a list of attributes, got:/,
                 fn ->
                   image("img_logo", [width(px(120)), height(px(80))])
                 end
  end

  test "video/2 validates attrs are first" do
    target = %VideoTarget{id: "preview", width: 640, height: 360, mode: :prime, ref: make_ref()}

    assert_raise ArgumentError,
                 ~r/video\/2 expects the first argument to be a list of attributes, got:/,
                 fn ->
                   video(target, [width(px(160))])
                 end
  end

  test "Input.text/2 validates attrs are first" do
    assert_raise ArgumentError,
                 ~r/Input\.text\/2 expects the first argument to be a list of attributes, got:/,
                 fn ->
                   Emerge.UI.Input.text("hello", [key(:search)])
                 end
  end

  test "Input.button/2 validates attrs are first" do
    assert_raise ArgumentError,
                 ~r/Input\.button\/2 expects the first argument to be a list of attributes, got:/,
                 fn ->
                   Emerge.UI.Input.button("Save", [key(:save_btn)])
                 end
  end

  test "Input.button/2 expects a single child element" do
    assert_raise ArgumentError,
                 ~r/Input\.button\/2 expects the second argument to be a single child element, got:/,
                 fn ->
                   Emerge.UI.Input.button([key(:save_btn)], "Save")
                 end
  end

  test "padding_xy expands to per-edge padding" do
    assert padding_xy(6, 3) == {:padding, {3, 6, 3, 6}}
  end

  test "Background.image encodes source and fit" do
    assert Background.image("img_hero") == {:background, {:image, "img_hero", :cover}}

    assert Background.image("img_hero", fit: :contain) ==
             {:background, {:image, "img_hero", :contain}}

    assert Background.image("img_hero", fit: :cover) ==
             {:background, {:image, "img_hero", :cover}}

    assert Background.image("img_hero", fit: :repeat) ==
             {:background, {:image, "img_hero", :repeat}}

    assert Background.image("img_hero", fit: :repeat_x) ==
             {:background, {:image, "img_hero", :repeat_x}}

    assert Background.image("img_hero", fit: :repeat_y) ==
             {:background, {:image, "img_hero", :repeat_y}}
  end

  test "Background helper modes mirror elm-ui" do
    assert Background.uncropped("img_hero") == {:background, {:image, "img_hero", :contain}}
    assert Background.tiled("img_hero") == {:background, {:image, "img_hero", :repeat}}
    assert Background.tiled_x("img_hero") == {:background, {:image, "img_hero", :repeat_x}}
    assert Background.tiled_y("img_hero") == {:background, {:image, "img_hero", :repeat_y}}
  end
end
