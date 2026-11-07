#!/usr/bin/env node
// Keep every version declaration in sync.
//
// Usage:
//   node scripts/version.mjs --check        Verify all versions match.
//   node scripts/version.mjs --set 0.2.0    Update every version declaration.
//
// When GITHUB_REF_NAME is a version tag (vX.Y.Z), --check also verifies that the
// tag matches the workspace version.

import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const files = {
  cargo: path.join(root, "Cargo.toml"),
  packageJson: path.join(root, "crates/nsromexefs-gui/package.json"),
  packageLock: path.join(root, "crates/nsromexefs-gui/package-lock.json"),
  tauriConf: path.join(root, "crates/nsromexefs-gui/src-tauri/tauri.conf.json"),
};

const semver = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;

function fail(message) {
  console.error(`version: ${message}`);
  process.exit(1);
}

async function readJson(file) {
  return JSON.parse(await readFile(file, "utf8"));
}

function readCargoVersion(text) {
  const section = text.match(/\[workspace\.package\]\s*\n([\s\S]*?)(?=\n\[|$)/);
  if (!section) fail("missing [workspace.package] in Cargo.toml");

  const match = section[1].match(/^version\s*=\s*"([^"]+)"/m);
  if (!match) fail("missing version in [workspace.package]");
  return match[1];
}

function readCoreDependencyVersion(text) {
  const match = text.match(
    /nsromexefs-core = \{ path = "crates\/nsromexefs-core", version = "([^"]+)" \}/,
  );
  if (!match) fail("missing workspace version for nsromexefs-core");
  return match[1];
}

async function readVersions() {
  const cargoText = await readFile(files.cargo, "utf8");
  const packageJson = await readJson(files.packageJson);
  const packageLock = await readJson(files.packageLock);
  const tauriConf = await readJson(files.tauriConf);

  return {
    cargo: readCargoVersion(cargoText),
    cargoCoreDep: readCoreDependencyVersion(cargoText),
    packageJson: packageJson.version,
    packageLock: packageLock.version,
    packageLockRoot: packageLock.packages?.[""]?.version,
    tauriConf: tauriConf.version,
  };
}

function report(versions) {
  const entries = Object.entries(versions);
  for (const [name, version] of entries) {
    console.log(`${name.padEnd(16)} ${version ?? "(missing)"}`);
  }
}

async function check() {
  const versions = await readVersions();
  report(versions);

  const values = Object.values(versions);
  if (values.some((value) => !value)) fail("one or more version fields are missing");

  const unique = new Set(values);
  if (unique.size !== 1) fail("version fields do not match");

  const tag = process.env.GITHUB_REF_NAME;
  if (tag && /^v\d+\.\d+\.\d+/.test(tag) && tag.slice(1) !== versions.cargo) {
    fail(`tag ${tag} does not match workspace version ${versions.cargo}`);
  }

  console.log(`version: all files use ${versions.cargo}`);
}

async function setVersion(version) {
  if (!semver.test(version)) fail(`invalid semver: ${version}`);

  const cargoText = await readFile(files.cargo, "utf8");
  let updatedCargo = cargoText.replace(
    /(\[workspace\.package\]\s*\n(?:[^\n]*\n)*?version\s*=\s*")[^"]+(")/,
    `$1${version}$2`,
  );
  updatedCargo = updatedCargo.replace(
    /(nsromexefs-core = \{ path = "crates\/nsromexefs-core", version = ")[^"]+(")/,
    `$1${version}$2`,
  );
  if (updatedCargo === cargoText) fail("could not update Cargo.toml");
  await writeFile(files.cargo, updatedCargo);

  const packageJson = await readJson(files.packageJson);
  packageJson.version = version;
  await writeFile(files.packageJson, `${JSON.stringify(packageJson, null, 2)}\n`);

  const packageLock = await readJson(files.packageLock);
  packageLock.version = version;
  if (!packageLock.packages?.[""]) fail("missing root package in package-lock.json");
  packageLock.packages[""].version = version;
  await writeFile(files.packageLock, `${JSON.stringify(packageLock, null, 2)}\n`);

  const tauriText = await readFile(files.tauriConf, "utf8");
  const updatedTauri = tauriText.replace(
    /("version"\s*:\s*")[^"]+(")/,
    `$1${version}$2`,
  );
  if (updatedTauri === tauriText) fail("could not update tauri.conf.json");
  await writeFile(files.tauriConf, updatedTauri);

  console.log(`version: updated every file to ${version}`);
}

const args = process.argv.slice(2);
if (args[0] === "--check" || args.length === 0) {
  await check();
} else if (args[0] === "--set" && args[1]) {
  await setVersion(args[1]);
} else {
  fail("usage: node scripts/version.mjs --check | --set <version>");
}
