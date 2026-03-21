import Config

config :logger, level: :debug

config :emerge_demo, EmergeDemo.Application, auto_start?: true

import_config "#{config_env()}.exs"
