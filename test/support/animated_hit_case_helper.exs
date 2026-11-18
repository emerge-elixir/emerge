defmodule EmergeSkia.TestSupport.AnimatedHitCase do
  use Emerge.UI

  def probe(:newly_occupied_outside_host), do: {130, 41}

  def tree do
    el(
      [
        key(:host),
        width(px(128)),
        height(px(82)),
        Nearby.in_front(target()),
        Background.color(:gray)
      ],
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
        Event.on_mouse_enter(:target_enter),
        Event.on_mouse_leave(:target_leave),
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
              el(
                [center_x(), align_bottom(), Transform.move_y(-8), Font.size(9)],
                text("Original slot")
              )
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
end
