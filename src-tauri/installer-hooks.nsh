; Frame Player's additions to Tauri's generated NSIS installer.
;
; Reached through `bundle.windows.nsis.installerHooks`, which the template
; `!include`s at its line 35 - before its own `Var` declarations and, crucially,
; before it defines its pages. That is what lets this file do more than the four
; `NSIS_HOOK_*` macros allow: those are inserted *inside* sections, far too late
; to add anything to a page, but an include at the top of the script can define
; `MUI_PAGE_CUSTOMFUNCTION_SHOW` for a page the template inserts afterwards.
; `MUI2.nsh` is included at line 22, so nsDialogs, WinMessages and LogicLib are
; all available here.
;
; What it adds is one checkbox on the directory page: whether this installation
; keeps its settings, watch history and caches beside the executable instead of
; in the user profile. The player decides that by the presence of `.portable` in
; its own directory (see src-tauri/src/portable.rs); all this does is write or
; remove that file, which is also why the same thing can be had by unpacking the
; portable archive, and why the two cannot disagree.
;
; The checkbox is English only, like the rest of this installer: the bundle does
; not set `nsis.languages`, so the template builds an English-only installer and
; a lone translated control would be the odd one out.

Var FpPortable          ; 1 when the box is ticked
Var FpPortableAsked     ; 1 when the page was actually shown
Var FpPortableCheckbox

!define FP_PORTABLE_LABEL "Keep settings and watch history in the installation folder, not in my user profile"

; The template defines this too, but at its line 427 — after this file is
; included, so it is not yet known here and the RTL branch below would parse
; with an empty style. `/ifndef` on both sides means whichever comes first wins
; and they cannot disagree.
!define /ifndef WS_EX_LAYOUTRTL 0x00400000

; **On the welcome page, and that is not a preference.** `MUI_PAGE_CUSTOMFUNCTION_SHOW`
; is a *generic* define: the first page inserted after it takes it and undefines
; it again. The template's first page is the welcome page (its line 170), so a
; definition made here can only ever reach that one — the directory page, where
; this question would sit more naturally, is nine pages of template later and
; unreachable from an include at the top. Measured rather than assumed: a probe
; with both pages puts the control on the welcome page and leaves the directory
; page bare. The finish page, the other candidate, has both of its checkbox
; slots taken already (desktop shortcut, run the player).
!define MUI_PAGE_CUSTOMFUNCTION_SHOW FpWelcomeShow
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE FpWelcomeLeave

; Adding a control to a stock MUI page means creating it on the inner dialog by
; hand, scaled by the window's DPI. The shape below - find `#32770`, ask for the
; DPI, scale, `CreateWindowEx`, inherit the parent's font - is the template's own
; (`un.ConfirmShow`, which adds the "delete app data" box to the uninstaller's
; confirm page), followed here so that the two look alike.
Function FpWelcomeShow
  ; $1 inner dialog, $2 DPI, $3 exstyle, $4..$7 x y w h
  FindWindow $1 "#32770" "" $HWNDPARENT
  System::Call "user32::GetDpiForWindow(p r1) i .r2"
  ${If} $(^RTL) = 1
    StrCpy $3 "${__NSD_CheckBox_EXSTYLE} | ${WS_EX_LAYOUTRTL}"
  ${Else}
    StrCpy $3 "${__NSD_CheckBox_EXSTYLE}"
  ${EndIf}
  ; Measured on the page rather than guessed. The welcome page's inner dialog is
  ; 497x305 logical units: the image panel takes x 0-163, and the text static
  ; spans y 71-277 across the rest — generously, far past where its words end.
  ; A control placed inside that span exists and reports itself visible, and the
  ; static paints straight over it; the free band is underneath, which is also
  ; where an installer's options conventionally sit — with room for the two
  ; lines the label wraps to, which a 16-unit control cut in half.
  IntOp $4 185 * $2
  IntOp $5 275 * $2
  IntOp $6 305 * $2
  IntOp $7 28 * $2
  IntOp $4 $4 / 96
  IntOp $5 $5 / 96
  IntOp $6 $6 / 96
  IntOp $7 $7 / 96
  ; Created with no title, and the text set afterwards. `System::Call` splits
  ; its argument list on commas, so a label containing one silently shifts every
  ; parameter after it: the first version of this had a comma in the sentence
  ; and produced an untitled, invisible checkbox at coordinates nobody asked
  ; for. Out of the call, punctuation cannot reach the parser.
  System::Call 'user32::CreateWindowEx(i r3, w "${__NSD_CheckBox_CLASS}", w "", i ${__NSD_CheckBox_STYLE}, i r4, i r5, i r6, i r7, p r1, i0, i0, i0) i .s'
  Pop $FpPortableCheckbox
  SendMessage $FpPortableCheckbox ${WM_SETTEXT} 0 "STR:${FP_PORTABLE_LABEL}"
  SendMessage $HWNDPARENT ${WM_GETFONT} 0 0 $1
  SendMessage $FpPortableCheckbox ${WM_SETFONT} $1 1

  ; Show what is true rather than a default: re-running the installer over a
  ; folder that already keeps its state beside itself should arrive with the box
  ; ticked, or unticking it would be the only way to leave things as they are.
  ${If} ${FileExists} "$INSTDIR\.portable"
    SendMessage $FpPortableCheckbox ${BM_SETCHECK} ${BST_CHECKED} 0
  ${EndIf}
FunctionEnd

Function FpWelcomeLeave
  SendMessage $FpPortableCheckbox ${BM_GETCHECK} 0 0 $FpPortable
  StrCpy $FpPortableAsked 1
FunctionEnd

!macro NSIS_HOOK_POSTINSTALL
  ; **Only when the page was shown.** The updater runs this installer with
  ; `/P /UPDATE`, which skips every page, and a silent run skips them too - so
  ; without this guard an unanswered checkbox would read as "unticked" and every
  ; update would quietly delete the marker and move a portable installation's
  ; state back into the user profile, losing sight of its watch history.
  ${If} $FpPortableAsked = 1
    ${If} $FpPortable = 1
      ; The player only checks that this exists; the text is for whoever opens
      ; it wondering what it is.
      FileOpen $0 "$INSTDIR\.portable" w
      FileWrite $0 "Frame Player keeps its settings, watch history and caches in the .data$\r$\nfolder beside this file, instead of in the Windows user profile. Delete this$\r$\nfile to go back to the profile.$\r$\n"
      FileClose $0
    ${Else}
      Delete "$INSTDIR\.portable"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Here rather than in POSTUNINSTALL, because the template's own
  ; `RMDir "$INSTDIR"` runs in between: anything of ours still on disk by then
  ; leaves the installation directory behind for the sake of a marker file.
  ;
  ; `$DeleteAppDataCheckboxState` is the uninstaller's own checkbox, read on its
  ; confirm page before this section runs. The state beside the executable is
  ; this installation's app data, so it answers to the same question the profile
  ; directories answer to - and is kept for the same reason when the answer is
  ; no.
  Delete "$INSTDIR\.portable"
  ${If} $DeleteAppDataCheckboxState = 1
    RMDir /r "$INSTDIR\.data"
  ${EndIf}
!macroend
