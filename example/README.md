# Emerge Counter Example

This example shows a minimal app tree with:

- a `Solve` app process (`EmergeDemo.State`)
- an `Emerge.Viewport` process (`EmergeDemo`)

The viewport renders a counter UI and dispatches button events through
`Solve.Lookup` event refs.

## Run

```bash
mix deps.get
mix run --no-halt
```

## Configuration

The example is zero-config.

- renderer backend defaults to Wayland
- window title defaults to `Emerge Demo`
