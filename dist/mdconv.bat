@echo off
rem MDeditor - command line converter wrapper
rem   mdconv.bat report.pdf
rem   mdconv.bat D:\docs -r -d D:\docs\md
rem   mdconv.bat paper.docx --stdout > paper.md
rem Keep this file next to MDeditor.exe, or add that folder to PATH.
chcp 65001 >nul
set "EXE=%~dp0MDeditor.exe"
if not exist "%EXE%" (
  echo [error] cannot find "%EXE%"
  echo Please keep mdconv.bat in the same folder as MDeditor.exe.
  exit /b 1
)
"%EXE%" --to-md %*
exit /b %ERRORLEVEL%
