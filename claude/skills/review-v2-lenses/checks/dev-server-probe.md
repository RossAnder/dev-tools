+++
id = "dev-server-probe"
status = "stable"
families = ["code", "perf"]
executor = "surveyor"
fence = "DEV SERVER"
activation = "signal"
apply_when = { signals = ["frontend"] }
+++

# dev-server-probe

## Runs

Detects a running dev server for frontend scope, so cell agents with a browser can observe the UI. It only detects: it never starts, restarts or stops a server, and never runs a dev, build or install command. Stop at the first step that settles the fence.

1. **Frontend scope.** Go on only when the recorded signals include `frontend`, or a scope file ends in `.tsx`, `.jsx`, `.vue`, `.svelte`, `.css`, `.scss`, `.less`, `.html`, `.cshtml` or `.razor`. Otherwise the result is `no frontend scope`.
2. **Browser available.** Read `.mcp.json` at the repository root. Go on only when its `mcpServers` table has a `playwright` key. Otherwise the result is `no playwright server in .mcp.json`.
3. **Find the documented URL.** Take the first of these that gives one:
   - a `http://localhost:<port>` or `http://127.0.0.1:<port>` URL in the repo-root `CLAUDE.md`, or in a `CLAUDE.md` inside the scope's directories, on a line that mentions a dev server, `dev` or `serve`;
   - the `dev` script under `scripts` in the `package.json` nearest the scope's frontend files (the same directory or the closest parent): a URL in it, or a `--port <n>` or `-p <n>` argument, which gives `http://localhost:<n>`;
   - that same `dev` script running `vite` with no port, which gives `http://localhost:5173`, or `next dev` with no port, which gives `http://localhost:3000`.

   When none gives a URL, the result is `no documented dev URL`.
4. **Probe it.** Run `curl -s -o /dev/null -w '%{http_code}' --max-time 5 <url>` once. Any status from `100` to `599` means a server answered, a `404` included. `000`, or a curl error, means nothing answered.
5. **Note the dev command.** When nothing answered, note the `dev` script from step 3 as the documented dev command, if there is one.

## Interpreting the result

- A status from `100` to `599`: a server is up at that URL, and cell agents may browse it.
- `000` or a curl error: nothing is listening. Do not retry, and do not try other ports or URLs.
- The surveyor never asks the user to start a server. When nothing answered and a dev command is documented, the fence records the command so the orchestrator can ask.

## Fence or finding mapping

Writes the `DEV SERVER` fence in `pack.md` and no findings. The fence holds one line:

- a server answered: `DEV SERVER: <url>`;
- nothing answered and a dev command is documented: `DEV SERVER: none — nothing answered at <url>; documented dev command: <the dev script>`;
- any other outcome: `DEV SERVER: none — <the result from Runs>`.

Cell agents attach only to the URL a `DEV SERVER: <url>` line names, and never start a server of their own. `DEV SERVER: none` means no browser observation in this run.

In your digest, the `checks:` line for `dev-server-probe` is `ran` whenever the fence was written, with the fence line after it.
