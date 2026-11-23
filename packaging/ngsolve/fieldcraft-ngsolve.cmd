@echo off
setlocal
"%~dp0fieldcraft-cli.exe" ngsolve %* --python "%~dp0python\python.exe"
exit /b %errorlevel%
