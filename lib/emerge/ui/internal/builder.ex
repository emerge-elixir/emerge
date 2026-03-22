defmodule Emerge.UI.Internal.Builder do
  @moduledoc false

  alias Emerge.Engine.Element
  alias Emerge.Engine.Tree.Attrs, as: TreeAttrs
  alias Emerge.UI.Internal.Validation

  @spec build_element(map(), atom(), [Element.t()]) :: Element.t()
  def build_element(attrs, type, children) when is_map(attrs) do
    {key, attrs} = Map.pop(attrs, :key)
    attrs = Map.put(attrs, :__attrs_hash, TreeAttrs.attrs_hash(attrs))

    %Element{
      type: type,
      id: key,
      attrs: attrs,
      children: children
    }
  end

  @spec prepare_attrs!(String.t(), list(), keyword()) :: map()
  def prepare_attrs!(function_name, attrs, opts \\ []) do
    attrs = Validation.validate_attrs_list!(function_name, attrs)
    Validation.parse_attrs(attrs, function_name, opts)
  end

  @spec prepare_single_child!(String.t(), list(), Element.t()) :: {map(), Element.t()}
  def prepare_single_child!(function_name, attrs, child) do
    attrs = Validation.validate_attrs_list!(function_name, attrs)
    child = Validation.validate_child_element!(function_name, child)

    {Validation.parse_attrs(attrs, function_name), child}
  end

  @spec prepare_children!(String.t(), list(), [Element.t()]) :: {map(), [Element.t()]}
  def prepare_children!(function_name, attrs, children) do
    attrs = Validation.validate_attrs_list!(function_name, attrs)
    children = Validation.validate_children_list!(function_name, children)

    {Validation.parse_attrs(attrs, function_name), children}
  end
end
