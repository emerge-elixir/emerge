defmodule Emerge.CodeReloader.Compiler do
  @moduledoc false

  @default_compilers [:elixir]
  @default_args ["--no-all-warnings"]

  @spec reload([atom()], keyword()) :: :ok | :noop | {:error, binary()}
  def reload(reloadable_apps, opts \\ []) when is_list(reloadable_apps) and is_list(opts) do
    with :ok <- ensure_mix_available() do
      Mix.Project.with_build_lock(fn ->
        try do
          do_reload(reloadable_apps, opts)
        catch
          kind, reason ->
            {:error, Exception.format(kind, reason, __STACKTRACE__)}
        end
      end)
    end
  end

  defp do_reload(reloadable_apps, opts) do
    statuses =
      Mix.Dep.cached()
      |> Enum.filter(&(&1.app in reloadable_apps))
      |> Enum.map(fn dep ->
        Mix.Dep.in_dependency(dep, fn _ ->
          compile_current_project(opts)
        end)
      end)

    case merge_statuses(statuses) do
      {:error, _reason} = error ->
        error

      dep_status ->
        project_status = compile_root_project(reloadable_apps, opts)
        merge_statuses([dep_status, project_status])
    end
  end

  defp compile_root_project(reloadable_apps, opts) do
    if Mix.Project.config()[:app] in reloadable_apps do
      compile_current_project(opts)
    else
      :noop
    end
  end

  defp compile_current_project(opts) do
    with :ok <- ensure_restart_not_required(),
         :ok <- build_structure(),
         {:ok, status} <- run_compilers(opts),
         :ok <- maybe_compile_protocols(status) do
      status
    end
  end

  defp ensure_mix_available do
    cond do
      not Code.ensure_loaded?(Mix.Project) ->
        {:error, "Emerge.CodeReloader requires Mix to be available in the running VM."}

      is_nil(Mix.Project.get()) ->
        {:error, "Emerge.CodeReloader requires a current Mix project."}

      true ->
        :ok
    end
  end

  defp ensure_restart_not_required do
    manifests = Mix.Tasks.Compile.Elixir.manifests()

    files =
      Mix.Project.config_files()
      |> Enum.concat(List.wrap(Mix.Project.project_file()))

    case Mix.Utils.extract_stale(files, manifests) do
      [] ->
        :ok

      stale_files ->
        {:error, restart_required_message(stale_files)}
    end
  end

  defp build_structure do
    Mix.Project.build_structure(Mix.Project.config())
  end

  defp run_compilers(opts) do
    compilers = Keyword.get(opts, :reloadable_compilers, @default_compilers)
    args = compiler_args(opts)

    Mix.Task.Compiler.reenable(compilers)

    case Mix.Task.Compiler.run(compilers, args) do
      {:error, diagnostics} -> {:error, format_diagnostics(diagnostics)}
      {status, _diagnostics} -> {:ok, status}
    end
  end

  defp compiler_args(opts) do
    consolidation_path = Mix.Project.consolidation_path(Mix.Project.config())

    [
      "--return-errors",
      "--purge-compiler-modules",
      "--purge-consolidation-path-if-stale",
      consolidation_path
      | Keyword.get(opts, :reloadable_args, @default_args)
    ]
  end

  defp maybe_compile_protocols(:ok) do
    if Mix.Project.config()[:consolidate_protocols] do
      path = Mix.Project.consolidation_path(Mix.Project.config())
      Mix.Task.reenable("compile.protocols")
      _ = Mix.Task.run("compile.protocols", [])
      Code.prepend_path(path)
    end

    :ok
  end

  defp maybe_compile_protocols(:noop), do: :ok

  defp merge_statuses(statuses) do
    Enum.reduce_while(statuses, :noop, fn
      {:error, _reason} = error, _status -> {:halt, error}
      :ok, :noop -> {:cont, :ok}
      :ok, :ok -> {:cont, :ok}
      :noop, status -> {:cont, status}
    end)
  end

  defp restart_required_message(stale_files) do
    relative_files = Enum.map_join(stale_files, "\n  * ", &Path.relative_to_cwd/1)

    """
    Emerge.CodeReloader cannot continue because project configuration changed.

    Restart the running application before using hot reload again. The following files changed:

      * #{relative_files}
    """
    |> String.trim()
  end

  defp format_diagnostics([]), do: "Compilation failed."

  defp format_diagnostics(diagnostics) do
    diagnostics
    |> Enum.map(&diagnostic_to_chars/1)
    |> IO.iodata_to_binary()
    |> String.trim()
  end

  defp diagnostic_to_chars(%{severity: :error, message: "**" <> _ = message}) do
    ["\n", message, "\n"]
  end

  defp diagnostic_to_chars(%{
         severity: severity,
         message: message,
         file: file,
         position: position
       })
       when is_binary(file) do
    [
      "\n",
      to_string(severity),
      ": ",
      message,
      "\n  ",
      Path.relative_to_cwd(file),
      format_position(position),
      "\n"
    ]
  end

  defp diagnostic_to_chars(%{severity: severity, message: message}) do
    ["\n", to_string(severity), ": ", message, "\n"]
  end

  defp format_position({line, column}), do: ":#{line}:#{column}"
  defp format_position(line) when is_integer(line) and line > 0, do: ":#{line}"
  defp format_position(_position), do: ""
end
