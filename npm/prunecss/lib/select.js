// Chooses which platform packages can run on a host, best first. Pure, so
// npm/test.mjs can check it for hosts other than the one it runs on.
'use strict';

// Compares "2.31" with "2.35"; returns <0, 0 or >0.
function compareVersions(a, b) {
  const pa = a.split('.').map(Number);
  const pb = b.split('.').map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i += 1) {
    const d = (pa[i] || 0) - (pb[i] || 0);
    if (d !== 0) return d;
  }
  return 0;
}

// host: { platform, arch, glibcVersion } where glibcVersion is null on musl
// systems (Alpine) and on non-Linux hosts.
function candidates(platforms, host) {
  const matching = platforms.filter((p) => p.os === host.platform && p.cpu === host.arch);
  if (host.platform !== 'linux') return matching;
  return matching
    .filter((p) => {
      if (p.libc !== 'glibc') return true; // the musl build is static: runs anywhere
      // A glibc build needs glibc, at least the version it was linked against.
      if (!host.glibcVersion) return false;
      return !p.glibcMin || compareVersions(host.glibcVersion, p.glibcMin) >= 0;
    })
    // Prefer the glibc build where it can run (faster allocator), else musl.
    .sort((a, b) => (a.libc === 'glibc' ? 0 : 1) - (b.libc === 'glibc' ? 0 : 1));
}

module.exports = { candidates, compareVersions };
