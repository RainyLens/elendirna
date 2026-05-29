# elendirna

AI-friendly knowledge vault — `elf` CLI + MCP server, distributed as a prebuilt binary via npm (no Rust toolchain required).

## Install

```sh
# one-off, no install
npx -y elendirna --help

# or install globally (provides both `elendirna` and `eln` commands)
npm i -g elendirna
elendirna --help
eln --help
```

The npm package is a thin Node launcher. The actual binary ships in a per-platform
optional dependency (`elendirna-cli-<os>-<cpu>`); npm installs only the one matching
your OS/CPU. No `postinstall` script and no network access at run time.

**Supported platforms:** linux / macOS / Windows × x64 / arm64. Linux builds target
glibc ≥ 2.35 (Ubuntu 22.04 baseline); musl/Alpine is not yet shipped — use
`cargo install eln-cli` there.

## MCP server

```sh
# print a Claude Desktop / .claude/mcp.json snippet for stdio transport
elendirna serve
```

When launched through this npm wrapper, the snippet's `command` is the stable
`elendirna` (on PATH after a global install) rather than a `node_modules` path.
For an npx-only setup, use:

```json
{ "mcpServers": { "elendirna": {
  "command": "npx",
  "args": ["-y", "elendirna", "serve", "--mcp", "--transport", "stdio", "--vault", "/path/to/vault"]
} } }
```

## Source

Rust source, issues, and docs: <https://github.com/elen-labs/elendirna>.
Also published to crates.io as `eln-cli` (`cargo install eln-cli`).
