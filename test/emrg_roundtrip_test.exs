defmodule EmergeSkia.EmrgRoundtripTest do
  use ExUnit.Case

  import Emerge.UI

  defp normalize_tree(%Emerge.Element{} = element) do
    %{
      type: element.type,
      id: element.id,
      attrs: normalize_attrs(element.attrs),
      children: Enum.map(element.children, &normalize_tree/1)
    }
  end

  defp normalize_attrs(attrs) do
    attrs
    |> Map.drop([:__attrs_hash])
    |> Enum.map(fn {key, value} -> {key, normalize_value(value)} end)
    |> Map.new()
  end

  defp normalize_value({:gradient, from, to, angle}) do
    {:gradient, normalize_value(from), normalize_value(to), normalize_number(angle)}
  end

  defp normalize_value({:color_rgb, _} = color), do: color
  defp normalize_value({:color_rgba, _} = color), do: color
  defp normalize_value(value) when is_number(value), do: normalize_number(value)
  defp normalize_value(value), do: value

  defp normalize_number(value) when is_integer(value), do: value * 1.0
  defp normalize_number(value) when is_float(value), do: value

  test "EMRG roundtrip through Rust preserves tree" do
    tree =
      column([width(:fill), height(:fill), padding(20), spacing(12), Emerge.UI.Background.color(:black)], [
        el([padding(10), Emerge.UI.Background.color({:color_rgba, {20, 30, 40, 255}}), Emerge.UI.Border.rounded(8)],
          el([Emerge.UI.Font.size(18), Emerge.UI.Font.color(:white)], text("Hello"))
        ),
        row([spacing(8)], [
          el([padding(6), Emerge.UI.Background.color(:red)], text("A")),
          el([padding(6), Emerge.UI.Background.color(:green)], text("B")),
          el([padding(6), Emerge.UI.Background.color(:blue)], text("C"))
        ])
      ])

    {_vdom, assigned} = Emerge.Reconcile.assign_ids(tree)
    encoded = Emerge.Serialization.encode_tree(assigned)

    roundtrip =
      case EmergeSkia.Native.tree_roundtrip(encoded) do
        bin when is_binary(bin) -> bin
        {:ok, bin} when is_binary(bin) -> bin
        {:error, reason} -> flunk("tree_roundtrip failed: #{reason}")
        other -> flunk("unexpected tree_roundtrip result: #{inspect(other)}")
      end

    decoded = Emerge.Serialization.decode(roundtrip)

    assert normalize_tree(decoded) == normalize_tree(assigned)
  end
end
