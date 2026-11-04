@echo off
rem MDeditor - command line converter wrapper
rem   mdconv.bat report.pdf
rem   mdconv.bat D:\docs -r -d D:\docs\md
rem   mdconv.bat paper.docx --stdout > paper.md
rem Keep this file next to MDeditor.exe, or add that folder to PATH.
rem Switch to UTF-8 for Chinese paths, then restore the caller's code page.
set "OLDCP="
for /f "tokens=2 delims=:." %%A in ('chcp') do set "OLDCP=%%A"
chcp 65001 >nul
set "EXE=%~dp0MDeditor.exe"
if not exist "%EXE%" (
  echo [error] cannot find "%EXE%"
  echo Please keep mdconv.bat in the same folder as MDeditor.exe.
  if defined OLDCP chcp %OLDCP% >nul
  exit /b 1
)
"%EXE%" --to-md %*
set "RC=%ERRORLEVEL%"
if defined OLDCP chcp %OLDCP% >nul
exit /b %RC%
