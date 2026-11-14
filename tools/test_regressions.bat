@echo off
rem runs tools\test_regressions.ps1 from cmd or Explorer; arguments pass through (-Quick)
setlocal
cd /d "%~dp0\.."
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0test_regressions.ps1" %*
exit /b %ERRORLEVEL%
