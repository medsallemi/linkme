; Both installer sections use the session-local mutex of Tauri single-instance 2.5.5.
; SYNCHRONIZE opens an observation handle. No process is sent a shutdown message.
!macro LinkmeRequireStopped
  !define LinkmeGuardID ${__LINE__}
  linkme_retry_${LinkmeGuardID}:
  System::Call 'kernel32::OpenMutexW(i 0x00100000, i 0, w "${BUNDLEID}-sim") p .r0 ?e'
  Pop $1
  ${If} $0 P= 0
    ${If} $1 = 2 ; ERROR_FILE_NOT_FOUND
      Goto linkme_stopped_${LinkmeGuardID}
    ${EndIf}
  ${Else}
    System::Call 'kernel32::CloseHandle(p r0) i'
  ${EndIf}
  ; A silent run defaults to cancellation. The user quits from the tray.
  MessageBox MB_RETRYCANCEL|MB_ICONINFORMATION "Linkme is running, or Setup could not verify that it has stopped. Choose Quit from Linkme's notification-area menu, then select Retry." /SD IDCANCEL IDRETRY linkme_retry_${LinkmeGuardID}
  SetErrorLevel 2
  Abort "Setup was canceled before changing Linkme."
  linkme_stopped_${LinkmeGuardID}:
  !undef LinkmeGuardID
!macroend

; The open handle locks this Windows user's profile against overlapping Setup/Uninstall.
; Windows releases the handle and deletes the lock when Setup exits.
!macro LinkmeLockSetup
  System::Call 'kernel32::CreateFileW(w "$LOCALAPPDATA\Linkme-Setup.lock", i 0x40010000, i 0, p 0, i 4, i 0x04000080, p 0) p .r0'
  ${If} $0 P= -1
    MessageBox MB_OK|MB_ICONEXCLAMATION "Another Linkme setup is running, or this profile cannot be locked. Close other Linkme setup windows and retry." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
!macroend

Var LinkmeExisting
Var LinkmeStartupEnabled

!macro LinkmeInitializeSetup
  !insertmacro LinkmeLockSetup
  ReadRegStr $LinkmeExisting HKCU "${MANUPRODUCTKEY}" ""
  ${If} $LinkmeExisting != ""
    StrCpy $INSTDIR $LinkmeExisting
  ${EndIf}
  StrCpy $LinkmeStartupEnabled 1
  ClearErrors
  ReadRegDWORD $0 HKCU "Software\Linkme" "StartupEnabled"
  ${IfNot} ${Errors}
    StrCpy $LinkmeStartupEnabled $0
  ${Else}
    ReadRegDWORD $0 HKCU "Software\Linkme" "StartupDefaultApplied"
    ReadRegStr $1 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Linkme"
    ${If} $0 == 1
    ${AndIf} $1 == ""
      StrCpy $LinkmeStartupEnabled 0
    ${EndIf}
  ${EndIf}
!macroend

!macro LinkmeSetupFunctions
Function LinkmeSkipExisting
  ${If} $LinkmeExisting != ""
  ${OrIf} $PassiveMode = 1
    Abort
  ${EndIf}
FunctionEnd

!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; The browser return link launches only a fixed command. It passes no URL or OAuth data.
  StrCpy $1 "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" --open-library"
  ReadRegStr $0 HKCU "Software\Classes\linkme-desktop\shell\open\command" ""
  ${If} $0 == ""
    ReadRegStr $0 HKLM "Software\Classes\linkme-desktop\shell\open\command" ""
  ${EndIf}
  ${If} $0 == ""
  ${OrIf} $0 == $1
    WriteRegStr HKCU "Software\Classes\linkme-desktop" "" "URL:Linkme Desktop"
    WriteRegStr HKCU "Software\Classes\linkme-desktop" "URL Protocol" ""
    WriteRegStr HKCU "Software\Classes\linkme-desktop\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
    WriteRegStr HKCU "Software\Classes\linkme-desktop\shell\open\command" "" $1
  ${EndIf}
  ; Repair and upgrade stay in the registered location and leave the existing choice alone.
  ${If} $LinkmeExisting == ""
    StrCpy $1 "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" --windows-startup"
    ClearErrors
    ${If} $LinkmeStartupEnabled = 1
      StrLen $2 $1
      ${If} $2 > 260
        SetErrors
      ${Else}
        WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Linkme" $1
      ${EndIf}
    ${Else}
      ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Linkme"
      ClearErrors
      ${If} $0 == $1
        DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Linkme"
      ${EndIf}
    ${EndIf}
    ${IfNot} ${Errors}
      WriteRegDWORD HKCU "Software\Linkme" "StartupEnabled" $LinkmeStartupEnabled
      WriteRegDWORD HKCU "Software\Linkme" "StartupDefaultApplied" 1
    ${EndIf}
    ${If} ${Errors}
      ; A failed Setup registration should be resolved explicitly in settings.
      WriteRegDWORD HKCU "Software\Linkme" "StartupDefaultApplied" 1
      MessageBox MB_OK|MB_ICONEXCLAMATION "Linkme was installed, but Windows could not configure startup. Check Linkme settings or Windows Startup Apps." /SD IDOK
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ReadRegStr $0 HKCU "Software\Classes\linkme-desktop\shell\open\command" ""
  ${If} $0 == "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" --open-library"
    DeleteRegKey HKCU "Software\Classes\linkme-desktop"
  ${EndIf}
  ; Leave another Linkme copy's startup entry and the explicit preference marker alone.
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Linkme"
  ${If} $0 == "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" --windows-startup"
    WriteRegDWORD HKCU "Software\Linkme" "StartupEnabled" 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Linkme"
  ${ElseIf} $0 == ""
    WriteRegDWORD HKCU "Software\Linkme" "StartupEnabled" 0
  ${EndIf}
  DeleteRegValue HKCU "${MANUPRODUCTKEY}" ""
  DeleteRegValue HKCU "${MANUPRODUCTKEY}" "Installer Language"
  DeleteRegValue HKCU "${MANUPRODUCTKEY}" "StartMenuFolder"
  DeleteRegKey /ifempty HKCU "${MANUPRODUCTKEY}"
!macroend
