@echo off
setlocal
cd /d "%~dp0"

set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" goto no_vs

for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSPATH=%%i"
if not defined VSPATH goto no_vs

call "%VSPATH%\VC\Auxiliary\Build\vcvars64.bat" >nul
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

cd /d "%~dp0src-tauri"
cargo build --release
if errorlevel 1 goto failed

echo.
echo   Build finished: src-tauri\target\release\agent-hub.exe
echo.
pause
exit /b 0

:no_vs
echo.
echo   Visual Studio Build Tools with the C++ workload is required.
echo   https://visualstudio.microsoft.com/visual-cpp-build-tools/
echo.
pause
exit /b 1

:failed
echo.
echo   Build failed. See the messages above.
pause
exit /b 1
