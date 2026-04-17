defmodule EmergeSkia.Transport do
  @moduledoc false

  alias EmergeSkia.BuildConfig
  alias EmergeSkia.Macos.Renderer

  @type renderer_handle :: term()
  @type asset_config :: map()
  @type offscreen_opts :: map()

  @callback start_session(map(), asset_config()) :: {:ok, renderer_handle()} | {:error, term()}
  @callback stop_session(renderer_handle()) :: :ok
  @callback session_running?(renderer_handle()) :: boolean()
  @callback set_input_target(renderer_handle(), pid() | nil) :: :ok
  @callback set_log_target(renderer_handle(), pid() | nil) :: :ok
  @callback set_input_mask(renderer_handle(), non_neg_integer()) :: :ok
  @callback upload_tree(renderer_handle(), binary()) :: :ok | {:error, term()}
  @callback patch_tree(renderer_handle(), binary()) :: :ok | {:error, term()}
  @callback measure_text(String.t(), float()) :: {float(), float(), float(), float()}
  @callback load_font(String.t(), non_neg_integer(), boolean(), binary()) ::
              :ok | {:ok, boolean()} | {:error, term()}
  @callback configure_assets(renderer_handle(), asset_config()) ::
              :ok | {:error, term()}
  @callback render_tree_to_pixels(binary(), offscreen_opts(), asset_config()) ::
              binary() | {:ok, binary()} | {:error, String.t()}
  @callback render_tree_to_png(binary(), offscreen_opts(), asset_config()) ::
              binary() | {:ok, binary()} | {:error, String.t()}

  @spec for_backend(atom() | String.t()) :: module()
  def for_backend(backend) when backend in [:macos, "macos"], do: EmergeSkia.Transport.MacosHost
  def for_backend(_backend), do: EmergeSkia.Transport.Native

  @spec for_renderer(renderer_handle()) :: module()
  def for_renderer(%Renderer{}), do: EmergeSkia.Transport.MacosHost
  def for_renderer(_renderer), do: EmergeSkia.Transport.Native

  @spec default() :: module()
  def default do
    BuildConfig.default_runtime_backend()
    |> for_backend()
  end
end
