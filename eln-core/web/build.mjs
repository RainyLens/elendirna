// FE 빌드 — 1회성 사전 트랜스파일([[N0106]] P1, "추출→조립→임베드").
//
//   node build.mjs
//
// 1) React 18 UMD(production)를 dist/vendor/에 벤더링(없을 때만 1회 다운로드) → 오프라인 동작.
// 2) esbuild로 src/main.jsx를 dist/bundle.js로 번들(JSX classic transform, React는 전역 external).
//    esbuild는 첫 실행 시 npx로 받아오며 상시 toolchain은 남기지 않는다.
// 3) 정적 자산(index.html, tokens.css, app.css, fonts/*)을 dist/로 복사.
//
// 산출 dist/ 는 rust-embed 임베드 대상이며 빌드 아티팩트로 커밋한다(cargo build 시 Node 불필요).

import { execSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url)); // web/
const src = join(root, "src");
const dist = join(root, "dist");

mkdirSync(join(dist, "vendor"), { recursive: true });
mkdirSync(join(dist, "fonts"), { recursive: true });

// ── 1) React UMD 벤더링 ──────────────────────────────
const VENDORS = [
  ["react.production.min.js", "https://unpkg.com/react@18.3.1/umd/react.production.min.js"],
  ["react-dom.production.min.js", "https://unpkg.com/react-dom@18.3.1/umd/react-dom.production.min.js"],
];
for (const [name, url] of VENDORS) {
  const dest = join(dist, "vendor", name);
  if (existsSync(dest)) {
    console.log(`vendor: ${name} (cached)`);
    continue;
  }
  console.log(`vendor: downloading ${name} …`);
  const res = await fetch(url);
  if (!res.ok) throw new Error(`fetch ${url} → ${res.status}`);
  const buf = Buffer.from(await res.arrayBuffer());
  writeFileSync(dest, buf);
}

// ── 2) esbuild 번들 ──────────────────────────────────
const cmd = [
  "npx --yes esbuild",
  JSON.stringify(join(src, "main.jsx")),
  "--bundle",
  "--format=iife",
  "--jsx=transform",
  "--jsx-factory=React.createElement",
  "--jsx-fragment=React.Fragment",
  "--target=es2018",
  "--minify",
  `--outfile=${JSON.stringify(join(dist, "bundle.js"))}`,
].join(" ");
console.log("esbuild:", cmd);
execSync(cmd, { stdio: "inherit", cwd: root, shell: true });

// ── 3) 정적 자산 복사 ────────────────────────────────
for (const f of ["index.html", "tokens.css", "app.css"]) {
  cpSync(join(src, f), join(dist, f));
}
const fontsSrc = join(src, "fonts");
if (existsSync(fontsSrc)) {
  for (const f of readdirSync(fontsSrc)) {
    cpSync(join(fontsSrc, f), join(dist, "fonts", f));
  }
}

console.log("build: dist/ ready");
