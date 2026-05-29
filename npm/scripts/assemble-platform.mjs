// CI 전용(로컬 검증도 가능): 한 타깃의 플랫폼 패키지를 npm/dist/ 아래로 조립한다.
//   node npm/scripts/assemble-platform.mjs <os> <cpu> <triple> [binPath]
//     os    : linux | darwin | win32        (process.platform 값 = npm "os" 필드)
//     cpu   : x64 | arm64                    (process.arch 값     = npm "cpu" 필드)
//     triple: x86_64-unknown-linux-gnu 등     (cargo target triple, 설명/문서용)
//     binPath: 바이너리 경로 (생략 시 target/<triple>/release/<elf|elf.exe>)
// 산출: npm/dist/elendirna-cli-<os>-<cpu>/{package.json, README.md, bin/<elf|elf.exe>}
import { chmodSync, cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url)); // npm/scripts
const npmRoot = dirname(here); //                        npm/
const repoRoot = dirname(npmRoot); //                    repo root

const [os, cpu, triple, binPathArg] = process.argv.slice(2);
if (!os || !cpu || !triple) {
  console.error("usage: node assemble-platform.mjs <os> <cpu> <triple> [binPath]");
  process.exit(2);
}

function workspaceVersion() {
  const cargo = readFileSync(join(repoRoot, "Cargo.toml"), "utf8");
  const m = cargo.match(/\[workspace\.package\][\s\S]*?\bversion\s*=\s*"([^"]+)"/);
  if (!m) throw new Error("Cargo.toml: [workspace.package] version를 찾지 못함");
  return m[1];
}

const version = workspaceVersion();
const exe = os === "win32" ? "elf.exe" : "elf";
const binPath = binPathArg || join(repoRoot, "target", triple, "release", exe);

if (!existsSync(binPath)) {
  console.error(`바이너리 없음: ${binPath}\n먼저 'cargo build --release --target ${triple} -p eln-cli' 또는 CI artifact 경로를 넘기세요.`);
  process.exit(1);
}

const subst = (s) =>
  s
    .replaceAll("{{os}}", os)
    .replaceAll("{{cpu}}", cpu)
    .replaceAll("{{version}}", version)
    .replaceAll("{{triple}}", triple);

const pkgDir = join(npmRoot, "dist", `elendirna-cli-${os}-${cpu}`);
mkdirSync(join(pkgDir, "bin"), { recursive: true });

writeFileSync(join(pkgDir, "package.json"), subst(readFileSync(join(npmRoot, "platform", "package.json.tmpl"), "utf8")));
writeFileSync(join(pkgDir, "README.md"), subst(readFileSync(join(npmRoot, "platform", "README.md.tmpl"), "utf8")));
cpSync(binPath, join(pkgDir, "bin", exe));
try {
  chmodSync(join(pkgDir, "bin", exe), 0o755); // POSIX 실행 비트 (Windows에선 무시)
} catch {
  /* noop */
}

console.log(`assembled ${pkgDir}\n  version=${version} triple=${triple} bin=${binPath}`);
