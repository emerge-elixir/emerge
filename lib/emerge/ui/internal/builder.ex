defmodule Emerge.UI.Internal.Builder do
  @moduledoc false

  alias Emerge.Engine.Element
  alias Emerge.Engine.Tree.Attrs, as: TreeAttrs
  alias Emerge.UI.Internal.Validation

  @type attrs_map :: map()
  @type attrs_owner :: String.t()
  @type attrs_options :: keyword()
  @type child :: Element.t()
  @type children :: [Element.t()]

  @spec build_element(attrs_map(), atom(), children()) :: Element.t()
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

  @spec prepare_attrs!(attrs_owner(), Emerge.UI.attrs()) :: attrs_map()
  @spec prepare_attrs!(attrs_owner(), Emerge.UI.attrs(), attrs_options()) :: attrs_map()
  def prepare_attrs!(function_name, attrs, opts \\ []) do
    attrs = Validation.validate_attrs_list!(function_name, attrs)
    Validation.parse_attrs(attrs, function_name, opts)
  end

  @spec prepare_single_child!(attrs_owner(), Emerge.UI.attrs(), child()) :: {attrs_map(), child()}
  def prepare_single_child!(function_name, attrs, child) do
    attrs = Validation.validate_attrs_list!(function_name, attrs)
    child = Validation.validate_child_element!(function_name, child)

    {Validation.parse_attrs(attrs, function_name), child}
  end

  @spec prepare_children!(attrs_owner(), Emerge.UI.attrs(), children()) ::
          {attrs_map(), children()}
  def prepare_children!(function_name, attrs, children) do
    attrs = Validation.validate_attrs_list!(function_name, attrs)
    children = Validation.validate_children_list!(function_name, children)

    {Validation.parse_attrs(attrs, function_name), children}
  end
end
