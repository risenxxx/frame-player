// `bun run fetch-libs`: the native libraries for whichever platform this is.
// Each platform has its own script, because what "fetch" means differs —
// Windows downloads the SDKs, macOS downloads the published library set, Linux
// checks the distribution's packages (see each script's header). Arguments are
// passed through (`bun run fetch-libs -- -Arch arm64` on Windows).
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);

const [cmd, cmdArgs] =
  process.platform === 'win32'
    ? ['powershell', ['-ExecutionPolicy', 'Bypass', '-File', join(here, 'fetch-libs.ps1'), ...args]]
    : process.platform === 'darwin'
      ? ['bash', [join(here, 'fetch-macos-libs.sh'), ...args]]
      : process.platform === 'linux'
        ? ['bash', [join(here, 'fetch-linux-libs.sh'), ...args]]
        : [null, []];

if (!cmd) {
  console.error(`fetch-libs: no native library set for ${process.platform}`);
  process.exit(1);
}
const result = spawnSync(cmd, cmdArgs, { stdio: 'inherit' });
process.exit(result.status ?? 1);
