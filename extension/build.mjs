// ─── Build script for mprisence-browser-extension ─────────────────
// Usage: node build.mjs <browser> [--watch] [--store]
//   browser: "firefox" | "chromium"
//   --store   store packaging (keeps manifest key on chromium)

import * as esbuild from "esbuild";
import { copyFileSync, mkdirSync, readFileSync, writeFileSync, existsSync } from "fs";
import { join, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));

const browsers = ["firefox", "chromium"];
const target = process.argv[2];
const isWatch = process.argv.includes("--watch");
const isStore = process.argv.includes("--store");

if (!target || !browsers.includes(target)) {
  console.error(`Usage: node build.mjs <browser> [--watch] [--store]\n  browser: ${browsers.join(" | ")}`);
  process.exit(1);
}

console.log(`Building for ${target}${isStore ? " (store)" : ""}...`);

const outdir = join(__dirname, "dist", target);
mkdirSync(outdir, { recursive: true });

// ── Read manifest, merge shared fields ──
const manifestRaw = readFileSync(join(__dirname, `manifest.${target}.json`), "utf-8");
const manifest = JSON.parse(manifestRaw);
const shared = JSON.parse(readFileSync(join(__dirname, "manifest.shared.json"), "utf-8"));

const merged = {
  ...manifest,
  icons: shared.icons,
  host_permissions: shared.host_permissions,
  content_scripts: [
    {
      matches: shared.content_script_matches,
      js: ["content.js"],
      run_at: "document_idle",
    },
    {
      matches: shared.content_script_matches,
      js: ["page-world.js"],
      run_at: "document_idle",
      world: "MAIN",
    },
  ],
};

// ── Get git SHA ──
let gitSha = "unknown";
try {
  const { execSync } = await import("child_process");
  gitSha = execSync("git rev-parse --short HEAD").toString().trim();
  const status = execSync("git status --porcelain").toString().trim();
  if (status) gitSha += "-dirty";
} catch {}

// ── esbuild entries ──
const entryPoints = {
  "background": "src/background.ts",
  "content": "src/content.ts",
  "page-world": "src/page-world.ts",
};

async function build() {
  const ctx = await esbuild.context({
    entryPoints,
    outdir,
    bundle: true,
    sourcemap: true,
    target: "es2022",
    format: "esm",
    platform: "browser",
    define: { __GIT_SHA__: JSON.stringify(gitSha) },
    outbase: "src",
    outExtension: { ".js": ".js" },
  });

  if (isWatch) {
    await ctx.watch();
    console.log(`Watching ${target}...`);
  } else {
    await ctx.rebuild();
    await ctx.dispose();

    if (isStore && target === "chromium" && "key" in merged) {
      console.log('  keeping "key" for stable Chrome extension ID');
    }

    // Write manifest
    writeFileSync(join(outdir, "manifest.json"), JSON.stringify(merged, null, 2));

    // Copy icons
    const iconDir = join(outdir, "icons");
    mkdirSync(iconDir, { recursive: true });
    for (const size of ["48", "96", "128"]) {
      const src = join(__dirname, "icons", `icon-${size}.png`);
      if (existsSync(src)) {
        copyFileSync(src, join(iconDir, `icon-${size}.png`));
      }
    }
    // Use SVG as fallback icon
    if (existsSync(join(__dirname, "icons", "icon.svg"))) {
      copyFileSync(join(__dirname, "icons", "icon.svg"), join(iconDir, "icon.svg"));
    }

    console.log(`Built: ${outdir}/`);
  }
}

build().catch((err) => {
  console.error(err);
  process.exit(1);
});
