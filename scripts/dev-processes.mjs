/*
 * Finding and stopping the processes a dev session leaves behind: the dev build of the app, the
 * host sidecar it started, and the vite server on the dev port. Shared by the gate (which refuses
 * to run cargo while the app holds its executable) and the E2E runner (which owns the app's whole
 * lifetime).
 *
 * Only processes whose executable lives inside this repository count as ours — an installed copy
 * of the app, or another checkout, is never touched.
 */

import { execFileSync } from "node:child_process";
import { resolve, sep } from "node:path";

const OWN_EXECUTABLES = ["textree.exe", "textree-host.exe"];
export const DEV_SERVER_PORT = 1420;

function powershell(script) {
  return execFileSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "ignore"],
  }).trim();
}

function asArray(json) {
  if (!json) return [];
  const parsed = JSON.parse(json);
  return Array.isArray(parsed) ? parsed : [parsed];
}

/** The app and host processes running from an executable inside `repo`. Empty off Windows. */
export function runningDevApps(repo) {
  if (process.platform !== "win32") return [];
  const names = OWN_EXECUTABLES.map((n) => `'${n}'`).join(",");
  const rows = asArray(
    powershell(
      `Get-CimInstance Win32_Process | Where-Object { @(${names}) -contains $_.Name -and $_.ExecutablePath } | ` +
        `Select-Object @{n='pid';e={$_.ProcessId}}, @{n='name';e={$_.Name}}, @{n='path';e={$_.ExecutablePath}} | ConvertTo-Json -Compress`,
    ),
  );
  const root = resolve(repo).toLowerCase() + sep;
  return rows.filter((r) => r.path.toLowerCase().startsWith(root));
}

/** Processes listening on the dev server port — a vite left behind by a stopped app. */
export function devServerListeners() {
  if (process.platform !== "win32") return [];
  return asArray(
    powershell(
      // "No such connection" is the common case, and it still sets a failing exit code even with
      // SilentlyContinue — so an empty result exits 0 explicitly.
      `$c = Get-NetTCPConnection -LocalPort ${DEV_SERVER_PORT} -State Listen -ErrorAction SilentlyContinue; ` +
        `if ($c) { $c | Select-Object -Unique @{n='pid';e={$_.OwningProcess}} | ConvertTo-Json -Compress }; exit 0`,
    ),
  );
}

/** Stops a process and everything it started. Returns false when it was already gone. */
export function killTree(pid) {
  try {
    execFileSync("taskkill", ["/PID", String(pid), "/T", "/F"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}
