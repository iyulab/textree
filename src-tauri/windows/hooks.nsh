; Custom NSIS installer hooks for Textree.
;
; Two file locks can make the installer fail with "Error opening file for writing"
; while it overwrites files during extraction:
;
;   1. host\textree-host.exe -- Textree spawns a self-contained .NET sidecar
;      (textree-host.exe) for local AI. On a normal app exit the Rust side runs
;      shutdown_host (RunEvent::ExitRequested), but an update/reinstall force-kills
;      only the main binary, orphaning the sidecar which keeps a lock on its exe.
;
;   2. textree.exe (the main binary itself) -- the generated template runs, in order:
;         NSIS_HOOK_PREINSTALL  ->  CheckIfAppIsRunning (kills textree.exe)  ->  File (extracts textree.exe)
;      CheckIfAppIsRunning kills textree.exe with NO delay before the very next
;      instruction extracts it, so on a running install Windows has often not yet
;      released the file handle -> the write fails. (Clicking Retry "works" only
;      because the handle is released by the time the user reacts.)
;
; This PREINSTALL hook runs BEFORE CheckIfAppIsRunning, so terminating BOTH binaries
; here plus the trailing Sleep gives the OS time to release every handle before
; extraction begins; CheckIfAppIsRunning then finds nothing running and is a no-op.
;   - textree-host.exe uses /T to also reap any child processes it spawned.
;   - textree.exe uses NO /T on purpose: in the updater flow the installer process is
;     a child of the running textree.exe, and /T would kill the installer itself.
; Best-effort: taskkill exits nonzero when no such process exists, which we ignore.
; nsExec::Exec runs the command hidden (no console flash).
;   - PREINSTALL   runs before the main binary + resources are extracted.
;   - PREUNINSTALL runs before the install dir is deleted.

!macro NSIS_HOOK_PREINSTALL
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /T /IM textree-host.exe'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM textree.exe'
  Pop $0
  Sleep 500
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /T /IM textree-host.exe'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM textree.exe'
  Pop $0
  Sleep 500
!macroend
