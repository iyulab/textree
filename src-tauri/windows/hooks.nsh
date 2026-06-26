; Custom NSIS installer hooks for Textree.
;
; Textree spawns a self-contained .NET sidecar (textree-host.exe) for local AI.
; On a normal app exit the Rust side runs shutdown_host (RunEvent::ExitRequested),
; but an update or reinstall force-kills only the main binary (textree.exe) via the
; template's CheckIfAppIsRunning macro. That hard kill bypasses the graceful exit
; path, so the sidecar is orphaned and keeps a file lock on host\textree-host.exe.
; The installer then fails with "Error opening file for writing" when it tries to
; overwrite that file during extraction.
;
; These hooks terminate the sidecar before the installer touches its files:
;   - PREINSTALL  runs before the resources (including host\) are extracted.
;   - PREUNINSTALL runs before the install dir is deleted.
; Best-effort: taskkill exits nonzero when no such process exists, which we ignore.
; nsExec::Exec runs the command hidden (no console flash). The short Sleep gives the
; OS a moment to release the file handle after termination before extraction begins.

!macro NSIS_HOOK_PREINSTALL
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /T /IM textree-host.exe'
  Pop $0
  Sleep 500
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /T /IM textree-host.exe'
  Pop $0
  Sleep 500
!macroend
