[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Executable,
    [ValidateRange(0, 16)]
    [int]$CameraIndex = 0,
    [Parameter(Mandatory)]
    [ValidatePattern('^[a-z0-9][a-z0-9_-]{0,63}$')]
    [string]$Scenario
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$binary = Get-Item -LiteralPath $Executable
$binaryHash = (Get-FileHash -LiteralPath $binary.FullName -Algorithm SHA256).Hash
$start = [Diagnostics.ProcessStartInfo]::new($binary.FullName)
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
$start.RedirectStandardOutput = $true
$start.RedirectStandardError = $true
$start.ArgumentList.Add('--sample-ambient')
$start.ArgumentList.Add([string]$CameraIndex)
$process = [Diagnostics.Process]::new()
$process.StartInfo = $start
$clock = [Diagnostics.Stopwatch]::new()
$cpuSeconds = 0.0
$peakWorkingSet = 0L
$timedOut = $false
$warnings = [Collections.Generic.List[string]]::new()

try {
    $clock.Start()
    if (-not $process.Start()) { throw 'Não foi possível iniciar a leitura da câmera.' }
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    while (-not $process.HasExited) {
        if ($clock.ElapsedMilliseconds -ge 5000) {
            $timedOut = $true
            $process.Kill($true)
            if (-not $process.WaitForExit(1000)) { throw 'A leitura não encerrou após o prazo.' }
            break
        }
        try {
            $process.Refresh()
            $cpuSeconds = $process.TotalProcessorTime.TotalSeconds
            $peakWorkingSet = [math]::Max($peakWorkingSet, $process.PeakWorkingSet64)
        } catch {
            $warnings.Add('Uma amostra coincidiu com o encerramento do processo.')
        }
        $process.WaitForExit([int][math]::Max(0, [math]::Min(100, 5000 - $clock.ElapsedMilliseconds))) | Out-Null
    }
    $clock.Stop()
    $rawOutput = $stdout.GetAwaiter().GetResult()
    $rawError = $stderr.GetAwaiter().GetResult()
    $validReading = $false
    $luminance = $null
    if ($process.ExitCode -eq 0 -and -not $timedOut) {
        try {
            $reading = $rawOutput | ConvertFrom-Json
            $value = $reading.PSObject.Properties['luminance']
            if ($null -ne $value) {
                $luminance = [double]$value.Value
                $validReading = [double]::IsFinite($luminance) -and $luminance -ge 0 -and $luminance -le 1
            }
        } catch {
            $warnings.Add('O processo não retornou uma leitura JSON reconhecida.')
        }
    }
    $report = [ordered]@{
        scenario = $Scenario
        binary_version = $binary.VersionInfo.FileVersion
        binary_sha256 = $binaryHash
        measured_utc = [DateTime]::UtcNow.ToString('o')
        duration_seconds = [math]::Round($clock.Elapsed.TotalSeconds, 4)
        cpu_seconds_observed = $cpuSeconds
        peak_working_set_bytes_observed = $peakWorkingSet
        exit_code = $process.ExitCode
        timed_out = $timedOut
        valid_reading = $validReading
        relative_luminance = if ($validReading) { $luminance } else { $null }
        stderr_present = -not [string]::IsNullOrWhiteSpace($rawError)
        warnings = @($warnings)
        limitations = 'Amostragem a cada 100 ms pode perder o último trecho de CPU; frames e identificadores de câmera não são salvos. Não mede lux, nits, watts ou joules.'
    }
    $outputDirectory = Join-Path (Split-Path $PSScriptRoot -Parent) 'artifacts/performance'
    [IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
    $output = Join-Path $outputDirectory "$Scenario-camera.json"
    $report | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $output -Encoding utf8
    $report | ConvertTo-Json -Depth 4
} finally {
    $process.Dispose()
}
