#!/usr/bin/env node
// Runs the prebuilt prunecss binary for this platform.
//
// The binaries live in one npm package per platform (@prunecss/darwin-arm64,
// @prunecss/linux-x64-gnu, ...). They are optional dependencies of this
// package with `os`/`cpu` fields, so npm installs only the one that fits.
// This script finds it and runs it with the same arguments, standard
// streams and exit code.
'use strict';

const { spawnSync } = require('node:child_process');
const platforms = require('../platforms.json');
const { candidates } = require('../lib/select.js');

// The glibc version of this system, or null on musl systems (Alpine) and
// other operating systems. Node reports it in its diagnostic report header.
function glibcVersion() {
  if (process.platform !== 'linux' || !process.report || !process.report.getReport) return null;
  const report = process.report.getReport();
  const header = (typeof report === 'string' ? JSON.parse(report) : report).header;
  return (header && header.glibcVersionRuntime) || null;
}

function findBinary() {
  if (process.env.PRUNECSS_BINARY) {
    return process.env.PRUNECSS_BINARY;
  }
  const host = { platform: process.platform, arch: process.arch, glibcVersion: glibcVersion() };
  for (const platform of candidates(platforms, host)) {
    const file = platform.os === 'win32' ? 'prunecss.exe' : 'prunecss';
    try {
      return require.resolve(`${platform.package}/bin/${file}`);
    } catch {
      // Not installed; try the next candidate.
    }
  }
  return null;
}

const binary = findBinary();
if (!binary) {
  const supported = platforms.map((p) => `${p.os}-${p.cpu}${p.libc ? ` (${p.libc})` : ''}`);
  process.stderr.write(
    `prunecss: no prebuilt binary for ${process.platform}-${process.arch}.\n` +
      `Supported: ${supported.join(', ')}.\n` +
      'On Linux, the glibc builds need a recent glibc; x64 falls back to the static musl build.\n' +
      'If your platform is listed, reinstall without --no-optional / --omit=optional.\n' +
      'Otherwise install from source with Rust: cargo install prunecss --locked\n',
  );
  process.exit(2);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit', windowsHide: true });
if (result.error) {
  process.stderr.write(`prunecss: could not run ${binary}: ${result.error.message}\n`);
  process.exit(2);
}
if (result.signal) {
  // Re-raise the signal so the caller sees the same outcome.
  process.kill(process.pid, result.signal);
}
process.exit(result.status === null ? 2 : result.status);
