@echo off
setlocal
rem Run existing user configuration, then isolate the Rust cache from old plum.
if exist "%~dp0rime-install-config.bat" call "%~dp0rime-install-config.bat"
if not defined PLUM_RS_DIR set "PLUM_RS_DIR=%LOCALAPPDATA%\plum-rs"
"%~dp0rime-install.exe" --plum-dir "%PLUM_RS_DIR%" %*
exit /b %ERRORLEVEL%
