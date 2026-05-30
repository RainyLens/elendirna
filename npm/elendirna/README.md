# elendirna

AI-friendly knowledge vault — `elf` CLI + MCP server, distributed as a prebuilt binary via npm (no Rust toolchain required).

## Install

```sh
# one-off, no install
npx -y elendirna --help

# or install globally (the `elendirna` command)
npm i -g elendirna
elendirna --help
```

The npm package bundles prebuilt `elf` binaries for all supported platforms and a
thin Node launcher that selects the right one at run time. No `postinstall` script,
and no network access at install or run time.

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
