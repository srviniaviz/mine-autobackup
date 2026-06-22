import fs from "node:fs";

const version = process.argv[2];

if (!version || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  console.error("Usage: npm run version:app -- 1.2.3");
  process.exit(1);
}

function readJson(path) {
  return JSON.parse(fs.readFileSync(path, "utf8"));
}

function writeJson(path, value) {
  fs.writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}

const packageJson = readJson("package.json");
packageJson.version = version;
writeJson("package.json", packageJson);

if (fs.existsSync("package-lock.json")) {
  const lock = readJson("package-lock.json");
  lock.version = version;
  if (lock.packages?.[""]) {
    lock.packages[""].version = version;
  }
  writeJson("package-lock.json", lock);
}

const tauriConfig = readJson("src-tauri/tauri.conf.json");
tauriConfig.version = version;
writeJson("src-tauri/tauri.conf.json", tauriConfig);

const cargoPath = "src-tauri/Cargo.toml";
const cargoToml = fs.readFileSync(cargoPath, "utf8");
fs.writeFileSync(
  cargoPath,
  cargoToml.replace(/^version = ".*"$/m, `version = "${version}"`)
);

console.log(`App version updated to ${version}`);
