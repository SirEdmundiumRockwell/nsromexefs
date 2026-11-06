#!/usr/bin/env node
// Builds the nsromexefs CLI and places it where Tauri expects a bundled sidecar:
//   src-tauri/binaries/nsromexefs-<target-triple><ext>
// Tauri copies it beside the GUI executable during `tauri build`.
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const appDir = resolve(scriptDir, '..');
const repoRoot = resolve(appDir, '..', '..');
const debug = process.argv.includes('--debug');
const profile = debug ? 'debug' : 'release';
const exeExtension = process.platform === 'win32' ? '.exe' : '';

function fail(message) {
  console.error(`[sidecar] ${message}`);
  process.exit(1);
}

function hostTriple() {
  const direct = spawnSync('rustc', ['--print', 'host-tuple'], { encoding: 'utf8' });
  if (direct.status === 0 && direct.stdout.trim()) {
    return direct.stdout.trim();
  }
  const verbose = spawnSync('rustc', ['-Vv'], { encoding: 'utf8' });
  if (verbose.status !== 0) {
    fail('Could not run rustc. Install the Rust toolchain first.');
  }
  const match = /^host:\s*(\S+)$/m.exec(verbose.stdout);
  if (!match) {
    fail('Could not determine the host target triple from rustc.');
  }
  return match[1];
}

const triple = hostTriple();
console.log(`[sidecar] Building nsromexefs-cli (${profile}) for ${triple}...`);

const cargoArgs = ['build', '-p', 'nsromexefs-cli', '--locked'];
if (!debug) {
  cargoArgs.push('--release');
}
const build = spawnSync('cargo', cargoArgs, { cwd: repoRoot, stdio: 'inherit' });
if (build.status !== 0) {
  fail(`cargo build failed with exit code ${build.status ?? 'unknown'}.`);
}

const binaryName = `nsromexefs${exeExtension}`;
// Cargo may place the build in target/<profile> or target/<triple>/<profile>
// depending on CARGO_BUILD_TARGET or previous cross builds. Pick the newest
// existing artifact so a stale build from the other layout is never bundled.
const candidates = [
  join(repoRoot, 'target', triple, profile, binaryName),
  join(repoRoot, 'target', profile, binaryName)
]
  .filter((candidate) => existsSync(candidate))
  .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
const source = candidates[0];
if (!source) {
  fail(`Built CLI not found in:\n  ${join(repoRoot, 'target', triple, profile)}\n  ${join(repoRoot, 'target', profile)}`);
}
console.log(`[sidecar] Using newest build: ${source}`);

const destinationDir = join(appDir, 'src-tauri', 'binaries');
mkdirSync(destinationDir, { recursive: true });
const destination = join(destinationDir, `nsromexefs-${triple}${exeExtension}`);
copyFileSync(source, destination);

const size = statSync(destination).size;
console.log(`[sidecar] ${source}`);
console.log(`[sidecar] -> ${destination} (${(size / 1024 / 1024).toFixed(1)} MB)`);
