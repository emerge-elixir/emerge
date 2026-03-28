# Emerge TodoMVC Example

This example is a fuller `Emerge` + `Solve` application built around a TodoMVC-style UI.

It demonstrates:

- a `Solve` app process (`EmergeDemo.TodoApp`)
- an `Emerge` viewport process (`EmergeDemo`)
- collection controllers for per-todo editor state
- focus styling and `focus_on_mount()` for inline editing
- a realistic tree split across view and controller modules

## Run

```bash
mix deps.get
iex -S mix
```

This starts the example in dev mode with hot-code reloading enabled for files under `example/lib`.

## Configuration

The example is zero-config by default.

- renderer backend defaults to Wayland
- window title defaults to `Emerge TodoMVC`
- dev mode enables the `Emerge` code reloader for files under `example/lib`
