defmodule Emerge.Bench.Scenarios do
  @moduledoc false

  use Emerge.UI

  alias Emerge.Engine
  alias Emerge.Engine.Element
  alias Emerge.Engine.Patch
  alias EmergeSkia.VideoTarget

  @default_sizes [500]
  @scenario_ids [
    :list_text,
    :layout_matrix,
    :text_rich,
    :paint_rich,
    :interactive_rich,
    :nearby_rich,
    :nearby_code_show,
    :nearby_code_hide,
    :media_rich,
    :animation_rich,
    :scroll_rich
  ]

  @mutation_ids [
    :noop,
    :paint_attr,
    :layout_attr,
    :text_content,
    :event_attr,
    :animation_attr,
    :keyed_reorder,
    :insert_tail,
    :remove_tail,
    :nearby_slot_change,
    :nearby_reorder
  ]

  @constraint %{width: 960.0, height: 4_000.0, scale: 1.0}

  @layout_attr_keys MapSet.new([
                      :width,
                      :height,
                      :padding,
                      :spacing,
                      :spacing_xy,
                      :space_evenly,
                      :align_x,
                      :align_y,
                      :scrollbar_y,
                      :scrollbar_x,
                      :scroll_x,
                      :scroll_y,
                      :clip_nearby,
                      :border_width,
                      :text_align
                    ])

  @paint_attr_keys MapSet.new([
                     :background,
                     :border_radius,
                     :border_color,
                     :border_style,
                     :box_shadow,
                     :font_color,
                     :font_underline,
                     :font_strike,
                     :move_x,
                     :move_y,
                     :rotate,
                     :scale,
                     :alpha,
                     :svg_color,
                     :video_target
                   ])

  @text_measure_attr_keys MapSet.new([
                            :content,
                            :font_size,
                            :font,
                            :font_weight,
                            :font_style,
                            :font_letter_spacing,
                            :font_word_spacing,
                            :image_src,
                            :image_fit,
                            :image_size,
                            :svg_expected
                          ])

  @event_attr_keys MapSet.new([
                     :on_click,
                     :on_press,
                     :on_swipe_up,
                     :on_swipe_down,
                     :on_swipe_left,
                     :on_swipe_right,
                     :on_mouse_down,
                     :on_mouse_up,
                     :on_mouse_enter,
                     :on_mouse_leave,
                     :on_mouse_move,
                     :on_change,
                     :on_focus,
                     :on_blur,
                     :on_key_down,
                     :on_key_up,
                     :on_key_press,
                     :virtual_key,
                     :focus_on_mount
                   ])

  @interaction_attr_keys MapSet.new([:mouse_over, :focused, :mouse_down])
  @animation_attr_keys MapSet.new([:animate, :animate_enter, :animate_exit])

  def inputs(sizes \\ sizes(), scenario_ids \\ scenario_ids()) do
    for scenario_id <- scenario_ids, size <- sizes do
      scenario_input(scenario_id, size)
    end
    |> Map.new(fn input -> {input.label, input} end)
  end

  def list_text_inputs(sizes \\ sizes()), do: inputs(sizes, [:list_text])

  def scenario_ids do
    case System.get_env("EMERGE_BENCH_SCENARIOS") do
      nil -> @scenario_ids
      value -> value |> String.split(",", trim: true) |> Enum.map(&String.to_atom/1)
    end
  end

  def mutation_ids, do: @mutation_ids

  def sizes do
    case System.get_env("EMERGE_BENCH_SIZES") || System.get_env("EMERGE_BENCH_COUNT") do
      nil -> @default_sizes
      value -> value |> String.split(",", trim: true) |> Enum.map(&String.to_integer/1)
    end
  end

  def print_metadata(inputs) when is_map(inputs) do
    Enum.each(inputs, fn {label, input} ->
      metadata = input.metadata

      IO.puts(
        "benchmark scenario=#{label} nodes=#{metadata.node_count} text_nodes=#{metadata.text_node_count} " <>
          "item_count=#{metadata.item_count} full_emrg_bytes=#{metadata.full_emrg_bytes} " <>
          "types=#{format_counts(metadata.element_types)} attr_families=#{format_counts(metadata.attr_families)}"
      )

      Enum.each(metadata.patches, fn {mutation, patch} ->
        IO.puts(
          "benchmark patch=#{label}/#{mutation} invalidation=#{patch.expected_invalidation} " <>
            "ops=#{patch.op_count} bytes=#{patch.byte_size} operations=#{format_counts(patch.operation_counts)}"
        )
      end)
    end)
  end

  defp scenario_input(scenario_id, size) do
    initial = scenario_tree(scenario_id, size)

    variants = %{
      noop: initial,
      paint_attr: scenario_tree(scenario_id, size, paint_variant: true),
      layout_attr: scenario_tree(scenario_id, size, layout_variant: true),
      text_content: scenario_tree(scenario_id, size, text_variant: true),
      event_attr: scenario_tree(scenario_id, size, event_variant: true),
      animation_attr: scenario_tree(scenario_id, size, animation_variant: true),
      keyed_reorder: scenario_tree(scenario_id, size, order: :reverse),
      insert_tail: scenario_tree(scenario_id, size, count_delta: 1),
      remove_tail: scenario_tree(scenario_id, size, count_delta: -1),
      nearby_slot_change: scenario_tree(scenario_id, size, nearby_slot: :below),
      nearby_reorder: scenario_tree(scenario_id, size, nearby_order: :reverse)
    }

    {full_bin, state, assigned} = Engine.encode_full(Engine.diff_state_new(), initial)

    patch_bins =
      variants
      |> Map.new(fn {mutation, tree} ->
        {patch_bin, _next_state, _assigned} = Engine.diff_state_update(state, tree)
        {mutation, patch_bin}
      end)

    patch_metadata =
      patch_bins
      |> Map.new(fn {mutation, patch_bin} ->
        patches = Patch.decode(patch_bin)

        {mutation,
         %{
           byte_size: byte_size(patch_bin),
           op_count: length(patches),
           operations: patches |> Enum.map(&patch_operation/1) |> Enum.uniq(),
           operation_counts: operation_counts(patches),
           expected_invalidation: expected_invalidation(mutation, patches)
         }}
      end)

    %{
      id: scenario_id,
      label: "#{scenario_id}_#{size}",
      size: size,
      item_count: item_count(scenario_id, size),
      constraint: @constraint,
      initial: initial,
      variants: variants,
      assigned: assigned,
      state: state,
      full_bin: full_bin,
      patch_bins: patch_bins,
      metadata: %{
        scenario: Atom.to_string(scenario_id),
        size: size,
        item_count: item_count(scenario_id, size),
        node_count: count_nodes(assigned),
        text_node_count: count_type(assigned, :text),
        full_emrg_bytes: byte_size(full_bin),
        element_types: element_type_counts(assigned),
        attr_families: attr_family_counts(assigned),
        patches: patch_metadata,
        constraint: @constraint
      }
    }
  end

  defp scenario_tree(scenario_id, size, opts \\ []) do
    item_count = Kernel.max(item_count(scenario_id, size) + Keyword.get(opts, :count_delta, 0), 0)
    control = mutation_control(scenario_id, opts)
    body = scenario_body(scenario_id, item_count, opts)

    column([key({:scenario, scenario_id}), width(fill()), spacing(8)], [control, body])
  end

  defp item_count(:list_text, size), do: size
  defp item_count(:layout_matrix, size), do: rich_count(size)
  defp item_count(:text_rich, size), do: rich_count(size)
  defp item_count(:paint_rich, size), do: rich_count(size)
  defp item_count(:interactive_rich, size), do: rich_count(size)
  defp item_count(:nearby_rich, size), do: rich_count(size)
  defp item_count(:nearby_code_show, size), do: rich_count(size)
  defp item_count(:nearby_code_hide, size), do: rich_count(size)
  defp item_count(:media_rich, size), do: rich_count(size)
  defp item_count(:animation_rich, size), do: rich_count(size)
  defp item_count(:scroll_rich, size), do: rich_count(size)

  defp rich_count(size), do: Kernel.max(div(size, 5), 1)

  defp ordered_indexes(count, _opts) when count <= 0, do: []

  defp ordered_indexes(count, opts) do
    indexes = Enum.to_list(1..count)

    case Keyword.get(opts, :order, :forward) do
      :reverse -> Enum.reverse(indexes)
      :forward -> indexes
    end
  end

  defp mutation_control(scenario_id, opts) do
    attrs =
      ([
         key({scenario_id, :mutation_target}),
         width(fill()),
         padding(if(Keyword.get(opts, :layout_variant, false), do: 10, else: 6)),
         Background.color(
           if Keyword.get(opts, :paint_variant, false),
             do: color(:rose, 500),
             else: color(:slate, 100)
         ),
         Border.rounded(8),
         Border.color(color(:slate, 300)),
         Border.width(1)
       ] ++ control_nearby_attrs(scenario_id, opts))
      |> maybe_add_event(opts, scenario_id)
      |> maybe_add_animation(opts)

    text_suffix = if Keyword.get(opts, :text_variant, false), do: " changed", else: ""
    el(attrs, text("Mutation target #{scenario_id}#{text_suffix}"))
  end

  defp control_nearby_attrs(scenario_id, _opts)
       when scenario_id in [:nearby_code_show, :nearby_code_hide],
       do: []

  defp control_nearby_attrs(scenario_id, opts), do: [control_nearby_attr(scenario_id, opts)]

  defp control_nearby_attr(scenario_id, opts) do
    case Keyword.get(opts, :nearby_slot, :above) do
      :below -> Nearby.below(control_nearby(scenario_id))
      _ -> Nearby.above(control_nearby(scenario_id))
    end
  end

  defp control_nearby(scenario_id) do
    el(
      [
        key({scenario_id, :mutation_nearby}),
        width(px(120)),
        padding(4),
        Background.color(color(:sky, 100)),
        Border.rounded(6)
      ],
      text("Nearby")
    )
  end

  defp maybe_add_event(attrs, opts, scenario_id) do
    if Keyword.get(opts, :event_variant, false) do
      [Event.on_click({self(), {:benchmark_click, scenario_id}}) | attrs]
    else
      attrs
    end
  end

  defp maybe_add_animation(attrs, opts) do
    if Keyword.get(opts, :animation_variant, false) do
      [
        Animation.animate(
          [
            [Transform.alpha(0.85), Transform.move_y(0)],
            [Transform.alpha(1.0), Transform.move_y(2)]
          ],
          240,
          :ease_in_out,
          :loop
        )
        | attrs
      ]
    else
      attrs
    end
  end

  defp scenario_body(:list_text, count, opts) do
    column([key({:list_text, :body}), width(fill()), spacing(2)], list_text_rows(count, opts))
  end

  defp scenario_body(:layout_matrix, count, opts) do
    wrapped_row(
      [key({:layout_matrix, :body}), width(px(940)), spacing_xy(8, 8)],
      layout_cards(count, opts)
    )
  end

  defp scenario_body(:text_rich, count, opts) do
    text_column([key({:text_rich, :body}), width(px(920)), spacing(10)], text_blocks(count, opts))
  end

  defp scenario_body(:paint_rich, count, opts) do
    wrapped_row(
      [key({:paint_rich, :body}), width(px(940)), spacing_xy(10, 10)],
      paint_cards(count, opts)
    )
  end

  defp scenario_body(:interactive_rich, count, opts) do
    column(
      [key({:interactive_rich, :body}), width(px(920)), spacing(8)],
      interactive_rows(count, opts)
    )
  end

  defp scenario_body(:nearby_rich, count, opts) do
    wrapped_row(
      [key({:nearby_rich, :body}), width(px(940)), spacing_xy(18, 18)],
      nearby_hosts(count, opts)
    )
  end

  defp scenario_body(:nearby_code_show, count, opts) do
    column(
      [key({:nearby_code_show, :body}), width(px(920)), spacing(8)],
      nearby_code_rows(:nearby_code_show, count, opts)
    )
  end

  defp scenario_body(:nearby_code_hide, count, opts) do
    column(
      [key({:nearby_code_hide, :body}), width(px(920)), spacing(8)],
      nearby_code_rows(:nearby_code_hide, count, opts)
    )
  end

  defp scenario_body(:media_rich, count, opts) do
    wrapped_row(
      [key({:media_rich, :body}), width(px(940)), spacing_xy(12, 12)],
      media_cards(count, opts)
    )
  end

  defp scenario_body(:animation_rich, count, opts) do
    wrapped_row(
      [key({:animation_rich, :body}), width(px(940)), spacing_xy(10, 10)],
      animation_cards(count, opts)
    )
  end

  defp scenario_body(:scroll_rich, count, opts) do
    column([key({:scroll_rich, :body}), width(px(920)), spacing(10)], scroll_panels(count, opts))
  end

  defp list_text_rows(count, opts) do
    count
    |> ordered_indexes(opts)
    |> Enum.map(fn index ->
      row(
        [
          key({:list_text, :row, index}),
          width(fill()),
          padding_xy(4, 2),
          Background.color(if(rem(index, 2) == 0, do: color(:slate, 50), else: color(:white)))
        ],
        [text("Benchmark row #{index}: repeated text content for layout measurement")]
      )
    end)
  end

  defp layout_cards(count, opts) do
    count
    |> ordered_indexes(opts)
    |> Enum.map(fn index ->
      column(
        [
          key({:layout_matrix, :card, index}),
          width(px(220 + rem(index, 3) * 24)),
          height(if(rem(index, 4) == 0, do: px(132), else: content())),
          padding_each(8, 10 + rem(index, 4), 8, 10),
          spacing(6),
          Border.width(if(rem(index, 5) == 0, do: 2, else: 1)),
          Border.color(color(:slate, 200)),
          Border.rounded(8),
          Background.color(color(:slate, 50))
        ],
        [
          row([width(fill()), spacing(6)], [
            el([width(fill(2)), center_y()], text("Layout #{index}")),
            el([width(fill(1)), align_right()], text("#{rem(index, 7)}"))
          ]),
          wrapped_row([width(fill()), spacing_xy(4, 4)], layout_chips(index)),
          el([width(fill()), height(px(12 + rem(index, 3) * 4))], none())
        ]
      )
    end)
  end

  defp layout_chips(index) do
    for chip <- 1..3 do
      el(
        [
          key({:layout_matrix, :chip, index, chip}),
          width(if(chip == 3, do: min(px(44), content()), else: content())),
          padding_xy(6, 3),
          Background.color(color(:sky, 50)),
          Border.rounded(999)
        ],
        text("#{index}.#{chip}")
      )
    end
  end

  defp text_blocks(count, opts) do
    count
    |> ordered_indexes(opts)
    |> Enum.map(fn index ->
      paragraph(
        [
          key({:text_rich, :paragraph, index}),
          width(fill()),
          Font.family(if(rem(index, 2) == 0, do: "default", else: :default)),
          Font.size(14 + rem(index, 5)),
          Font.word_spacing(rem(index, 3)),
          Font.align_left()
        ],
        [
          text("Paragraph #{index} mixes plain text with "),
          el([Font.weight(600), Font.color(color(:slate, 900))], text("weighted spans")),
          text(" and "),
          el(
            [Font.italic(), Font.letter_spacing(0.2 * rem(index, 4))],
            text("styled inline text")
          ),
          text(" to exercise inherited and local text attrs."),
          el([Font.underline(), Font.color(color(:sky, 700))], text(" link")),
          el([Font.strike(), Font.color(color(:slate, 400))], text(" old"))
        ]
      )
    end)
  end

  defp paint_cards(count, opts) do
    count
    |> ordered_indexes(opts)
    |> Enum.map(fn index ->
      el(
        [
          key({:paint_rich, :card, index}),
          width(px(170)),
          height(px(88)),
          padding(10),
          Background.gradient(color(:sky, 100 + rem(index, 5) * 100), color(:violet, 200), 18),
          Border.rounded_each(10, 16, 10 + rem(index, 4), 14),
          Border.width(1),
          Border.color(if(rem(index, 2) == 0, do: color(:sky, 300), else: color(:violet, 300))),
          Border.dashed(),
          Border.shadow(offset: {0, 2}, blur: 8, size: 1, color: color_rgba(15, 23, 42, 0.18)),
          Transform.rotate((rem(index, 5) - 2) * 1.5),
          Transform.alpha(0.82 + rem(index, 4) * 0.04),
          Font.color(color(:slate, 900))
        ],
        column([spacing(4)], [
          text("Paint #{index}"),
          el(
            [Transform.move_x(rem(index, 4)), Font.color(color(:slate, 600))],
            text("shadow/gradient")
          )
        ])
      )
    end)
  end

  defp interactive_rows(count, opts) do
    count
    |> ordered_indexes(opts)
    |> Enum.map(fn index ->
      row(
        [key({:interactive_rich, :row, index}), width(fill()), spacing(8)],
        [
          Input.button(
            [
              key({:interactive_rich, :button, index}),
              width(px(180)),
              padding_xy(10, 8),
              Background.color(color(:slate, 700)),
              Border.rounded(8),
              Font.color(color(:white)),
              Event.on_press({self(), {:bench_press, index}}),
              Event.on_mouse_enter({self(), {:bench_enter, index}}),
              Event.on_mouse_leave({self(), {:bench_leave, index}}),
              Event.on_key_down(:enter, {self(), {:bench_key, index}}),
              Interactive.mouse_over([Background.color(color(:slate, 600))]),
              Interactive.focused([
                Border.color(color(:sky, 400)),
                Border.glow(color_rgba(56, 189, 248, 0.35), 2)
              ]),
              Interactive.mouse_down([Transform.move_y(1), Transform.alpha(0.92)])
            ],
            text("Action #{index}")
          ),
          Input.text(
            [
              key({:interactive_rich, :input, index}),
              width(px(220)),
              padding(8),
              Background.color(color(:white)),
              Border.rounded(8),
              Border.width(1),
              Border.color(color(:slate, 200)),
              Event.on_change({self(), {:bench_change, index}}),
              Event.on_focus({self(), {:bench_focus, index}}),
              Event.on_blur({self(), {:bench_blur, index}}),
              Interactive.focused([Border.color(color(:sky, 400))])
            ],
            "Field #{index}"
          ),
          Input.button(
            [
              key({:interactive_rich, :virtual_key, index}),
              width(px(70)),
              padding(8),
              Border.rounded(8),
              Border.width(1),
              Border.color(color(:slate, 200)),
              Event.virtual_key(tap: {:text_and_key, "A", :a, [:shift]})
            ],
            text("A")
          )
        ]
      )
    end)
  end

  defp nearby_hosts(count, opts) do
    count
    |> ordered_indexes(opts)
    |> Enum.map(fn index ->
      attrs =
        [
          key({:nearby_rich, :host, index}),
          width(px(150)),
          height(px(64)),
          padding(8),
          clip_nearby(),
          Background.color(color(:emerald, 50)),
          Border.rounded(10),
          Border.width(1),
          Border.color(color(:emerald, 200))
        ] ++ nearby_mount_attrs(index, Keyword.get(opts, :nearby_order, :forward))

      el(attrs, text("Host #{index}"))
    end)
  end

  defp nearby_mount_attrs(index, :reverse),
    do: index |> nearby_mount_attrs(:forward) |> Enum.reverse()

  defp nearby_mount_attrs(index, :forward) do
    [
      Nearby.above(nearby_label(index, :above, color(:sky, 100))),
      Nearby.below(nearby_label(index, :below, color(:amber, 100))),
      Nearby.on_left(nearby_label(index, :left, color(:rose, 100))),
      Nearby.on_right(nearby_label(index, :right, color(:violet, 100))),
      Nearby.behind_content(nearby_backing(index)),
      Nearby.in_front(nearby_badge(index))
    ]
  end

  defp nearby_label(index, slot, background) do
    el(
      [
        key({:nearby_rich, :nearby, index, slot}),
        width(px(74)),
        padding_xy(8, 4),
        Background.color(background),
        Border.rounded(999),
        Border.width(1),
        Border.color(color(:slate, 200))
      ],
      text(Atom.to_string(slot))
    )
  end

  defp nearby_backing(index) do
    el(
      [
        key({:nearby_rich, :nearby, index, :behind}),
        width(fill()),
        height(fill()),
        Background.color(color_rgba(16, 185, 129, 0.18)),
        Border.rounded(12)
      ],
      none()
    )
  end

  defp nearby_badge(index) do
    el(
      [
        key({:nearby_rich, :nearby, index, :front}),
        align_right(),
        align_top(),
        width(px(28)),
        height(px(20)),
        Background.color(color(:emerald, 500)),
        Border.rounded(999),
        Font.color(color(:white)),
        Font.size(11)
      ],
      text("#{rem(index, 99)}")
    )
  end

  defp nearby_code_rows(scenario_id, count, opts) do
    show_code? = nearby_code_visible?(scenario_id, opts)

    count
    |> ordered_indexes(opts)
    |> Enum.map(fn index -> nearby_code_row(scenario_id, index, show_code? && index == 1) end)
  end

  defp nearby_code_visible?(:nearby_code_show, opts),
    do: Keyword.has_key?(opts, :nearby_slot)

  defp nearby_code_visible?(:nearby_code_hide, opts),
    do: not Keyword.has_key?(opts, :nearby_slot)

  defp nearby_code_row(scenario_id, index, show_code?) do
    attrs =
      [
        key({scenario_id, :row, index}),
        width(fill()),
        height(px(58)),
        padding_xy(10, 8),
        Background.color(if(rem(index, 2) == 0, do: color(:slate, 50), else: color(:white))),
        Border.rounded(10),
        Border.width(1),
        Border.color(color(:slate, 200)),
        Border.shadow(offset: {0, 2}, blur: 8, size: 1, color: color_rgba(15, 23, 42, 0.12))
      ] ++ if(show_code?, do: [Nearby.below(nearby_code_block(scenario_id))], else: [])

    el(
      attrs,
      row([width(fill()), spacing(8)], [
        el([width(fill()), Font.weight(600)], text("Orbiting shadows #{index}")),
        el([Font.color(color(:slate, 500)), Font.size(12)], text("hover target"))
      ])
    )
  end

  defp nearby_code_block(scenario_id) do
    column(
      [
        key({scenario_id, :code_block}),
        width(px(360)),
        padding(10),
        spacing(4),
        Background.color(color(:slate, 900)),
        Border.rounded(10),
        Border.width(1),
        Border.color(color(:slate, 700)),
        Font.color(color(:slate, 50)),
        Font.size(13)
      ],
      [
        text("Border.shadow(offset: {0, 2}, blur: 8)"),
        text("Animation.animate([...], 900, :ease_in_out, :loop)"),
        text("Nearby.below(code_block)")
      ]
    )
  end

  defp media_cards(count, opts) do
    count
    |> ordered_indexes(opts)
    |> Enum.map(fn index ->
      column(
        [
          key({:media_rich, :card, index}),
          width(px(180)),
          spacing(6),
          padding(8),
          Background.image({:id, "bench-background-#{rem(index, 8)}"},
            fit: if(rem(index, 2) == 0, do: :cover, else: :contain)
          ),
          Border.rounded(10),
          Border.width(1),
          Border.color(color(:slate, 200))
        ],
        [
          image(
            [
              key({:media_rich, :image, index}),
              width(px(160)),
              height(px(72)),
              image_fit(if(rem(index, 2) == 0, do: :cover, else: :contain)),
              Border.rounded(8)
            ],
            {:id, "bench-image-#{index}"}
          ),
          row([key({:media_rich, :meta, index}), width(fill()), spacing(6)], [
            svg(
              [
                key({:media_rich, :svg, index}),
                width(px(24)),
                height(px(24)),
                Svg.color(color(:sky, 600))
              ],
              {:id, "bench-icon-#{rem(index, 5)}"}
            ),
            video(
              [
                key({:media_rich, :video, index}),
                width(px(48)),
                height(px(24)),
                image_fit(:cover)
              ],
              video_target(index)
            ),
            el([key({:media_rich, :label, index}), width(fill())], text("Media #{index}"))
          ])
        ]
      )
    end)
  end

  defp video_target(index) do
    %VideoTarget{
      id: "bench-video-#{index}",
      width: 64 + rem(index, 3) * 16,
      height: 36 + rem(index, 2) * 12,
      mode: :prime,
      ref: make_ref()
    }
  end

  defp animation_cards(count, opts) do
    count
    |> ordered_indexes(opts)
    |> Enum.map(fn index ->
      el(
        [
          key({:animation_rich, :card, index}),
          width(px(170)),
          height(px(76)),
          padding(8),
          Background.color(color(:indigo, 50)),
          Border.rounded(10),
          Border.width(1),
          Border.color(color(:indigo, 200)),
          Animation.animate(
            [
              [Transform.move_x(-2), Transform.alpha(0.78)],
              [Transform.move_x(2), Transform.alpha(1.0)]
            ],
            900 + rem(index, 5) * 100,
            :ease_in_out,
            :loop
          ),
          Animation.animate_enter(
            [
              [Transform.move_y(8), Transform.alpha(0.0)],
              [Transform.move_y(0), Transform.alpha(1.0)]
            ],
            180,
            :ease_out
          ),
          Animation.animate_exit([[Transform.alpha(1.0)], [Transform.alpha(0.0)]], 120, :linear)
        ],
        text("Animated #{index}")
      )
    end)
  end

  defp scroll_panels(count, opts) do
    count
    |> ordered_indexes(opts)
    |> Enum.map(fn index ->
      axis_attrs =
        case rem(index, 3) do
          0 -> [scrollbar_y()]
          1 -> [scrollbar_x()]
          _ -> [scrollbar_y(), scrollbar_x()]
        end

      el(
        [
          key({:scroll_rich, :panel, index}),
          width(px(880)),
          height(px(92)),
          padding(8),
          Background.color(color(:slate, 50)),
          Border.rounded(10),
          Border.width(1),
          Border.color(color(:slate, 200))
        ] ++ axis_attrs,
        scroll_content(index)
      )
    end)
  end

  defp scroll_content(index) do
    if rem(index, 3) == 1 do
      row([key({:scroll_rich, :content, index}), spacing(8)], scroll_chips(index, 12))
    else
      column([key({:scroll_rich, :content, index}), spacing(4)], scroll_lines(index, 6))
    end
  end

  defp scroll_chips(index, count) do
    for chip <- 1..count do
      el(
        [
          key({:scroll_rich, :chip, index, chip}),
          width(px(120)),
          padding_xy(8, 5),
          Background.color(color(:white)),
          Border.rounded(999),
          Border.width(1),
          Border.color(color(:slate, 200))
        ],
        text("Chip #{index}.#{chip}")
      )
    end
  end

  defp scroll_lines(index, count) do
    for line <- 1..count do
      el(
        [key({:scroll_rich, :line, index, line}), width(fill()), padding_xy(4, 2)],
        text("Scrollable line #{index}.#{line} with enough content to measure")
      )
    end
  end

  defp count_nodes(%Element{} = element) do
    1 + Enum.reduce(element.children, 0, &(&2 + count_nodes(&1))) +
      Enum.reduce(element.nearby, 0, fn {_slot, child}, acc -> acc + count_nodes(child) end)
  end

  defp count_type(%Element{} = element, type) do
    own = if element.type == type, do: 1, else: 0

    own + Enum.reduce(element.children, 0, &(&2 + count_type(&1, type))) +
      Enum.reduce(element.nearby, 0, fn {_slot, child}, acc -> acc + count_type(child, type) end)
  end

  defp element_type_counts(%Element{} = element) do
    collect_counts(element, %{}, fn element -> element.type end)
  end

  defp attr_family_counts(%Element{} = element) do
    element
    |> collect_attr_families(%{})
    |> Enum.sort_by(fn {family, _count} -> family end)
    |> Map.new()
  end

  defp collect_counts(%Element{} = element, acc, key_fun) do
    acc = Map.update(acc, key_fun.(element), 1, &(&1 + 1))

    acc = Enum.reduce(element.children, acc, &collect_counts(&1, &2, key_fun))

    Enum.reduce(element.nearby, acc, fn {_slot, child}, next_acc ->
      collect_counts(child, next_acc, key_fun)
    end)
  end

  defp collect_attr_families(%Element{} = element, acc) do
    acc =
      element.attrs
      |> Map.keys()
      |> Enum.reject(&internal_attr?/1)
      |> Enum.reduce(acc, fn attr, next_acc ->
        Map.update(next_acc, attr_family(attr), 1, &(&1 + 1))
      end)

    acc =
      if element.nearby == [],
        do: acc,
        else: Map.update(acc, :nearby, length(element.nearby), &(&1 + length(element.nearby)))

    acc = Enum.reduce(element.children, acc, &collect_attr_families(&1, &2))

    Enum.reduce(element.nearby, acc, fn {_slot, child}, next_acc ->
      collect_attr_families(child, next_acc)
    end)
  end

  defp attr_family(attr) do
    cond do
      MapSet.member?(@layout_attr_keys, attr) -> :layout
      MapSet.member?(@paint_attr_keys, attr) -> :paint
      MapSet.member?(@text_measure_attr_keys, attr) -> :measure
      MapSet.member?(@event_attr_keys, attr) -> :event
      MapSet.member?(@interaction_attr_keys, attr) -> :interaction
      MapSet.member?(@animation_attr_keys, attr) -> :animation
      true -> :other
    end
  end

  defp patch_operation({operation, _id, _attrs}) when operation in [:set_attrs, :set_children],
    do: operation

  defp patch_operation({:set_nearby_mounts, _id, _mounts}), do: :set_nearby_mounts
  defp patch_operation({:insert_subtree, _parent_id, _index, _subtree}), do: :insert_subtree

  defp patch_operation({:insert_nearby_subtree, _host_id, _index, _slot, _subtree}),
    do: :insert_nearby_subtree

  defp patch_operation({:remove, _id}), do: :remove

  defp operation_counts(patches) do
    patches
    |> Enum.map(&patch_operation/1)
    |> Enum.frequencies()
  end

  defp internal_attr?(:__attrs_hash), do: true
  defp internal_attr?(_attr), do: false

  defp expected_invalidation(_mutation, []), do: :none
  defp expected_invalidation(:noop, _patches), do: :none
  defp expected_invalidation(:paint_attr, _patches), do: :paint
  defp expected_invalidation(:layout_attr, _patches), do: :measure
  defp expected_invalidation(:text_content, _patches), do: :measure
  defp expected_invalidation(:event_attr, _patches), do: :registry
  defp expected_invalidation(:animation_attr, _patches), do: :measure
  defp expected_invalidation(:keyed_reorder, _patches), do: :structure
  defp expected_invalidation(:insert_tail, _patches), do: :structure
  defp expected_invalidation(:remove_tail, _patches), do: :structure
  defp expected_invalidation(:nearby_slot_change, _patches), do: :structure
  defp expected_invalidation(:nearby_reorder, _patches), do: :structure

  defp format_counts(counts) do
    counts
    |> Enum.sort_by(fn {key, _value} -> key end)
    |> Enum.map(fn {key, value} -> "#{key}:#{value}" end)
    |> Enum.join("|")
  end
end
