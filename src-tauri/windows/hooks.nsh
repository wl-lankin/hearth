; Removes Hearth's "Start at login" entry when Hearth is uninstalled, so Windows does not try to
; start a program that is gone. The value name matches startup::LOGIN_ENTRY_NAME.
!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Hearth"
!macroend
