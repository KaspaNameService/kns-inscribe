Param(
  [string]$Version = $env:KNS_INSCRIBE_VERSION,
  [string]$BinDir = $env:KNS_INSCRIBE_BIN_DIR
)

$ErrorActionPreference = "Stop"

$Repo = "KaspaNameService/kns-inscribe"
$BinName = "kns-inscribe"

if ([string]::IsNullOrWhiteSpace($Version)) { $Version = "latest" }
if ([string]::IsNullOrWhiteSpace($BinDir)) {
  $BinDir = Join-Path $HOME "bin"
}

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
switch ($arch) {
  "x64" { $archId = "x86_64" }
  default { throw "Unsupported architecture: $arch" }
}

$suffix = "windows-$archId"
$asset = "$BinName-$suffix.exe"

if ($Version -eq "latest") {
  $baseUrl = "https://github.com/$Repo/releases/latest/download"
} else {
  $baseUrl = "https://github.com/$Repo/releases/download/$Version"
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("kns-inscribe-" + [System.Guid]::NewGuid().ToString("n"))
New-Item -ItemType Directory -Force $tmp | Out-Null

try {
  Write-Host "Downloading $BinName ($suffix) from $Repo ($Version)..."

  $binPath = Join-Path $tmp $asset
  $shaPath = "$binPath.sha256"

  Invoke-WebRequest -Uri "$baseUrl/$asset" -OutFile $binPath
  Invoke-WebRequest -Uri "$baseUrl/$asset.sha256" -OutFile $shaPath

  $shaLine = (Get-Content -Path $shaPath -Raw).Trim()
  $expected = ($shaLine -split "\s+")[0].ToLowerInvariant()
  if ([string]::IsNullOrWhiteSpace($expected)) { throw "Checksum file format unexpected: $shaPath" }

  $actual = (Get-FileHash -Algorithm SHA256 -Path $binPath).Hash.ToLowerInvariant()
  if ($expected -ne $actual) {
    throw "Checksum mismatch for $asset. Expected $expected, got $actual"
  }

  New-Item -ItemType Directory -Force $BinDir | Out-Null
  $dest = Join-Path $BinDir "$BinName.exe"
  Copy-Item -Force $binPath $dest

  Write-Host "Installed: $dest"
  Write-Host "Verify: $BinName --version"

  if ((($env:PATH ?? "") -split ";") -notcontains $BinDir) {
    Write-Host "NOTE: $BinDir is not on PATH. Add it (User PATH) in Windows Settings or run:"
    Write-Host "  setx PATH \"$BinDir;$env:PATH\""
  }
}
finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
