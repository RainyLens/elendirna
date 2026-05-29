#!/usr/bin/env node
// elendirna npm launcher — optionalDependencies 플랫폼 패키지에 동봉된 prebuilt `elf`
// 바이너리를 resolve해 spawn한다. (N0097 v0.8 cut — postinstall download 대신 optionalDeps.)
//
// 이 shim은 일회성 CLI(`elf query …`)와 장수명 stdio MCP 서버(`elf serve --mcp`) 둘 다를
// 기동하므로 spawnSync가 아닌 async spawn을 쓴다 — 부모가 받은 SIGINT/SIGTERM을 자식에
// 전달해야 서버가 고아 프로세스로 남지 않는다 (서버는 stdin EOF 또는 시그널로 종료).
"use strict";

const { spawn } = require("node:child_process");
const path = require("node:path");

function binaryPath() {
  const os = process.platform; // linux | darwin | win32
  const cpu = process.arch; //    x64   | arm64
  const pkg = `elendirna-cli-${os}-${cpu}`;
  const exe = os === "win32" ? "elf.exe" : "elf";
  try {
    // 플랫폼 패키지의 package.json을 기준점으로 bin/ 경로 조립 (install layout에 견고).
    const pkgRoot = path.dirname(require.resolve(`${pkg}/package.json`));
    return path.join(pkgRoot, "bin", exe);
  } catch {
    return null;
  }
}

const bin = binaryPath();
if (!bin) {
  const os = process.platform;
  const cpu = process.arch;
  process.stderr.write(
    `elendirna: no prebuilt binary for ${os}-${cpu}.\n` +
      `The platform package "elendirna-cli-${os}-${cpu}" is not installed.\n` +
      `This usually means optional dependencies were skipped ` +
      `(--omit=optional / --no-optional / --ignore-optional), or your OS/CPU is unsupported ` +
      `(supported: linux/darwin/win32 × x64/arm64).\n` +
      `Reinstall without omitting optional deps (npm i elendirna), ` +
      `or build from source: cargo install eln-cli\n`
  );
  process.exit(1);
}

// `serve`가 출력하는 MCP config 스니펫이 node_modules 내부 절대경로 대신 안정 PATH 명령을
// emit하도록, 사용자가 호출한 bin 이름(elendirna|eln)을 env로 넘긴다 (serve.rs resolve_elf_bin).
const invoked = path
  .basename(process.argv[1] || "elendirna")
  .replace(/\.js$/, "");
const launcherCmd = invoked === "eln" ? "eln" : "elendirna";

const child = spawn(bin, process.argv.slice(2), {
  stdio: "inherit",
  env: { ...process.env, ELN_LAUNCHER_CMD: launcherCmd },
  windowsHide: false,
});

// 부모가 받은 종료 시그널을 장수명 서버 자식에게 전달 (Windows: SIGBREAK).
for (const sig of ["SIGINT", "SIGTERM", "SIGHUP", "SIGBREAK"]) {
  process.on(sig, () => {
    if (!child.killed) {
      try {
        child.kill(sig);
      } catch {
        /* 자식이 이미 종료 */
      }
    }
  });
}

// 부모가 다른 경로로 죽어도 자식을 고아로 남기지 않는다.
process.on("exit", () => {
  if (!child.killed) {
    try {
      child.kill();
    } catch {
      /* noop */
    }
  }
});

child.on("error", (err) => {
  process.stderr.write(`elendirna: failed to launch binary: ${err.message}\n`);
  process.exit(126);
});

// exit code와 시그널을 정확히 전파 (`elf --version`, 일회성 CLI, 서버 종료 모두 올바른 상태).
child.on("close", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
  } else {
    process.exit(code === null ? 1 : code);
  }
});
