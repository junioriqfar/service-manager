Option Explicit

Dim shell, fso, scriptDir, actionScript, command, exitCode

Set shell = CreateObject("WScript.Shell")
Set fso = CreateObject("Scripting.FileSystemObject")

scriptDir = fso.GetParentFolderName(WScript.ScriptFullName)
actionScript = fso.BuildPath(scriptDir, "Run-UpdaterTaskAction.ps1")

If Not fso.FileExists(actionScript) Then
    WScript.Quit 1
End If

command = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File """ & actionScript & """"
exitCode = shell.Run(command, 0, True)
WScript.Quit exitCode
