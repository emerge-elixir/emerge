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
  @type nearby_mounts :: [{atom(), Element.t()}]

  @spec build_element(attrs_map(), atom(), children()) :: Element.t()
  def build_element(attrs, type, children) when is_map(attrs) do
    build_element(attrs, [], type, children)
  end

  @spec build_element(attrs_map(), nearby_mounts(), atom(), children()) :: Element.t()
  def build_element(attrs, nearby, type, children) when is_map(attrs) and is_list(nearby) do
    {key, attrs} = Map.pop(attrs, :key)
    attrs = Map.put(attrs, :__attrs_hash, TreeAttrs.attrs_hash(attrs))

    %Element{
      type: type,
      id: key,
      attrs: attrs,
      children: children,
      nearby: nearby
    }
  end

  @spec prepare_attrs!(attrs_owner(), Emerge.UI.attrs()) :: {attrs_map(), nearby_mounts()}
  @spec prepare_attrs!(attrs_owner(), Emerge.UI.attrs(), attrs_options()) ::
          {attrs_map(), nearby_mounts()}
  def prepare_attrs!(function_name, attrs, opts \\ []) do
    attrs = Validation.validate_attrs_list!(function_name, attrs)
    Validation.parse_attrs_with_nearby(attrs, function_name, opts)
  end

  @spec prepare_single_child!(attrs_owner(), Emerge.UI.attrs(), child()) ::
          {attrs_map(), nearby_mounts(), child()}
  def prepare_single_child!(function_name, attrs, child) do
    attrs = Validation.validate_attrs_list!(function_name, attrs)
    child = Validation.validate_child_element!(function_name, child)

    {attrs, nearby} = Validation.parse_attrs_with_nearby(attrs, function_name)
    {attrs, nearby, child}
  end

  @spec prepare_children!(attrs_owner(), Emerge.UI.attrs(), children()) ::
          {attrs_map(), nearby_mounts(), children()}
  def prepare_children!(function_name, attrs, children) do
    attrs = Validation.validate_attrs_list!(function_name, attrs)
    children = Validation.validate_children_list!(function_name, children)

    {attrs, nearby} = Validation.parse_attrs_with_nearby(attrs, function_name)
    {attrs, nearby, children}
  end
end
