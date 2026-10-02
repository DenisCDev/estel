[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$version = '1.96.0'
$target = 'x86_64-pc-windows-msvc'
$installRoot = 'D:\estel-tools'
$toolchain = Join-Path $installRoot "rust-$version-$target"
$downloads = Join-Path $installRoot 'downloads'
$extraction = Join-Path $installRoot "packages-$version"

function Invoke-BoundedTar {
    param([string[]]$Arguments)
    $start = [Diagnostics.ProcessStartInfo]::new('C:\Windows\System32\tar.exe')
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    try {
        if (-not $process.Start()) { throw 'Não foi possível iniciar a extração.' }
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(120000)) {
            $process.Kill($true)
            throw 'A extração excedeu o prazo de 120 segundos.'
        }
        if ($process.ExitCode -ne 0) { throw "A extração falhou: $($stderr.GetAwaiter().GetResult())" }
        return $stdout.GetAwaiter().GetResult()
    } finally {
        $process.Dispose()
    }
}

function Get-VerifiedDownload {
    param([string]$Url, [string]$Destination, [string]$ExpectedHash)
    $uri = [uri]$Url
    if ($uri.Scheme -ne 'https' -or $uri.Host -ne 'static.rust-lang.org') {
        throw 'O endereço não pertence à distribuição oficial do Rust.'
    }
    if (-not (Test-Path -LiteralPath $Destination)) {
        Invoke-WebRequest -Uri $Url -OutFile $Destination -TimeoutSec 120 -MaximumRedirection 0
    }
    $actual = (Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $ExpectedHash.ToLowerInvariant()) {
        throw 'O SHA-256 não corresponde ao publicado. Nenhum binário será executado.'
    }
    return $actual
}

if (Test-Path -LiteralPath $toolchain) {
    throw 'A pasta da versão já existe; confira a instalação existente antes de continuar.'
}
[IO.Directory]::CreateDirectory($downloads) | Out-Null
[IO.Directory]::CreateDirectory($extraction) | Out-Null
$manifestUrl = "https://static.rust-lang.org/dist/channel-rust-$version.toml"
$publishedHash = (Invoke-WebRequest -Uri "$manifestUrl.sha256" -TimeoutSec 30 -MaximumRedirection 0).Content
if ($publishedHash -is [byte[]]) { $publishedHash = [Text.Encoding]::UTF8.GetString($publishedHash) }
$hashMatch = [regex]::Match([string]$publishedHash, '^[a-fA-F0-9]{64}')
if (-not $hashMatch.Success) { throw 'O SHA-256 publicado para o manifesto é inválido.' }
$manifestPath = Join-Path $downloads "channel-rust-$version.toml"
$manifestHash = Get-VerifiedDownload $manifestUrl $manifestPath $hashMatch.Value
$manifest = Get-Content -LiteralPath $manifestPath -Raw
$packages = [Collections.Generic.List[object]]::new()

foreach ($component in @('rustc', 'cargo', 'rust-std', 'rustfmt-preview', 'clippy-preview')) {
    $sectionName = [regex]::Escape("pkg.$component.target.$target")
    $section = [regex]::Match($manifest, "(?ms)^\[$sectionName\]\s*\r?\n(.*?)(?=^\[|\z)").Groups[1].Value
    $url = [regex]::Match($section, '(?m)^xz_url = "([^"]+)"').Groups[1].Value
    $expectedHash = [regex]::Match($section, '(?m)^xz_hash = "([a-f0-9]{64})"').Groups[1].Value
    if (-not $url -or -not $expectedHash) { throw "O manifesto não descreve o componente $component." }
    $archiveName = [IO.Path]::GetFileName(([uri]$url).AbsolutePath)
    $archive = Join-Path $downloads $archiveName
    Write-Output "Baixando e verificando $component $version."
    $archiveHash = Get-VerifiedDownload $url $archive $expectedHash
    $entries = (Invoke-BoundedTar -Arguments @('-tf', $archive)) -split '\r?\n' | Where-Object { $_ }
    $packageRoot = $archiveName -replace '\.tar\.xz$', ''
    foreach ($entry in $entries) {
        if (($entry -ne $packageRoot -and $entry -notlike "$packageRoot/*") -or $entry -match '(^|[/\\])\.\.([/\\]|$)|:|^[/\\]') {
            throw 'O pacote contém um caminho fora da pasta esperada.'
        }
    }
    Invoke-BoundedTar -Arguments @('-xf', $archive, '-C', $extraction) | Out-Null
    $packageDirectory = Join-Path $extraction $packageRoot
    $componentNames = Get-Content -LiteralPath (Join-Path $packageDirectory 'components')
    foreach ($componentName in $componentNames) {
        if ($componentName -notmatch '^[a-zA-Z0-9_-]+$') { throw 'O nome de um componente é inválido.' }
        $componentDirectory = Join-Path $packageDirectory $componentName
        foreach ($directory in @('bin', 'lib', 'libexec', 'share', 'etc')) {
            $source = Join-Path $componentDirectory $directory
            if (Test-Path -LiteralPath $source) {
                [IO.Directory]::CreateDirectory($toolchain) | Out-Null
                Copy-Item -LiteralPath $source -Destination $toolchain -Recurse -Force
            }
        }
    }
    $packages.Add([pscustomobject]@{ component = $component; url = $url; sha256 = $archiveHash })
}

$receipt = [ordered]@{
    version = $version
    target = $target
    verified_utc = [DateTime]::UtcNow.ToString('o')
    manifest_url = $manifestUrl
    manifest_sha256 = $manifestHash
    packages = @($packages)
    execution = 'Nenhum binário Rust foi executado; a política do Windows não foi alterada.'
}
$receipt | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $toolchain 'verified-downloads.json') -Encoding utf8
Write-Output "Componentes oficiais verificados em $toolchain."
