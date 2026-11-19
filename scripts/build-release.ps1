# Construye anyremoted (Rust) + anyremote_app (Flutter) con la versión de VERSION.
#
# Uso (desde cualquier carpeta):
#   powershell -ExecutionPolicy Bypass -File scripts\build-release.ps1
#
# La versión vive en `VERSION` (raíz del repo) y se inyecta en:
#   - el daemon (`core/daemon/Cargo.toml` la declara; el script comprueba que coincide),
#   - la app (`--dart-define=APP_VERSION`, fuente de la UI) y
#   - los metadatos del .exe (`--build-name`, VERSIONINFO de Windows).
# Si `VERSION` y `app/pubspec.yaml` no coinciden, el script NO compila (evita
# volver a publicar un binario con la versión equivocada).

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

$version = (Get-Content (Join-Path $root 'VERSION') -Raw).Trim()
if (-not $version) { throw 'VERSION está vacío' }

$pubspec = Join-Path $root 'app/pubspec.yaml'
$m = Select-String -Path $pubspec -Pattern '^version:\s*([0-9]+\.[0-9]+\.[0-9]+)' | Select-Object -First 1
if (-not $m) { throw "app/pubspec.yaml sin 'version:'" }
$pubspecVersion = $m.Matches[0].Groups[1].Value
if ($pubspecVersion -ne $version) {
    throw "app/pubspec.yaml ($pubspecVersion) no coincide con VERSION ($version): actualiza ambos"
}

$cargo = Join-Path $root 'core/daemon/Cargo.toml'
$c = Select-String -Path $cargo -Pattern '^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"' | Select-Object -First 1
if (-not $c) { throw "core/daemon/Cargo.toml sin 'version ='" }
$cargoVersion = $c.Matches[0].Groups[1].Value
if ($cargoVersion -ne $version) {
    throw "core/daemon/Cargo.toml ($cargoVersion) no coincide con VERSION ($version): actualiza ambos"
}

$git = (git -C $root rev-parse --short HEAD).Trim()
$flutterVer = ((flutter --version 2>$null | Select-Object -First 1) -replace '^Flutter\s+', '').Trim()
if (-not $flutterVer) { $flutterVer = 'desconocida' }

Write-Host "== anyremote $version ($git) =="

Push-Location (Join-Path $root 'core')
try {
    cargo build --release -p anyremote-daemon --bin anyremoted
} finally { Pop-Location }

Push-Location (Join-Path $root 'app')
try {
    flutter build windows --release `
        --build-name=$version --build-number=1 `
        --dart-define=APP_VERSION=$version `
        --dart-define=GIT_REV=$git `
        --dart-define=FLUTTER_BUILD=$flutterVer
} finally { Pop-Location }

# El daemon debe quedar junto a la app: resolveDaemonPath() lo busca ahí.
$release = Join-Path $root 'app/build/windows/x64/runner/Release'
Copy-Item (Join-Path $root 'core/target/release/anyremoted.exe') $release -Force

Write-Host ''
Write-Host "OK v$version -> $release"
Get-ChildItem $release -Filter *.exe | Select-Object Name, Length
