[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string[]]$Path,
    [Parameter(Mandatory)]
    [string]$SignToolPath,
    [Parameter(Mandatory)]
    [ValidatePattern('^[a-fA-F0-9]{40}$')]
    [string]$ExpectedSignerThumbprint,
    [string]$ChecksumPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw 'A verificação requer Windows e PowerShell 7.' }
$signTool = Get-Item -LiteralPath $SignToolPath
if ($signTool.PSIsContainer) { throw 'Informe o executável signtool.exe do Windows SDK.' }
$files = @($Path | ForEach-Object { Get-Item -LiteralPath $_ })
if ($files.Where({ $_.PSIsContainer }).Count) { throw 'Informe somente arquivos para verificar.' }
$checksumFile = if ($ChecksumPath) { [IO.Path]::GetFullPath($ChecksumPath) } else { $null }
if ($checksumFile -and $files.FullName -contains $checksumFile) {
    throw 'O arquivo de checksums não pode substituir um arquivo verificado.'
}
if ($checksumFile -and @($files.Name | Sort-Object -Unique).Count -ne $files.Count) {
    throw 'O arquivo de checksums exige nomes de arquivo distintos.'
}
$results = [Collections.Generic.List[object]]::new()

foreach ($file in $files) {
    # Keep the verified bytes unchanged until their checksum has been computed.
    $stream = [IO.File]::Open($file.FullName, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        $start = [Diagnostics.ProcessStartInfo]::new($signTool.FullName)
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        foreach ($argument in @('verify', '/pa', '/all', '/tw', $file.FullName)) {
            $start.ArgumentList.Add($argument)
        }
        $process = [Diagnostics.Process]::new()
        $process.StartInfo = $start
        try {
            if (-not $process.Start()) { throw 'Não foi possível iniciar o SignTool.' }
            $stdout = $process.StandardOutput.ReadToEndAsync()
            $stderr = $process.StandardError.ReadToEndAsync()
            if (-not $process.WaitForExit(30000)) {
                $process.Kill($true)
                if (-not $process.WaitForExit(1000)) { throw 'O SignTool não encerrou após o prazo.' }
                throw "A verificação de $($file.Name) excedeu 30 segundos."
            }
            if ($process.ExitCode -ne 0) {
                throw "A assinatura de $($file.Name) falhou ou gerou aviso: $($stderr.GetAwaiter().GetResult()) $($stdout.GetAwaiter().GetResult())"
            }
        } finally {
            $process.Dispose()
        }
        $signature = Get-AuthenticodeSignature -LiteralPath $file.FullName
        if ($signature.Status -ne 'Valid' -or $signature.SignatureType -ne 'Authenticode') {
            throw "$($file.Name) precisa de assinatura Authenticode incorporada e válida."
        }
        $certificate = $signature.SignerCertificate
        if ($certificate.Thumbprint -ne $ExpectedSignerThumbprint) {
            throw "O editor de $($file.Name) não corresponde ao certificado esperado."
        }
        if ($certificate.Subject -eq $certificate.Issuer) { throw 'Certificados autoassinados não são aceitos.' }
        if ($null -eq $signature.TimeStamperCertificate) { throw "$($file.Name) não possui carimbo de tempo." }
        if ($certificate.PublicKey.Oid.Value -notin @('1.2.840.113549.1.1.1', '1.2.840.10045.2.1')) {
            throw 'O certificado precisa utilizar RSA ou ECC, conforme os requisitos do Smart App Control.'
        }
        $hash = Get-FileHash -InputStream $stream -Algorithm SHA256
        $results.Add([pscustomobject]@{
            file = $file.Name
            publisher = $certificate.Subject
            signer_thumbprint = $certificate.Thumbprint
            timestamp_present = $true
            sha256 = $hash.Hash
        })
    } finally {
        $stream.Dispose()
    }
}

# A failing input must never produce a new manifest that looks publishable.
if ($checksumFile) {
    $results | ForEach-Object { '{0}  {1}' -f $_.sha256.ToLowerInvariant(), $_.file } |
        Set-Content -LiteralPath $checksumFile -Encoding utf8
}
$results
