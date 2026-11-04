@echo off
rem Build a standalone release exe (static C runtime) and copy it beside this script.
rem   build.cmd        GPU speech recognition if the Vulkan SDK is installed, else CPU
rem   build.cmd cpu    force a CPU-only build
rem Full output is saved to build.log.
setlocal
cd /d "%~dp0"
set RUSTFLAGS=-C target-feature=+crt-static
rem Short build folder (whisper.cpp's Vulkan shader sub-build nests very deep).
set CARGO_TARGET_DIR=C:\bns

rem ---- Visual Studio compiler environment (cl.exe, INCLUDE, LIB) for CMake + Ninja
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" (
  echo Cannot find vswhere.exe - is Visual Studio Build Tools installed?
  exit /b 1
)
for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSINSTALL=%%i"
if not defined VSINSTALL (
  echo Visual Studio C++ build tools not found. Install "Desktop development with C++".
  exit /b 1
)
call "%VSINSTALL%\VC\Auxiliary\Build\vcvars64.bat" >nul
if errorlevel 1 (
  echo vcvars64.bat failed
  exit /b 1
)

rem ---- Build whisper.cpp with Ninja instead of Visual Studio projects
rem (MSBuild projects hit path-length and file-tracking problems in ggml's nested sub-build)
where ninja >nul 2>nul
if errorlevel 1 (
  echo Ninja not found. Install it with:  winget install Ninja-build.Ninja
  echo then open a new terminal and run build.cmd again.
  exit /b 1
)
set CMAKE_GENERATOR=Ninja
rem Static C runtime for whisper.cpp too (otherwise CMake adds /MD and linking fails with __imp_ symbols)
set "TOOLCHAIN=%~dp0cmake\static-crt.cmake"
set "CMAKE_TOOLCHAIN_FILE=%TOOLCHAIN:\=/%"
rem CMake cannot switch generator in an existing build folder: clear whisper.cpp's old one once.
if not exist "%CARGO_TARGET_DIR%\.ninja-mt2" (
  for /d %%d in ("%CARGO_TARGET_DIR%\x86_64-pc-windows-msvc\release\build\whisper-rs-sys-*") do rmdir /s /q "%%d"
  if not exist "%CARGO_TARGET_DIR%" mkdir "%CARGO_TARGET_DIR%"
  echo.> "%CARGO_TARGET_DIR%\.ninja-mt2"
)

rem whisper-rs needs libclang (LLVM) to generate its bindings
if not defined LIBCLANG_PATH if exist "C:\Program Files\LLVM\bin\libclang.dll" set "LIBCLANG_PATH=C:\Program Files\LLVM\bin"

set FEATURES=
if /i "%~1"=="cpu" goto cpu
if defined VULKAN_SDK (
  set FEATURES=--features gpu
  echo Vulkan SDK found: building with GPU speech recognition
  goto build
)
:cpu
echo Building CPU-only speech recognition
:build
echo Building in %CARGO_TARGET_DIR% with Ninja ... full output goes to build.log
cargo build --release --target x86_64-pc-windows-msvc %FEATURES% -vv > build.log 2>&1
if errorlevel 1 goto failed
copy /y "%CARGO_TARGET_DIR%\x86_64-pc-windows-msvc\release\beamng-speak.exe" . >nul
echo Built %~dp0beamng-speak.exe
exit /b 0
:failed
echo.
echo Build failed. Last lines of build.log:
powershell -NoProfile -Command "Get-Content -Path build.log -Tail 40"
exit /b 1
