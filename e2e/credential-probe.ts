import { execFileSync } from "node:child_process";

/**
 * Read-only look at a Windows Credential Manager entry, so a test can prove which entry the app
 * under test wrote to.
 *
 * A development build is compiled to use its own credential namespace and cannot reach the entry
 * the installed app uses (`secret_store.rs`). This exists so the suite checks that instead of
 * assuming it: if the namespacing were ever removed, a test that completes a sign-in would begin
 * overwriting a real publishing token, and only a browser sign-in can mint another one. The probe
 * turns that silent damage into a failing assertion.
 *
 * `cmdkey /list` is not usable here — it prints target names for entries that hold nothing, so it
 * cannot answer "does this entry exist". CredRead returns the record itself.
 *
 * The stored secret is never read out: only presence, byte length, and the last-written timestamp
 * cross the boundary, which is enough to tell "untouched" from "rewritten".
 */

export interface CredentialRecord {
  present: boolean;
  /** Size of the stored blob in bytes; 0 when absent. */
  size: number;
  /** Windows FILETIME as a decimal string; "" when absent. Changes on every write. */
  lastWritten: string;
}

/** Entry the installed (shipped) app uses. Nothing in the test suite may ever write to it. */
export const PRODUCTION_PUBLISH_TARGET = "token.com.textree.publish";
/** Entry a development build uses — the one a signed-in E2E run is expected to touch. */
export const DEV_PUBLISH_TARGET = "token.com.textree.publish.dev";

const PROBE_SCRIPT = `
$sig = @'
using System;
using System.Runtime.InteropServices;
public static class TextreeCredProbe {
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
  public struct CREDENTIAL {
    public uint Flags; public uint Type; public IntPtr TargetName; public IntPtr Comment;
    public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
    public uint CredentialBlobSize; public IntPtr CredentialBlob; public uint Persist;
    public uint AttributeCount; public IntPtr Attributes; public IntPtr TargetAlias; public IntPtr UserName;
  }
  [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  public static extern bool CredRead(string target, uint type, uint flags, out IntPtr credential);
  [DllImport("advapi32.dll")] public static extern void CredFree(IntPtr buffer);
  public static string Probe(string target) {
    IntPtr p;
    if (!CredRead(target, 1u, 0u, out p)) return "absent|0|";
    try {
      var c = (CREDENTIAL)Marshal.PtrToStructure(p, typeof(CREDENTIAL));
      long ft = ((long)c.LastWritten.dwHighDateTime << 32) | (uint)c.LastWritten.dwLowDateTime;
      return "present|" + c.CredentialBlobSize + "|" + ft;
    } finally { CredFree(p); }
  }
}
'@
Add-Type -TypeDefinition $sig -Language CSharp | Out-Null
[TextreeCredProbe]::Probe($env:TEXTREE_PROBE_TARGET)
`;

/** Reads one credential entry. Throws only if PowerShell itself cannot run. */
export function readCredential(target: string): CredentialRecord {
  const out = execFileSync(
    "powershell.exe",
    ["-NoProfile", "-NonInteractive", "-Command", PROBE_SCRIPT],
    { encoding: "utf8", env: { ...process.env, TEXTREE_PROBE_TARGET: target } },
  ).trim();
  const [state, size, lastWritten] = out.split("|");
  return { present: state === "present", size: Number(size) || 0, lastWritten: lastWritten ?? "" };
}

/** True when two readings describe the same untouched entry (including "absent both times"). */
export function sameRecord(a: CredentialRecord, b: CredentialRecord): boolean {
  return a.present === b.present && a.size === b.size && a.lastWritten === b.lastWritten;
}
