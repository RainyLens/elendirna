// CI 전용: 6 플랫폼 빌드 artifact를 단일 elendirna 패키지의 binaries/<os>-<cpu>/ 로 배치한다.
// (N0097 v0.8 매커니즘 = 단일 패키지 + 전 바이너리 동봉. 런처 bin/elendirna.js가 런타임에
//  binaries/<process.platform>-<process.arch>/elf[.exe] 를 선택.)
//   node npm/scripts/assemble-bundle.mjs <artifactsDir>
//     artifactsDir/binary-<os>-<cpu>/<elf|elf.exe> 6종을 읽어 npm/elendirna/binaries/ 로 복사
import { chmodSync, cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url)); // npm/scripts
const npmRoot = dirname(here); //                        npm/

const artifactsDir = process.argv[2];
if (!artifactsDir) {
  console.error("usage: node assemble-bundle.mjs <artifactsDir>");
  process.exit(2);
}

const TARGETS = [
  ["linux", "x64", "elf"],
  ["linux", "arm64", "elf"],
  ["darwin", "x64", "elf"],
  ["darwin", "arm64", "elf"],
  ["win32", "x64", "elf.exe"],
  ["win32", "arm64", "elf.exe"],
];

const binRoot = join(npmRoot, "elendirna", "binaries");
// 항상 정확히 6 fresh artifact만 동봉 — 잔여/부분 binaries/ 가 병합돼 잘못 게시되는 것 방지.
rmSync(binRoot, { recursive: true, force: true });
let placed = 0;
for (const [os, cpu, exe] of TARGETS) {
  const src = join(artifactsDir, `binary-${os}-${cpu}`, exe);
  if (!existsSync(src)) {
    console.error(`✗ missing artifact: ${src}`);
    process.exit(1);
  }
  const destDir = join(binRoot, `${os}-${cpu}`);
  mkdirSync(destDir, { recursive: true });
  const dest = join(destDir, exe);
  cpSync(src, dest);
  if (exe === "elf") {
    try {
      chmodSync(dest, 0o755); // POSIX 실행 비트 (Windows에선 무시)
    } catch {
      /* noop */
    }
  }
  placed++;
  console.log(`placed ${os}-${cpu}/${exe}`);
}
console.log(`bundle: ${placed}/6 binaries → ${binRoot}`);
