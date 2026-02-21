#Requires -Version 5.1
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Repo = "taradepan/reaper"
$Binary = "reaper.exe"

function Write-Info {
    param([string]$Message)
    Write-Host $Message -ForegroundColor Cyan
}

function Write-Err {
    param([string]$Message)
    Write-Host "error: $Message" -ForegroundColor Red
    exit 1
}

function Get-Target {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    switch ($arch) {
        "X64"   { return "x86_64-pc-windows-msvc" }
        default { Write-Err "Unsupported architecture: $arch. Only x86_64 Windows is currently supported." }
    }
}

function Get-LatestVersion {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
    $tag = $release.tag_name
    if (-not $tag) {
        Write-Err "Could not determine the latest version. Check https://github.com/$Repo/releases"
    }
    return $tag -replace '^v', ''
}

function Main {
    $target = Get-Target
    Write-Info "Detected target: $target"

    Write-Info "Fetching latest version..."
    $version = Get-LatestVersion
    Write-Info "Latest version: v$version"

    $archive = "reaper-v${version}-${target}.zip"
    $url = "https://github.com/$Repo/releases/download/v${version}/$archive"

    $tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("reaper-install-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

    try {
        $archivePath = Join-Path $tmpDir $archive
        Write-Info "Downloading $url..."
        Invoke-WebRequest -Uri $url -OutFile $archivePath -UseBasicParsing

        Write-Info "Extracting..."
        Expand-Archive -Path $archivePath -DestinationPath $tmpDir -Force

        $installDir = Join-Path $env:LOCALAPPDATA "reaper"
        if (-not (Test-Path $installDir)) {
            New-Item -ItemType Directory -Path $installDir -Force | Out-Null
        }

        $dest = Join-Path $installDir $Binary
        Move-Item -Path (Join-Path $tmpDir $Binary) -Destination $dest -Force

        # Add to PATH if not already there
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        if ($userPath -notlike "*$installDir*") {
            Write-Info "Adding $installDir to your PATH..."
            [Environment]::SetEnvironmentVariable("Path", "$userPath;$installDir", "User")
            $env:Path = "$env:Path;$installDir"
        }

        Write-Info "Installed reaper v$version to $dest"
        Write-Info "Run 'reaper --help' to get started."
        Write-Info ""
        Write-Info "NOTE: You may need to restart your terminal for PATH changes to take effect."
    }
    finally {
        Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Main