#!/usr/bin/env node
// elendirna npm launcher — 패키지에 동봉된(binaries/<os>-<cpu>/) prebuilt `elf` 바이너리를
// 런타임에 선택해 spawn한다. (N0097 v0.8 cut, 매커니즘 = 단일 패키지 + 전 바이너리 동봉:
// postinstall 없음·설치 시 네트워크 없음·플랫폼 패키지 없음 → npm 스팸 탐지 회피.)
//
// 이 shim은 일회성 CLI(`elf query …`)와 장수명 stdio MCP 서버(`elf serve --mcp`) 둘 다를
// 기동하므로 spawnSync가 아닌 async spawn을 쓴다 — 부모가 받은 SIGINT/SIGTERM을 자식에
// 전달해야 서버가 고아 프로세스로 남지 않는다 (서버는 stdin EOF 또는 시그널로 종료).
"use strict";

const { spawn } = require("node:child_process");
const path = require("node:path");
const fs = require("node:fs");

function binaryPath() {
  const os = process.platform; // linux | darwin | win32
  const cpu = process.arch; //    x64   | arm64
  const exe = os === "win32" ? "elf.exe" : "elf";
  // 런처는 bin/ 에, 바이너리는 자매 디렉터리 binaries/<os>-<cpu>/ 에 동봉된다.
  const p = path.join(__dirname, "..", "binaries", `${os}-${cpu}`, exe);
  return fs.existsSync(p) ? p : null;
}

const bin = binaryPath();
if (!bin) {
  const os = process.platform;
  const cpu = process.arch;
  process.stderr.write(
    `elendirna: no prebuilt binary bundled for ${os}-${cpu}.\n` +
      `Supported platforms: linux / darwin / win32 × x64 / arm64.\n` +
      `If your platform is in that set, the install may be corrupted — reinstall (npm i elendirna).\n` +
      `Otherwise build from source: cargo install eln-cli\n`
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
    // POSIX는 부모 종료 상태에 시그널을 반영(re-raise). Windows는 POSIX 시그널 re-raise를
    // 제대로 지원하지 않으므로 non-zero exit code로 대체한다.
    if (process.platform === "win32") {
      process.exit(1);
    } else {
      process.kill(process.pid, signal);
    }
  } else {
    process.exit(code === null ? 1 : code);
  }
});
