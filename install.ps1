# Installs the prumo binary from the latest GitHub release. One line, in PowerShell:
#
#   irm https://github.com/kevinraymond/prumo/raw/main/install.ps1 | iex
#
# It downloads the Windows x86-64 zip, checks it against the sha256 published next to it,
# puts prumo.exe in %LOCALAPPDATA%\Programs\prumo, and adds that directory to your user
# PATH when it is not there (open a new terminal afterwards). Nothing else is changed.
#
#   PRUMO_VERSION      the release to install, e.g. v0.1.0 (default: the latest)
#   PRUMO_INSTALL_DIR  where the binary goes (default %LOCALAPPDATA%\Programs\prumo)
#   PRUMO_NO_PATH      set to 1 to leave PATH alone
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$repo = 'kevinraymond/prumo'
$target = 'x86_64-pc-windows-msvc'
$version = $env:PRUMO_VERSION
$dir = if ($env:PRUMO_INSTALL_DIR) { $env:PRUMO_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\prumo' }
$dir = $dir.TrimEnd('\')

if (-not $version) {
    $latest = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/latest" -Headers @{ 'User-Agent' = 'prumo-install' }
    $version = $latest.tag_name
    if (-not $version) { throw "could not tell the latest release of $repo" }
}

$name = "prumo-$version-$target"
$base = "https://github.com/$repo/releases/download/$version"
$tmp = Join-Path ([IO.Path]::GetTempPath()) "prumo-install-$([IO.Path]::GetRandomFileName())"
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Host "prumo: downloading $name.zip"
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$name.zip" -OutFile (Join-Path $tmp "$name.zip")
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$name.zip.sha256" -OutFile (Join-Path $tmp 'sum')

    $want = (Get-Content (Join-Path $tmp 'sum') -Raw).Trim().ToLower()
    $have = (Get-FileHash (Join-Path $tmp "$name.zip") -Algorithm SHA256).Hash.ToLower()
    if ($have -ne $want) { throw "sha256 mismatch for $name.zip: got $have, the release says $want" }

    Expand-Archive -Path (Join-Path $tmp "$name.zip") -DestinationPath $tmp -Force
    $exe = Join-Path $tmp "$name\prumo.exe"
    if (-not (Test-Path $exe)) { throw "$name.zip has no prumo.exe in it" }

    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    $installed = Join-Path $dir 'prumo.exe'
    $replaced = if (Test-Path $installed) { ' (replacing the one that was there)' } else { '' }
    Copy-Item $exe $installed -Force
    $v = & $installed --version
    Write-Host "prumo: installed $v in $dir$replaced"
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

if (-not $env:PRUMO_NO_PATH) {
    $parts = @([Environment]::GetEnvironmentVariable('Path', 'User') -split ';' | Where-Object { $_ } | ForEach-Object { $_.TrimEnd('\') })
    if ($parts -notcontains $dir) {
        [Environment]::SetEnvironmentVariable('Path', (($parts + $dir) -join ';'), 'User')
        $env:Path = "$env:Path;$dir"
        Write-Host "prumo: added $dir to your user PATH. Open a new terminal to use it"
    }
}
Write-Host 'prumo: next, `prumo doctor` says what this machine can do'
