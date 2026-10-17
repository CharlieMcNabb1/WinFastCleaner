; Extra steps for the NSIS installer (see tauri.bundle.conf.json). Jumps use
; labels, not relative offsets: plugin calls take more than one instruction.
;
; The installer never starts WinFastCleaner's programs itself: it runs with
; administrator rights, and the tray monitor must run as the user. The app
; starts the tray again (if it is turned on) the next time it opens, and it
; also starts at sign-in.

; Before installing over an older version: close the tray monitor so its
; program file can be replaced.
!macro NSIS_HOOK_PREINSTALL
  IfFileExists "$INSTDIR\wfc.exe" 0 wfc_preinstall_done
    nsExec::Exec '"$INSTDIR\wfc.exe" close-tray'
    Pop $0
  wfc_preinstall_done:
!macroend

; Before uninstalling: remove what WinFastCleaner added to Windows when the
; user turned it on (the scheduled cleaning task and the tray monitor's
; sign-in entry) and close the tray. The data folder
; (%LOCALAPPDATA%\WinFastCleaner: settings, logs, and the recovery bin, which
; may still hold the user's files) is left in place on purpose.
!macro NSIS_HOOK_PREUNINSTALL
  IfFileExists "$INSTDIR\wfc.exe" 0 wfc_preuninstall_done
    nsExec::Exec '"$INSTDIR\wfc.exe" remove-integration'
    Pop $0
  wfc_preuninstall_done:
!macroend
