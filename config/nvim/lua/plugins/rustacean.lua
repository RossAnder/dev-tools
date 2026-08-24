return {
  "mrcjkb/rustaceanvim",
  lazy = false,
  opts = {
    server = {
      -- NOTE (windows): the Linux copy of this file carries
      --   cmd = { "env", "LD_PRELOAD=", "RUST_BACKTRACE=full", "rust-analyzer" }
      -- nested under `settings`, where rustaceanvim never reads it — dead
      -- config there, and `env`/LD_PRELOAD do not exist on Windows either way.
      -- Left out rather than relocated: relocating it to `server.cmd` would
      -- start applying a wrapper that has never actually been in effect.
      -- rustaceanvim finds rust-analyzer on PATH (rustup component add
      -- rust-analyzer, or a Mason install).
      settings = {
        ["rust-analyzer"] = {
          procMacro = {
            enable = true,
            attributes = {
              enable = true,
            },
            -- Parallel proc-macro expansion (useful for serde/embassy)
            processes = 2,
          },
          cargo = {
            buildScripts = {
              enable = true,
            },
            -- Separate target dir so RA never locks or invalidates your cargo build cache
            targetDir = true,
            extraEnv = {
              RUSTC_WRAPPER = "",
              RUSTFLAGS = "",
            },
          },
          check = {
            -- cargo check is ~10x lighter than clippy; run clippy manually or in CI
            command = "check",
          },
          diagnostics = {
            enable = true,
            experimental = {
              enable = true,
            },
          },
          lru = {
            capacity = 256,
          },
          completion = {
            limit = 100,
          },
          cachePriming = {
            enable = true,
            numThreads = "physical",
          },
        },
      },
    },
  },
}
