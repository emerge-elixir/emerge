Code.require_file("../mix/input_source_cache.exs", __DIR__)

[cache] = System.argv()
manifest = File.read!(Path.expand("../native/emerge_input/sources.rs", __DIR__))
pins = Emerge.Mix.InputSourceCache.pins!(manifest)
IO.puts("complete=#{Emerge.Mix.InputSourceCache.complete?(cache, pins)}")
