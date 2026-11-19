import { build } from "esbuild";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const webDir = join(here, "..");
const cargoToml = readFileSync(join(webDir, "../../Cargo.toml"), "utf-8");
const match = cargoToml.match(/^\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m);
if (!match) {
  throw new Error("Cannot read [workspace.package].version from root Cargo.toml");
}

await build({
  absWorkingDir: webDir,
  entryPoints: ["src/main.ts"],
  bundle: true,
  outfile: "dist/app.js",
  format: "esm",
  loader: { ".css": "css" },
  define: { __WEB_VERSION__: JSON.stringify(match[1]) },
});
