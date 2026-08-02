// Copies the static frontend (index.html + app/) into a gitignored
// dist/ folder that src-tauri/tauri.conf.json points frontendDist at.
//
// This exists so Tauri's frontendDist never points at the repo root
// (which would illegally include src-tauri/target and node_modules).
// The root index.html and app/ folder remain the single source of
// truth - dist/ is always regenerated fresh here, never hand-edited.
const fs = require("fs");
const path = require("path");

const root = path.resolve(__dirname, "..");
const dist = path.join(root, "dist");

function copyRecursive(source, destination) {
  const stat = fs.statSync(source);
  if (stat.isDirectory()) {
    fs.mkdirSync(destination, { recursive: true });
    for (const entry of fs.readdirSync(source)) {
      copyRecursive(path.join(source, entry), path.join(destination, entry));
    }
  } else {
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.copyFileSync(source, destination);
  }
}

fs.rmSync(dist, { recursive: true, force: true });
fs.mkdirSync(dist, { recursive: true });

copyRecursive(path.join(root, "index.html"), path.join(dist, "index.html"));
copyRecursive(path.join(root, "app"), path.join(dist, "app"));

console.log(`Prepared ${dist}`);
