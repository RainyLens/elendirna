// 단일 진실 = 워크스페이스 Cargo.toml의 [workspace.package] version.
// 그 버전을 npm 메인/별 패키지의 version·의존 핀에 전파한다.
//   node npm/scripts/sync-version.mjs            # 파일에 기록 (정규화)
//   node npm/scripts/sync-version.mjs --check     # 버전 정합 검증만 (drift면 exit 1) — CI/pre-publish 용
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url)); // npm/scripts
const npmRoot = dirname(here); //                        npm/
const repoRoot = dirname(npmRoot); //                    repo root

const check = process.argv.includes("--check");

export function workspaceVersion(root = repoRoot) {
  const cargo = readFileSync(join(root, "Cargo.toml"), "utf8");
  const m = cargo.match(/\[workspace\.package\][\s\S]*?\bversion\s*=\s*"([^"]+)"/);
  if (!m) throw new Error("Cargo.toml: [workspace.package] version를 찾지 못함");
  return m[1];
}

const PLATFORM_PKGS = [
  "elendirna-cli-linux-x64",
  "elendirna-cli-linux-arm64",
  "elendirna-cli-darwin-x64",
  "elendirna-cli-darwin-arm64",
  "elendirna-cli-win32-x64",
  "elendirna-cli-win32-arm64",
];

const version = workspaceVersion();
let drift = false;

// 의미 단위로 검증/수정 — 파일 포매팅에 의존하지 않는다.
function expectVersions(relPath, pkg) {
  const found = [["version", pkg.version]];
  if (relPath.startsWith("elendirna/")) {
    for (const n of PLATFORM_PKGS) found.push([`optionalDependencies.${n}`, pkg.optionalDependencies?.[n]]);
  } else if (relPath.startsWith("eln/")) {
    found.push(["dependencies.elendirna", pkg.dependencies?.elendirna]);
  }
  return found;
}

function apply(relPath, mutate) {
  const file = join(npmRoot, relPath);
  const pkg = JSON.parse(readFileSync(file, "utf8"));
  if (check) {
    let ok = true;
    for (const [field, value] of expectVersions(relPath, pkg)) {
      if (value !== version) {
        console.error(`✗ ${relPath} ${field} = ${value ?? "(missing)"} ≠ ${version}`);
        ok = false;
        drift = true;
      }
    }
    if (ok) console.log(`✓ ${relPath} @ ${version}`);
  } else {
    mutate(pkg);
    writeFileSync(file, JSON.stringify(pkg, null, 2) + "\n");
    console.log(`wrote ${relPath} @ ${version}`);
  }
}

apply("elendirna/package.json", (p) => {
  p.version = version;
  p.optionalDependencies = Object.fromEntries(PLATFORM_PKGS.map((n) => [n, version]));
});

apply("eln/package.json", (p) => {
  p.version = version;
  p.dependencies = { ...(p.dependencies || {}), elendirna: version };
});

if (check && drift) {
  console.error(`\nnpm 패키지 버전이 Cargo ${version}와 불일치 — 'node npm/scripts/sync-version.mjs' 실행 후 커밋.`);
  process.exit(1);
}
