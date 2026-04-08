defmodule EmergeDemo.TodoApp.View do
  use Emerge.UI
  use Solve.Lookup, :helpers

  alias EmergeDemo.Todo

  # Domain composition and state wiring

  def layout() do
    column(
      [width(fill()), height(fill()), padding(16), spacing(18), Background.color(page_bg())],
      [
        title_banner(),
        todo_app_base(),
        info_footer()
      ]
    )
  end

  def title_banner do
    el([center_x(), Font.size(80), Font.color(title_color())], text("todos"))
  end

  def info_footer do
    column([width(fill()), spacing(4), center_x()], [
      info_line("Use edit, then save or cancel your changes"),
      info_line("Created with Emerge and Solve")
    ])
  end

  def todo_app_base() do
    column(
      [
        width(fill()),
        Border.rounded(2),
        card_shadow()
      ],
      [input_bar(), todo_list(), controls()]
    )
  end

  def input_bar() do
    row(
      [
        width(fill()),
        height(px(65)),
        Background.color(card_bg()),
        Border.inner_shadow(offset: {0, -1}, blur: 4, size: 0, color: color_rgba(0, 0, 0, 0.05))
      ],
      [toggle_all(), create_todo_input()]
    )
  end

  def toggle_all() do
    todo_list = solve(Todo.App, :todo_list)

    if todo_list do
      font_color = if(todo_list.all_completed?, do: todo_text(), else: toggle_off())

      Input.button(
        [
          width(px(45)),
          height(fill()),
          Font.size(24),
          Font.color(font_color),
          Event.on_press(event(todo_list, :toggle_all)),
          Interactive.mouse_over([Font.color(todo_text())])
        ] ++ transparent_surface_attrs() ++ focus_ring_attrs(),
        el([center_x(), center_y()], text("v"))
      )
    else
      el([width(px(45)), height(fill())], none())
    end
  end

  def create_todo_input() do
    create_todo = solve(Todo.App, :create_todo)

    el(
      [width(fill()), height(fill())] ++ create_todo_placeholder_attrs(create_todo),
      Input.text(
        [
          width(fill()),
          height(fill()),
          padding(16),
          Font.size(24),
          Font.color(text_main()),
          Event.on_change(event(create_todo, :set_title)),
          Event.on_key_down(:enter, event(create_todo, :submit))
        ] ++ transparent_surface_attrs() ++ focus_ring_attrs(focus_ring_color()),
        create_todo.title
      )
    )
  end

  def todo_list() do
    filter = solve(Todo.App, :filter)

    column(
      [spacing(1)],
      Enum.map(filter.visible_ids, &todo_row/1)
    )
  end

  def controls() do
    todo_list = solve(Todo.App, :todo_list)

    row(
      [
        width(fill()),
        padding(16),
        Background.color(card_bg()),
        Border.color(line())
      ],
      [
        counter_label(todo_list.active_count),
        filters(),
        clear_completed(todo_list)
      ]
    )
  end

  defp todo_row(todo_id) do
    todo_editor = solve(Todo.App, {:todo_editor, todo_id})

    if todo_editor.editing? do
      editing_row(todo_editor)
    else
      regular_row(todo_id)
    end
  end

  defp regular_row(todo_id) do
    todo = solve(Todo.App, :todo_list).todos[todo_id]

    todo_row_shell(
      [
        Animation.animate_enter(
          [
            [Transform.move_y(-100), Transform.alpha(0.1)],
            [Transform.move_y(0), Transform.alpha(1.0)]
          ],
          150,
          :ease_in
        ),
        Animation.animate_exit(
          [
            [Transform.move_x(0), Transform.alpha(1.0)],
            [Transform.move_x(300), Transform.alpha(0.1)]
          ],
          150,
          :ease_out
        ),
        key({:todo, todo_id})
      ],
      [toggle_button(todo), title_button(todo), destroy_button(todo_id)]
    )
  end

  defp editing_row(todo_editor) do
    todo_row_shell(
      [key({:todo, todo_editor.id})],
      [
        el([width(px(43)), height(px(58))], none()),
        Input.text(
          [
            focus_on_mount(),
            Event.on_change(event(todo_editor, :set_title)),
            Event.on_key_down(:enter, event(todo_editor, :save_edit)),
            Event.on_key_down(:escape, event(todo_editor, :cancel_edit)),
            Event.on_blur(event(todo_editor, :save_edit))
          ] ++ inline_editor_input_attrs() ++ focus_ring_attrs(),
          todo_editor.title
        )
      ]
    )
  end

  defp destroy_button(todo_id) do
    todo_list = solve(Todo.App, :todo_list)

    row(
      [padding_each(0, 8, 0, 0), center_y()],
      [
        action_button(
          "x",
          event(todo_list, :delete_todo, todo_id),
          destroy_color(),
          destroy_hover_color()
        )
      ]
    )
  end

  defp toggle_button(todo) do
    todo_list = solve(Todo.App, :todo_list)

    Input.button(
      [padding(12), center_y()] ++ transparent_surface_attrs() ++ press_nudge_attrs(),
      toggle_circle(todo.completed?, event(todo_list, :toggle_todo, todo.id))
    )
  end

  defp title_button(todo) do
    todo_editor = solve(Todo.App, {:todo_editor, todo.id})

    Input.button(
      [
        width(fill()),
        padding_each(15, 0, 15, 15),
        Background.color(card_bg()),
        Event.on_press(event(todo_editor, :begin_edit)),
        Interactive.mouse_over([Background.color(color(:gray, 100))])
      ] ++ transparent_border_attrs() ++ focus_ring_attrs() ++ title_text_attrs(todo),
      paragraph([width(fill()), Font.align_left()], [text(todo.title)])
    )
  end

  defp filters() do
    filter = solve(Todo.App, :filter)

    row(
      [center_x(), spacing(6)],
      Enum.map(filter.filters, &filter_button(&1, filter))
    )
  end

  defp filter_button(filter_name, filter) do
    selected? = filter_name == filter.active

    selection_chip(
      selected?,
      Todo.Filter.label(filter_name),
      event(filter, :set, filter_name)
    )
  end

  defp counter_label(active_count) do
    row([spacing(4), center_y()], [
      el(
        [Font.size(14), Font.color(muted_text()), Font.bold()],
        text(Integer.to_string(active_count))
      ),
      el(
        [Font.size(14), Font.color(muted_text())],
        text(if(active_count == 1, do: "item left", else: "items left"))
      )
    ])
  end

  defp clear_completed(%{has_completed?: false}), do: none()

  defp clear_completed(todo_list) do
    Input.button(
      [
        center_y(),
        align_right(),
        Font.size(14),
        Font.color(muted_text()),
        Event.on_press(event(todo_list, :clear_completed)),
        Interactive.mouse_over([Font.underline()])
      ] ++ transparent_surface_attrs() ++ focus_ring_attrs() ++ press_nudge_attrs(),
      text("Clear completed")
    )
  end

  defp create_todo_placeholder_attrs(%{title: ""}) do
    [Nearby.behind_content(placeholder_overlay("What needs to be done?"))]
  end

  defp create_todo_placeholder_attrs(_create_todo), do: []

  defp title_text_attrs(%{completed?: true}) do
    [Font.size(24), Font.color(todo_completed()), Font.strike()]
  end

  defp title_text_attrs(%{completed?: false}) do
    [Font.size(24), Font.color(todo_text())]
  end

  # Generic elements

  defp info_line(content) do
    el([Font.size(11), Font.color(muted_text())], text(content))
  end

  defp placeholder_overlay(content) do
    el(
      [
        padding(16),
        center_y(),
        Font.size(24),
        Font.color(placeholder_text()),
        Font.italic()
      ],
      text(content)
    )
  end

  defp todo_row_shell(attrs, children) do
    row(attrs ++ todo_row_surface_attrs(), children)
  end

  defp toggle_circle(completed?, on_press) do
    el(
      [
        width(px(28)),
        height(px(28)),
        center_x(),
        center_y(),
        Border.rounded(999),
        Border.width(1),
        Border.color(if(completed?, do: toggle_on(), else: toggle_off())),
        Background.color(if(completed?, do: toggle_on_fill(), else: transparent_color())),
        Event.on_press(on_press),
        Interactive.mouse_over([Border.glow(focus_glow(), 2)]),
        Font.size(16),
        Font.color(toggle_on()),
        Font.center()
      ] ++ focus_ring_attrs(),
      el(
        [Transform.move_y(-1), Transform.move_x(0.5)],
        text(if(completed?, do: "x", else: ""))
      )
    )
  end

  defp action_button(label, on_press, base_color, hover_color) do
    Input.button(
      [
        width(px(58)),
        height(px(58)),
        Font.size(13),
        Font.color(base_color),
        Event.on_press(on_press),
        Interactive.mouse_over([Background.color(color(:gray, 100)), Font.color(hover_color)])
      ] ++ transparent_surface_attrs() ++ focus_ring_attrs() ++ press_nudge_attrs(),
      el([center_x(), center_y()], text(label))
    )
  end

  defp selection_chip(selected?, label, on_press) do
    border_color = if(selected?, do: filter_selected_color(), else: transparent_color())

    Input.button(
      [
        padding_xy(7, 3),
        Font.size(14),
        Font.color(muted_text()),
        Border.rounded(3),
        Border.width(1),
        Border.color(border_color),
        Event.on_press(on_press),
        Interactive.mouse_over([
          Border.color(if(selected?, do: filter_selected_color(), else: filter_hover_color()))
        ])
      ] ++ transparent_background_attrs() ++ focus_ring_attrs() ++ press_nudge_attrs(),
      text(label)
    )
  end

  # Reusable attribute bundles and palette

  defp transparent_surface_attrs do
    transparent_background_attrs() ++ transparent_border_attrs()
  end

  defp transparent_background_attrs do
    [Background.color(transparent_color())]
  end

  defp transparent_border_attrs do
    [Border.width(1), Border.color(transparent_color())]
  end

  defp focus_ring_attrs(glow_color \\ focus_glow()) do
    [
      Interactive.focused([
        Border.color(focus_ring_color()),
        Border.glow(glow_color, 2)
      ])
    ]
  end

  defp press_nudge_attrs do
    [Interactive.mouse_down([Transform.move_y(1)])]
  end

  defp todo_row_surface_attrs do
    [
      width(fill()),
      center_y(),
      Border.width_each(0, 0, 1, 0),
      Border.color(row_line()),
      Background.color(card_bg())
    ]
  end

  defp inline_editor_input_attrs do
    [
      width(fill()),
      padding_each(12, 16, 12, 16),
      Font.size(24),
      Font.color(todo_text()),
      Background.color(card_bg()),
      Border.width(1),
      Border.color(color_rgb(153, 153, 153)),
      Border.inner_shadow(offset: {0, -1}, blur: 5, size: 0, color: color_rgba(0, 0, 0, 0.18))
    ]
  end

  defp card_shadow do
    Border.shadow(offset: {0, 16}, blur: 40, size: 0, color: color_rgba(0, 0, 0, 0.12))
  end

  defp page_bg, do: color_rgb(245, 245, 245)
  defp card_bg, do: color_rgb(255, 255, 255)
  defp title_color, do: color_rgba(175, 47, 47, 0.22)
  defp text_main, do: color_rgb(17, 17, 17)
  defp todo_text, do: color_rgb(72, 72, 72)
  defp todo_completed, do: color_rgb(148, 148, 148)
  defp muted_text, do: color_rgb(77, 77, 77)
  defp placeholder_text, do: color_rgba(0, 0, 0, 0.4)
  defp line, do: color_rgb(230, 230, 230)
  defp row_line, do: color_rgb(237, 237, 237)
  defp toggle_off, do: color_rgb(148, 148, 148)
  defp toggle_on, do: color_rgb(89, 161, 147)
  defp toggle_on_fill, do: color_rgba(62, 163, 144, 0.12)
  defp destroy_color, do: color_rgb(148, 148, 148)
  defp destroy_hover_color, do: color_rgb(193, 133, 133)
  defp filter_hover_color, do: color_rgb(219, 118, 118)
  defp filter_selected_color, do: color_rgb(206, 70, 70)
  defp focus_ring_color, do: color_rgb(207, 125, 125)
  defp focus_glow, do: color_rgba(207, 125, 125, 0.28)
  defp transparent_color, do: color_rgba(255, 255, 255, 0.0)
end
