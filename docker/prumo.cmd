@echo off
rem Runs prumo from the Docker image, as a command, on Windows with Docker Desktop. Put this
rem file in a directory on your PATH and run it from the directory your document is in:
rem
rem   prumo time main.tex
rem
rem It types the docker run line so you don't have to. The current directory and your prumo
rem cache directory are mounted inside the container (C:\Users\you\thesis becomes
rem /C/Users/you/thesis) and prumo is told both names, so the paths it prints are Windows
rem paths. Runs survive the container and `prumo clean` sees them. Docker gets a terminal
rem only when output is not redirected: `time` draws its charts at the prompt, and
rem `prumo time --json > report.json` stays clean.
rem
rem The container sees only the current directory.
rem
rem   PRUMO_IMAGE        the image to run (default ghcr.io/kevinraymond/prumo)
rem   PRUMO_CACHE        the cache directory (default %LOCALAPPDATA%\prumo)
rem   PRUMO_DOCKER       the container engine (default docker; podman works)
rem   PRUMO_DOCKER_OPTS  extra options for docker run, e.g. --platform linux/amd64
setlocal

set "image=%PRUMO_IMAGE%"
if "%image%"=="" set "image=ghcr.io/kevinraymond/prumo"
set "engine=%PRUMO_DOCKER%"
if "%engine%"=="" set "engine=docker"
set "cache=%PRUMO_CACHE%"
if "%cache%"=="" set "cache=%LOCALAPPDATA%\prumo"

where %engine% >nul 2>&1
if errorlevel 1 (
    echo prumo: %engine% is not installed or not on PATH; this command runs prumo from the %image% image >&2
    exit /b 1
)

if not exist "%cache%" mkdir "%cache%"
set "cwd=%CD%"
call :inside "%cwd%" cwd_in
call :inside "%cache%" cache_in

rem -t only when stdout is the console; PowerShell knows, cmd does not.
set "tty=-t"
for /f %%r in ('powershell -NoProfile -Command "[Console]::IsOutputRedirected" 2^>nul') do if /i "%%r"=="True" set "tty="

%engine% run --rm %tty% %PRUMO_DOCKER_OPTS% ^
    -v "%cwd%:%cwd_in%" -w "%cwd_in%" ^
    -v "%cache%:%cache_in%" -e "PRUMO_CACHE=%cache_in%" ^
    -e "PRUMO_PATH_MAP=%cwd_in%=%cwd%;%cache_in%=%cache%" ^
    -e "PRUMO_IMAGE=%image%" -e NO_COLOR -e COLUMNS ^
    "%image%" %*
exit /b %errorlevel%

rem C:\Users\you\thesis -> /C/Users/you/thesis (the path inside the container).
:inside
set "p=%~1"
set "p=%p:\=/%"
set "%~2=/%p:~0,1%%p:~2%"
exit /b 0
