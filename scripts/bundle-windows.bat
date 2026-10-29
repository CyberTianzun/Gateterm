@echo off
REM Windows 侧不需要 bundle:单个 exe 就是全部产物(图标已嵌进二进制)。
REM 这个脚本只负责把 gateterm-desktop-web.exe 与 gateterm-desktop-native.exe 挪到统一的输出目录,和 macOS 那边对齐。
setlocal
set TARGET=%1
if "%TARGET%"=="" set TARGET=x86_64-pc-windows-msvc
set ROOT=%~dp0..
set WEB_BIN=%ROOT%\target\%TARGET%\release\gateterm-desktop-web.exe
set NATIVE_BIN=%ROOT%\clients\desktop-native\target\%TARGET%\release\gateterm-desktop-native.exe
if not exist "%WEB_BIN%" (
  echo 找不到 %WEB_BIN%,先跑 cargo build --release --target %TARGET% -p gateterm-desktop-web 1>&2
  exit /b 1
)
if not exist "%NATIVE_BIN%" (
  echo 找不到 %NATIVE_BIN%,先跑 cargo build --release --target %TARGET% --manifest-path clients/desktop-native/Cargo.toml -p gateterm-desktop-native 1>&2
  exit /b 1
)
if not exist "%ROOT%\target\bundle" mkdir "%ROOT%\target\bundle"
copy /Y "%WEB_BIN%" "%ROOT%\target\bundle\gateterm-desktop-web.exe" >nul
copy /Y "%NATIVE_BIN%" "%ROOT%\target\bundle\gateterm-desktop-native.exe" >nul
echo %ROOT%\target\bundle\gateterm-desktop-web.exe
echo %ROOT%\target\bundle\gateterm-desktop-native.exe
