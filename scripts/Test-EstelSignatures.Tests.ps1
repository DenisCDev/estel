[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$SignToolPath,
    [Parameter(Mandatory)]
    [string]$UnsignedExecutable
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$signTool = Get-Item -LiteralPath $SignToolPath
$unsigned = Get-Item -LiteralPath $UnsignedExecutable
$signature = Get-AuthenticodeSignature -LiteralPath $signTool.FullName
if ($signature.Status -ne 'Valid' -or $null -eq $signature.TimeStamperCertificate) {
    throw 'O SignTool usado como amostra positiva precisa de assinatura válida com carimbo de tempo.'
}
if ((Get-AuthenticodeSignature -LiteralPath $unsigned.FullName).Status -ne 'NotSigned') {
    throw 'A amostra negativa precisa ser um executável sem assinatura.'
}
$outputDirectory = Join-Path (Split-Path $PSScriptRoot -Parent) 'artifacts/performance'
[IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
$manifest = Join-Path $outputDirectory 'signature-tests.sha256'
$verifier = Join-Path $PSScriptRoot 'Test-EstelSignatures.ps1'
$common = @{ SignToolPath = $signTool.FullName; ExpectedSignerThumbprint = $signature.SignerCertificate.Thumbprint }
$passed = [Collections.Generic.List[string]]::new()

function Assert-Rejected {
    param([string]$Name, [scriptblock]$Action, [string]$ExpectedError)
    $failure = $null
    try { & $Action | Out-Null } catch { $failure = $_.Exception.Message }
    if (-not $failure -or $failure -notlike $ExpectedError) {
        throw "O teste $Name não recebeu a rejeição esperada: $failure"
    }
    $passed.Add($Name)
}

$verified = @(& $verifier @common -Path $signTool.FullName -ChecksumPath $manifest)
$expectedHash = (Get-FileHash -LiteralPath $signTool.FullName -Algorithm SHA256).Hash
if ($verified.Count -ne 1 -or $verified[0].sha256 -ne $expectedHash -or
    (Get-Content -LiteralPath $manifest -Raw).Trim() -ne ('{0}  {1}' -f $expectedHash.ToLowerInvariant(), $signTool.Name)) {
    throw 'A validação positiva não produziu o SHA-256 esperado.'
}
$passed.Add('assinatura_valida_e_sha256')
$sentinel = Get-Content -LiteralPath $manifest -Raw

Assert-Rejected 'arquivo_sem_assinatura' {
    & $verifier @common -Path $unsigned.FullName -ChecksumPath $manifest
} '*falhou ou gerou aviso*'
Assert-Rejected 'lote_parcial_nao_publicavel' {
    & $verifier @common -Path $signTool.FullName, $unsigned.FullName -ChecksumPath $manifest
} '*falhou ou gerou aviso*'
if ((Get-Content -LiteralPath $manifest -Raw) -cne $sentinel) {
    throw 'Uma rejeição alterou o manifesto anterior.'
}
$passed.Add('manifesto_preservado_em_falha')
Assert-Rejected 'editor_diferente' {
    & $verifier -SignToolPath $signTool.FullName -ExpectedSignerThumbprint ('0' * 40) -Path $signTool.FullName
} '*não corresponde ao certificado esperado*'
Assert-Rejected 'checksum_nao_sobrescreve_executavel' {
    & $verifier @common -Path $signTool.FullName -ChecksumPath $signTool.FullName
} '*não pode substituir*'
Assert-Rejected 'arquivo_ausente' {
    & $verifier @common -Path (Join-Path $outputDirectory ([guid]::NewGuid().ToString('N') + '.exe'))
} '*'
if ((Get-FileHash -LiteralPath $signTool.FullName -Algorithm SHA256).Hash -ne $expectedHash) {
    throw 'A amostra positiva foi alterada durante os testes.'
}
$report = [ordered]@{
    measured_utc = [DateTime]::UtcNow.ToString('o')
    passed = @($passed)
    signed_fixture = $signTool.Name
    unsigned_fixture = $unsigned.Name
    estel_signed = $false
}
$report | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $outputDirectory 'signature-tests.json') -Encoding utf8
$report | ConvertTo-Json -Depth 3
