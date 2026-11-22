@echo off
setlocal
set "PYTHONHOME="
set "PYTHONPATH="
set "PYTHONNOUSERSITE=1"
"%~dp0python\python.exe" "%~dp0bridge\workflow.py" %*
exit /b %errorlevel%
