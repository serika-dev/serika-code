@echo off
setlocal
if exist "%~dp0serika.exe" (
    "%~dp0serika.exe" setup %*
    exit /b %ERRORLEVEL%
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
