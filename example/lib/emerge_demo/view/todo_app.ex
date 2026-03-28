defmodule EmergeDemo.View.TodoApp do
  use Emerge.UI
  use Solve.Lookup, :helpers

  alias EmergeDemo.TodoApp

  @page_bg color_rgb(245, 245, 245)
  @card_bg color_rgb(255, 255, 255)
  @title_color color_rgba(175, 47, 47, 0.22)
  @text_main color_rgb(17, 17, 17)
  @todo_text color_rgb(72, 72, 72)
  @todo_completed color_rgb(148, 148, 148)
  @muted_text color_rgb(77, 77, 77)
  @placeholder_text color_rgba(0, 0, 0, 0.4)
  @line color_rgb(230, 230, 230)
  @row_line color_rgb(237, 237, 237)
  @toggle_off color_rgb(148, 148, 148)
  @toggle_on color_rgb(89, 161, 147)
  @toggle_on_fill color_rgba(62, 163, 144, 0.12)
  @destroy_color color_rgb(148, 148, 148)
  @destroy_hover color_rgb(193, 133, 133)
  @filter_hover color_rgb(219, 118, 118)
  @filter_selected color_rgb(206, 70, 70)
  @focus_ring color_rgb(207, 125, 125)

  def layout() do
    column(
      [width(fill()), height(fill()), padding(16), spacing(18), Background.color(@page_bg)],
      [
        title_banner(),
        todo_app_base(),
        info_footer()
      ]
    )
  end

  def title_banner do
    el([center_x(), Font.size(80), Font.color(@title_color)], text("todos"))
  end

  def info_footer do
    column([width(fill()), spacing(4), center_x()], [
      info_line("Use edit, then save or cancel your changes"),
      info_line("Created with Emerge and Solve")
    ])
  end

  def info_line(content) do
    el([Font.size(11), Font.color(@muted_text)], text(content))
  end

  def todo_app_base() do
    column(
      [
        width(fill()),
        Background.color(@card_bg),
        Border.rounded(2),
        Border.shadow(offset: {0, 16}, blur: 40, size: 0, color: color_rgba(0, 0, 0, 0.12))
      ],
      [input_bar(), todo_list(), controls()]
    )
  end

  def input_bar() do
    row(
      [
        width(fill()),
        height(px(65)),
        Background.color(color_rgba(0, 0, 0, 0.01)),
        Border.inner_shadow(offset: {0, -1}, blur: 4, size: 0, color: color_rgba(0, 0, 0, 0.05))
      ],
      [
        toggle_all(),
        create_todo_input()
      ]
    )
  end

  def toggle_all() do
    todo_list = solve(TodoApp, :todo_list)

    if todo_list do
      font_color = if(todo_list.all_completed?, do: color_rgb(72, 72, 72), else: @toggle_off)

      Input.button(
        [
          width(px(45)),
          height(fill()),
          Font.size(24),
          Font.color(font_color),
          Background.color(color_rgba(255, 255, 255, 0.0)),
          Border.width(1),
          Border.color(color_rgba(255, 255, 255, 0.0)),
          Interactive.mouse_over([Font.color(color_rgb(72, 72, 72))]),
          Interactive.focused([
            Border.color(@focus_ring),
            Border.glow(color_rgba(207, 125, 125, 0.28), 2)
          ]),
          Event.on_press(event(todo_list, :toggle_all))
        ],
        el([center_x(), center_y()], text("v"))
      )
    else
      el([width(px(45)), height(fill())], none())
    end
  end

  def create_todo_input() do
    create_todo = solve(TodoApp, :create_todo)

    el(
      [width(fill()), height(fill())] ++ create_todo_placeholder_attrs(create_todo),
      Input.text(
        [
          width(fill()),
          height(fill()),
          padding(16),
          Font.size(24),
          Font.color(@text_main),
          Background.color(color_rgba(255, 255, 255, 0.0)),
          Border.width(1),
          Border.color(color_rgba(255, 255, 255, 0.0)),
          Event.on_change(event(create_todo, :set_title)),
          Event.on_key_down(:enter, event(create_todo, :submit)),
          Interactive.focused([
            Border.color(@focus_ring),
            Border.glow(color_rgb(207, 125, 125), 2)
          ])
        ],
        create_todo.title
      )
    )
  end

  defp create_todo_placeholder_attrs(%{title: ""}) do
    [
      Emerge.UI.Nearby.behind_content(
        el(
          [
            padding(16),
            center_y(),
            Font.size(24),
            Font.color(@placeholder_text),
            Font.italic()
          ],
          text("What needs to be done?")
        )
      )
    ]
  end

  defp create_todo_placeholder_attrs(_create_todo), do: []

  def todo_list() do
    filter = solve(TodoApp, :filter)

    column(
      [width(fill()), Border.width_each(1, 0, 0, 0), Border.color(@line)],
      Enum.map(filter.visible_ids, &todo_row/1)
    )
  end

  defp todo_row(todo_id) do
    todo_editor = solve(TodoApp, {:todo_editor, todo_id})

    if todo_editor.editing? do
      editing_row(todo_editor)
    else
      regular_row(todo_id)
    end
  end

  defp regular_row(todo_id) do
    todo = solve(TodoApp, :todo_list).todos[todo_id]

    row(
      [
        key({:todo, todo_id}),
        width(fill()),
        center_y(),
        Border.width_each(0, 0, 1, 0),
        Border.color(@row_line),
        Background.color(@card_bg),
        Interactive.mouse_over([Background.color(color_rgba(0, 0, 0, 0.01))])
      ],
      [toggle_button(todo), title_button(todo), destroy_button(todo_id)]
    )
  end

  defp destroy_button(todo_id) do
    todo_list = solve(TodoApp, :todo_list)

    row(
      [padding_each(0, 8, 0, 0), center_y()],
      [
        action_button(
          "x",
          event(todo_list, :delete_todo, todo_id),
          @destroy_color,
          @destroy_hover
        )
      ]
    )
  end

  defp editing_row(todo_editor) do
    row(
      [
        key({:todo, todo_editor.id}),
        width(fill()),
        center_y(),
        Border.width_each(0, 0, 1, 0),
        Border.color(@row_line),
        Background.color(@card_bg)
      ],
      [
        el([width(px(43)), height(px(58))], none()),
        Input.text(
          [
            focus_on_mount(),
            width(fill()),
            padding_each(12, 16, 12, 16),
            Font.size(24),
            Font.color(@todo_text),
            Background.color(@card_bg),
            Border.width(1),
            Border.color(color_rgb(153, 153, 153)),
            Border.inner_shadow(
              offset: {0, -1},
              blur: 5,
              size: 0,
              color: color_rgba(0, 0, 0, 0.18)
            ),
            Event.on_change(event(todo_editor, :set_title)),
            Event.on_key_down(:enter, event(todo_editor, :save_edit)),
            Event.on_key_down(:escape, event(todo_editor, :cancel_edit)),
            Event.on_blur(event(todo_editor, :save_edit)),
            Interactive.focused([
              Border.color(@focus_ring),
              Border.glow(color_rgba(207, 125, 125, 0.28), 2)
            ])
          ],
          todo_editor.title
        )
      ]
    )
  end

  defp toggle_button(todo) do
    todo_list = solve(TodoApp, :todo_list)

    Input.button(
      [
        width(px(45)),
        height(px(58)),
        Background.color(color_rgba(255, 255, 255, 0.0)),
        Border.width(1),
        Border.color(color_rgba(255, 255, 255, 0.0)),
        Event.on_press(event(todo_list, :toggle_todo, todo.id)),
        Interactive.focused([
          Border.color(@focus_ring),
          Border.glow(color_rgba(207, 125, 125, 0.28), 2)
        ]),
        Interactive.mouse_down([Transform.move_y(1)])
      ],
      el(
        [
          width(px(28)),
          height(px(28)),
          center_x(),
          center_y(),
          Border.rounded(999),
          Border.width(1),
          Border.color(if(todo.completed?, do: @toggle_on, else: @toggle_off)),
          Background.color(
            if(todo.completed?, do: @toggle_on_fill, else: color_rgba(255, 255, 255, 0.0))
          ),
          Font.size(16),
          Font.color(@toggle_on),
          Font.center()
        ],
        el(
          [Transform.move_y(-1), Transform.move_x(0.5)],
          text(if(todo.completed?, do: "x", else: ""))
        )
      )
    )
  end

  defp title_button(todo) do
    todo_editor = solve(TodoApp, {:todo_editor, todo.id})

    Input.button(
      [
        width(fill()),
        padding_each(15, 0, 15, 15),
        Background.color(color_rgba(255, 255, 255, 0.0)),
        Border.width(1),
        Border.color(color_rgba(255, 255, 255, 0.0)),
        Border.rounded(3),
        Event.on_press(event(todo_editor, :begin_edit)),
        Interactive.focused([
          Border.color(@focus_ring),
          Border.glow(color_rgba(207, 125, 125, 0.28), 2)
        ])
      ] ++ title_text_attrs(todo),
      paragraph([width(fill()), Font.align_left()], [text(todo.title)])
    )
  end

  defp action_button(label, message, base_color, hover_color) do
    Input.button(
      [
        width(px(58)),
        height(px(58)),
        Font.size(13),
        Font.color(base_color),
        Background.color(color_rgba(255, 255, 255, 0.0)),
        Border.width(1),
        Border.color(color_rgba(255, 255, 255, 0.0)),
        Event.on_press(message),
        Interactive.mouse_over([Font.color(hover_color)]),
        Interactive.focused([
          Border.color(@focus_ring),
          Border.glow(color_rgba(207, 125, 125, 0.28), 2)
        ]),
        Interactive.mouse_down([Transform.move_y(1)])
      ],
      el([center_x(), center_y()], text(label))
    )
  end

  defp title_text_attrs(%{completed?: true}) do
    [Font.size(24), Font.color(@todo_completed), Font.strike()]
  end

  defp title_text_attrs(%{completed?: false}) do
    [Font.size(24), Font.color(@todo_text)]
  end

  def controls() do
    todo_list = solve(TodoApp, :todo_list)

    row(
      [
        width(fill()),
        padding(16),
        Border.color(@line)
      ],
      [
        counter_label(todo_list.active_count),
        filters(),
        clear_completed(todo_list)
      ]
    )
  end

  defp filters() do
    filter = solve(TodoApp, :filter)

    row(
      [center_x(), spacing(6)],
      Enum.map(filter.filters, &filter_button(&1, filter))
    )
  end

  defp filter_button(filter_name, filter) do
    selected? = filter_name == filter.active

    border_color = if selected?, do: @filter_selected, else: color_rgba(255, 255, 255, 0.0)

    Input.button(
      [
        padding_xy(7, 3),
        Font.size(14),
        Font.color(@muted_text),
        Background.color(color_rgba(255, 255, 255, 0.0)),
        Border.rounded(3),
        Border.width(1),
        Border.color(border_color),
        Event.on_press(event(filter, :set, filter_name)),
        Interactive.mouse_over([
          Border.color(if(selected?, do: @filter_selected, else: @filter_hover))
        ]),
        Interactive.focused([
          Border.color(@focus_ring),
          Border.glow(color_rgba(207, 125, 125, 0.28), 2)
        ]),
        Interactive.mouse_down([Transform.move_y(1)])
      ],
      text(TodoApp.Filter.label(filter_name))
    )
  end

  defp counter_label(active_count) do
    row([spacing(4), center_y()], [
      el(
        [Font.size(14), Font.color(@muted_text), Font.bold()],
        text(Integer.to_string(active_count))
      ),
      el(
        [Font.size(14), Font.color(@muted_text)],
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
        Font.color(@muted_text),
        Background.color(color_rgba(255, 255, 255, 0.0)),
        Border.width(1),
        Border.color(color_rgba(255, 255, 255, 0.0)),
        Event.on_press(event(todo_list, :clear_completed)),
        Interactive.mouse_over([Font.underline()]),
        Interactive.focused([
          Border.color(@focus_ring),
          Border.glow(color_rgba(207, 125, 125, 0.28), 2)
        ]),
        Interactive.mouse_down([Transform.move_y(1)])
      ],
      text("Clear completed")
    )
  end
end
