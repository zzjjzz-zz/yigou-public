// Verify that Tauri/WebView2 loads licensed fonts from same-origin resources.
// Small WOFF2 data URLs are not permitted under the restricted font-src CSP.
import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve, basename } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const assets = join(root, "app", "dist", "assets");
const cssFiles = readdirSync(assets).filter((f) => /^index-.*\.css$/.test(f));
assert.equal(cssFiles.length, 1, "Expected exactly one production entry stylesheet");
const css = readFileSync(join(assets, cssFiles[0]), "utf8");
const fonts = readdirSync(assets).filter((f) => f.endsWith(".woff2"));
const noto = fonts.filter((f) => f.startsWith("noto-sans-sc-"));
const mono = fonts.filter((f) => f.startsWith("jetbrains-mono-"));
assert.match(css, /Noto Sans SC Variable/, "Missing packaged Simplified Chinese typeface");
assert.match(css, /JetBrains Mono Variable/, "Missing packaged terminal typeface");
assert.doesNotMatch(css, /data:font\//i, "Inline font detected: violates same-origin resource policy");
assert.ok(noto.length >= 100, "Chinese font coverage unexpectedly incomplete");
assert.ok(mono.length >= 5, "Latin/monospace font coverage unexpectedly incomplete");
const resourceNames = [...css.matchAll(/url\(["']?(?:\.{0,2}\/)?(?:\/)?assets\/([^)"']+\.woff2)["']?\)/g)].map((m) => basename(m[1]));
assert.ok(resourceNames.length >= fonts.length, "Expected referenced files for each packaged font; found " + resourceNames.length);
for (const file of resourceNames) assert.ok(existsSync(join(assets, file)), "Missing WOFF2 resource: " + file);
const configuration = JSON.parse(readFileSync(join(root, "app", "src-tauri", "tauri.conf.json"), "utf8"));
const csp = configuration?.app?.security?.csp || "";
assert.match(csp, /font-src\s+'self'/, "App must explicitly allow same-origin fonts");
assert.doesNotMatch(csp, /font-src[^;]*data:/, "Font CSP should not allow inlined data URLs");
console.log("FONT_RESOURCES_PASS: " + noto.length + " Chinese subsets + " + mono.length + " monospace subsets; all referenced files present");
