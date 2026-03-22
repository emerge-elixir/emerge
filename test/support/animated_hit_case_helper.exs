defmodule EmergeSkia.TestSupport.AnimatedHitCase do
  use Emerge.UI

  alias Emerge.Engine.Element

  @sample_times_ms Enum.to_list(0..1400//50)
  @probes %{
    stable_inside: {48.0, 41.0},
    newly_occupied_inside_host: {110.0, 41.0},
    newly_occupied_outside_host: {130.0, 41.0},
    stable_outside: {175.0, 41.0}
  }

  def tree do
    el(
      [key(:host), width(px(128)), height(px(82)), Nearby.in_front(target()), Background.color(:gray)],
      underlying()
    )
  end

  def placeholder_tree do
    el(
      [
        key(:host),
        width(px(128)),
        height(px(82)),
        Nearby.in_front(placeholder_target()),
        Background.color(:gray)
      ],
      underlying()
    )
  end

  def replaced_root_placeholder_tree do
    el(
      [
        key(:placeholder_host),
        width(px(128)),
        height(px(82)),
        Background.color(:gray)
      ],
      underlying()
    )
  end

  def probe(label), do: Map.fetch!(@probes, label)

  def sample_times_ms, do: @sample_times_ms

  def loop_sample_times_ms(loop_count) when is_integer(loop_count) and loop_count >= 1 do
    duration_ms = 1400
    Enum.to_list(0..(duration_ms * loop_count)//50)
  end

  def expected_first_activation_ms(:newly_occupied_inside_host), do: 550
  def expected_first_activation_ms(:newly_occupied_outside_host), do: 700
  def expected_first_activation_ms(_label), do: nil

  def target_id_bin(%Element{} = assigned_tree) do
    target_id_from_tree(assigned_tree) |> :erlang.term_to_binary()
  end

  def target_id_bin_from_state(%Emerge.Engine.DiffState{event_registry: event_registry}) do
    Enum.find_value(event_registry, fn {id_bin, events} ->
      case Map.get(events, :mouse_move) do
        {_pid, {:probe, :target}} -> id_bin
        _ -> nil
      end
    end) || raise "could not find target element id in event registry"
  end

  def host_id_bin(%Element{} = assigned_tree) do
    host_id_from_tree(assigned_tree) |> :erlang.term_to_binary()
  end

  def host_id_bin_for_target(%Element{} = assigned_tree, target_id_bin)
      when is_binary(target_id_bin) do
    target_id = :erlang.binary_to_term(target_id_bin)
    host_id_from_tree(assigned_tree, target_id) |> :erlang.term_to_binary()
  end

  def page_switch_tree do
    column([key(:root), width(fill()), height(fill())], [
      el([key(:page_animation), width(px(128)), height(px(82))], animation_host())
    ])
  end

  def page_switch_placeholder_tree do
    column([key(:root), width(fill()), height(fill())], [
      el([key(:page_overview), width(px(128)), height(px(82)), Background.color(:gray)], none())
    ])
  end

  def demo_like_tree do
    demo_like_tree_with_log("Animation")
  end

  def demo_like_tree_with_log(log_text) when is_binary(log_text) do
    el(
      [key(:content_panel), width(px(320)), height(px(260)), padding(16), scrollbar_y()],
      column([width(fill()), spacing(16)], [
        el([key(:page_animation_wrapper)], text(log_text)),
        wrapped_row([key(:showcase_row), width(fill()), spacing_xy(14, 14)], [
          demo_like_showcase()
        ])
      ])
    )
  end

  def demo_like_page_switch_placeholder_tree do
    el(
      [key(:content_panel), width(px(320)), height(px(260)), padding(16), scrollbar_y()],
      column([width(fill()), spacing(16)], [
        el([key(:page_overview_wrapper)], text("Overview")),
        el(
          [key(:placeholder_card), width(px(220)), height(px(120)), Background.color(:gray)],
          none()
        )
      ])
    )
  end

  def demo_like_full_row_tree do
    el(
      [key(:content_panel), width(px(760)), height(px(260)), padding(16), scrollbar_y()],
      column([width(fill()), spacing(16)], [
        el([key(:page_animation_wrapper)], text("Animation")),
        wrapped_row([key(:showcase_row), width(fill()), spacing_xy(14, 14)], [
          demo_like_showcase(),
          secondary_hover_showcase(),
          secondary_press_showcase()
        ])
      ])
    )
  end

  def demo_like_full_row_placeholder_tree do
    el(
      [key(:content_panel), width(px(760)), height(px(260)), padding(16), scrollbar_y()],
      column([width(fill()), spacing(16)], [
        el([key(:page_overview_wrapper)], text("Overview")),
        el(
          [key(:placeholder_card), width(px(220)), height(px(120)), Background.color(:gray)],
          none()
        )
      ])
    )
  end

  defp underlying do
    el(
      [
        key(:underlying),
        width(fill()),
        height(fill()),
        Event.on_mouse_move({self(), {:probe, :underlying}}),
        Background.color({:color_rgb, {40, 50, 70}})
      ],
      none()
    )
  end

  defp target do
    target_common_attrs([
      Animation.animate(
        [
          [width(px(96)), Transform.move_x(-16)],
          [width(px(156)), Transform.move_x(26)]
        ],
        1400,
        :ease_in_out,
        :loop
      )
    ])
  end

  defp placeholder_target do
    target_common_attrs([width(px(96)), Transform.move_x(-16)])
  end

  defp target_common_attrs(extra_attrs) do
    el(
      [
        key(:target),
        height(px(82)),
        center_x(),
        center_y(),
        Background.color({:color_rgb, {70, 96, 148}}),
        Interactive.mouse_over([
          Background.color({:color_rgb, {96, 132, 188}}),
          Font.color(:white)
        ]),
        Event.on_mouse_move({self(), {:probe, :target}})
      ] ++ extra_attrs,
      none()
    )
  end

  defp animation_host do
    el(
      [
        width(px(128)),
        height(px(82)),
        Nearby.in_front(target()),
        Background.color(:gray)
      ],
      underlying()
    )
  end

  defp demo_like_showcase do
    column([key(:showcase), width(px(238)), spacing(8)], [
      el([Font.size(13), Font.color(:white)], text("Animated Width + Move")),
      el([Font.size(11)], text("demo-like nesting")),
      el(
        [width(fill()), padding(12), Background.color({:color_rgb, {44, 44, 66}})],
        column([spacing(8)], [
          el(
            [
              width(fill()),
              height(px(150)),
              Background.color({:color_rgba, {255, 255, 255, 12}})
            ],
            el(
              [
                key(:host_slot),
                width(px(128)),
                height(px(82)),
                center_x(),
                center_y(),
                Background.color({:color_rgba, {255, 255, 255, 8}}),
                Border.width(1),
                Border.color({:color_rgba, {208, 216, 240, 110}}),
                Border.rounded(12),
                Nearby.in_front(target())
              ],
              el([center_x(), align_bottom(), Transform.move_y(-8), Font.size(9)], text("Original slot"))
            )
          )
        ])
      )
    ])
  end

  defp secondary_hover_showcase do
    column([key(:showcase_hover), width(px(238)), spacing(8)], [
      el([Font.size(13), Font.color(:white)], text("Animated Padding + Rotate")),
      el([Font.size(11)], text("secondary hover card")),
      el(
        [width(fill()), padding(12), Background.color({:color_rgb, {44, 44, 66}})],
        column([spacing(8)], [
          el(
            [
              width(fill()),
              height(px(150)),
              Background.color({:color_rgba, {255, 255, 255, 12}})
            ],
            el(
              [
                key(:hover_host_slot),
                width(px(128)),
                height(px(82)),
                center_x(),
                center_y(),
                Background.color({:color_rgba, {255, 255, 255, 8}}),
                Border.width(1),
                Border.color({:color_rgba, {208, 216, 240, 110}}),
                Border.rounded(12),
                Nearby.in_front(secondary_hover_target())
              ],
              none()
            )
          )
        ])
      )
    ])
  end

  defp secondary_press_showcase do
    column([key(:showcase_press), width(px(238)), spacing(8)], [
      el([Font.size(13), Font.color(:white)], text("Animated Height + Scale Press")),
      el([Font.size(11)], text("secondary press card")),
      el(
        [width(fill()), padding(12), Background.color({:color_rgb, {44, 44, 66}})],
        column([spacing(8)], [
          el(
            [
              width(fill()),
              height(px(150)),
              Background.color({:color_rgba, {255, 255, 255, 12}})
            ],
            el(
              [
                key(:press_host_slot),
                width(px(128)),
                height(px(82)),
                center_x(),
                center_y(),
                Background.color({:color_rgba, {255, 255, 255, 8}}),
                Border.width(1),
                Border.color({:color_rgba, {208, 216, 240, 110}}),
                Border.rounded(12),
                Nearby.in_front(secondary_press_target())
              ],
              none()
            )
          )
        ])
      )
    ])
  end

  defp secondary_hover_target do
    el(
      [
        key(:target_hover),
        height(px(82)),
        center_x(),
        center_y(),
        Background.color({:color_rgb, {102, 76, 130}}),
        Interactive.mouse_over([
          Background.color({:color_rgb, {130, 96, 166}}),
          Font.color(:white)
        ]),
        Event.on_mouse_enter({self(), {:probe, :hover_enter}}),
        Event.on_mouse_leave({self(), {:probe, :hover_leave}}),
        Animation.animate(
          [
            [padding_each(10, 12, 10, 12), Transform.rotate(-10)],
            [padding_each(18, 24, 18, 24), Transform.rotate(10)]
          ],
          1600,
          :ease_in_out,
          :loop
        )
      ],
      none()
    )
  end

  defp secondary_press_target do
    el(
      [
        key(:target_press),
        height(px(82)),
        center_x(),
        center_y(),
        Background.color({:color_rgb, {86, 104, 78}}),
        Interactive.mouse_over([
          Background.color({:color_rgb, {106, 126, 98}}),
          Font.color(:white)
        ]),
        Event.on_mouse_down({self(), {:probe, :press_down}}),
        Event.on_mouse_up({self(), {:probe, :press_up}}),
        Animation.animate(
          [
            [height(px(70)), Transform.scale(0.94)],
            [height(px(108)), Transform.scale(1.08)]
          ],
          1200,
          :ease_in_out,
          :loop
        )
      ],
      none()
    )
  end

  defp target_id_from_tree(%Element{id: id, attrs: %{on_mouse_move: {_pid, {:probe, :target}}}}),
    do: id

  defp target_id_from_tree(%Element{children: children, attrs: attrs}) do
    Enum.find_value(children, &target_id_from_tree/1) ||
      Enum.find_value([:behind, :above, :on_right, :below, :on_left, :in_front], fn slot ->
        case Map.get(attrs, slot) do
          %Element{} = child -> target_id_from_tree(child)
          _ -> nil
        end
      end) ||
      raise "could not find target element in assigned tree"
  end

  defp host_id_from_tree(%Element{id: id, children: children, attrs: attrs}) do
    if match?(
         %Element{attrs: %{on_mouse_move: {_pid, {:probe, :target}}}},
         Map.get(attrs, :in_front)
       ) do
      id
    else
      Enum.find_value(children, &host_id_from_tree/1) ||
        Enum.find_value([:behind, :above, :on_right, :below, :on_left, :in_front], fn slot ->
          case Map.get(attrs, slot) do
            %Element{} = child -> host_id_from_tree(child)
            _ -> nil
          end
        end) ||
        raise "could not find host slot element in assigned tree"
    end
  end

  defp host_id_from_tree(
         %Element{id: id, attrs: %{in_front: %Element{id: target_id}}},
         target_id
       ),
       do: id

  defp host_id_from_tree(%Element{children: children, attrs: attrs}, target_id) do
    Enum.find_value(children, &host_id_from_tree(&1, target_id)) ||
      Enum.find_value([:behind, :above, :on_right, :below, :on_left, :in_front], fn slot ->
        case Map.get(attrs, slot) do
          %Element{} = child -> host_id_from_tree(child, target_id)
          _ -> nil
        end
      end) ||
      raise "could not find host slot element for target id in assigned tree"
  end
end
