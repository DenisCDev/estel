[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateRange(1, 2147483647)]
    [int]$TargetProcessId,
    [Parameter(Mandatory)]
    [ValidatePattern('^[a-z0-9][a-z0-9_-]{0,63}$')]
    [string]$Scenario,
    [ValidateRange(3, 60)]
    [int]$DurationSeconds = 30,
    [ValidateRange(1, 10)]
    [int]$IntervalSeconds = 2,
    [switch]$IncludeGpu,
    [switch]$IncludeDwm
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not $IsWindows) { throw 'A medição requer Windows e PowerShell 7.' }
$target = Get-Process -Id $TargetProcessId
$targetStart = $target.StartTime.ToUniversalTime()
$targetSession = $target.SessionId
$logicalProcessors = [Environment]::ProcessorCount
$outputDirectory = Join-Path (Split-Path $PSScriptRoot -Parent) 'artifacts/performance'
[IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
$runName = '{0}_{1}_{2}' -f $Scenario, [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ'), $TargetProcessId
$outputBase = Join-Path $outputDirectory $runName
$rows = [Collections.Generic.List[object]]::new()
$warnings = [Collections.Generic.HashSet[string]]::new()
$previous = @{}
$clock = [Diagnostics.Stopwatch]::StartNew()
$observer = Get-Process -Id $PID
$observerStartCpu = $observer.TotalProcessorTime.TotalSeconds
$status = 'concluído'
$gpuAvailable = $null
$gpuFailureCount = 0
$sampleIndex = 0

while ($clock.Elapsed.TotalSeconds -lt $DurationSeconds) {
    $sampleStart = $clock.Elapsed.TotalSeconds
    $target = Get-Process -Id $TargetProcessId -ErrorAction SilentlyContinue
    if ($null -eq $target -or $target.StartTime.ToUniversalTime() -ne $targetStart) {
        $status = 'processo_encerrado'
        break
    }
    if ($DurationSeconds - $clock.Elapsed.TotalSeconds -lt 2) { break }

    # WMI supplies ancestry only; command lines and executable paths are not recorded.
    try {
        $processes = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId -OperationTimeoutSec 2)
    } catch {
        $warnings.Add('A descoberta de filhos falhou; a amostra inclui somente o processo principal.') | Out-Null
        $processes = @()
    }
    $members = [Collections.Generic.HashSet[int]]::new()
    $members.Add($TargetProcessId) | Out-Null
    do {
        $added = $false
        foreach ($process in $processes) {
            if ($members.Contains([int]$process.ParentProcessId) -and $members.Add([int]$process.ProcessId)) {
                $added = $true
            }
        }
    } while ($added)

    $compositorIds = @()
    if ($IncludeDwm) {
        $compositorIds = @(Get-Process dwm -ErrorAction SilentlyContinue |
            Where-Object SessionId -eq $targetSession | ForEach-Object Id)
        foreach ($compositorId in $compositorIds) { $members.Add($compositorId) | Out-Null }
    }
    $gpu = @{}
    $gpuQuerySucceeded = $false
    if ($IncludeGpu -and $gpuFailureCount -lt 3 -and $DurationSeconds - $clock.Elapsed.TotalSeconds -ge 2) {
        try {
            $engines = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine -OperationTimeoutSec 2)
            foreach ($engine in $engines) {
                if ($engine.Name -match '^pid_(\d+)_.*_engtype_(.+)$') {
                    $engineProcessId = [int]$Matches[1]
                    if ($members.Contains($engineProcessId)) {
                        if (-not $gpu.ContainsKey($engineProcessId)) { $gpu[$engineProcessId] = @{} }
                        $kind = $Matches[2]
                        if (-not $gpu[$engineProcessId].ContainsKey($kind)) { $gpu[$engineProcessId][$kind] = 0.0 }
                        $gpu[$engineProcessId][$kind] += [double]$engine.UtilizationPercentage
                    }
                }
            }
            $gpuAvailable = $true
            $gpuQuerySucceeded = $true
            $gpuFailureCount = 0
        } catch {
            if ($null -eq $gpuAvailable) { $gpuAvailable = $false }
            $gpuFailureCount++
            $warnings.Add('Uma consulta de GPU falhou ou excedeu o prazo; a amostra não contém esses valores. Três falhas consecutivas suspendem consultas de GPU.') | Out-Null
        }
    }

    foreach ($memberId in $members) {
        if ($compositorIds -contains $memberId) {
            if ($DurationSeconds - $clock.Elapsed.TotalSeconds -lt 2) { continue }
            try {
                # DWM denies direct process-time access without elevation; public performance counters remain usable.
                $dwm = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter "IDProcess = $memberId" -OperationTimeoutSec 2
                if ($null -ne $dwm) {
                    $rows.Add([pscustomobject][ordered]@{
                        sample_index = $sampleIndex
                        elapsed_seconds = [math]::Round($clock.Elapsed.TotalSeconds, 3)
                        process_id = $memberId
                        is_root = $false
                        role = 'dwm'
                        cpu_percent_machine = [math]::Round([double]$dwm.PercentProcessorTime / $logicalProcessors, 5)
                        cpu_seconds_lifetime = $null
                        working_set_bytes = [long]$dwm.WorkingSet
                        private_bytes = [long]$dwm.PrivateBytes
                        handles = [long]$dwm.HandleCount
                        threads = [long]$dwm.ThreadCount
                        gpu_engines = if ($gpu.ContainsKey($memberId)) { $gpu[$memberId] } else { $null }
                        gpu_query_succeeded = $gpuQuerySucceeded
                    })
                }
            } catch {
                $warnings.Add('A consulta de contadores públicos do DWM falhou ou excedeu o prazo.') | Out-Null
            }
            continue
        }
        $process = Get-Process -Id $memberId -ErrorAction SilentlyContinue
        if ($null -eq $process) { continue }
        try {
            $process.Refresh()
            $sampleTime = $clock.Elapsed.TotalSeconds
            $started = $process.StartTime.ToUniversalTime().Ticks
            $cpuSeconds = $process.TotalProcessorTime.TotalSeconds
            $cpuPercent = $null
            if ($previous.ContainsKey($memberId) -and $previous[$memberId].Started -eq $started) {
                $elapsed = $sampleTime - $previous[$memberId].Time
                $cpuPercent = 100.0 * ($cpuSeconds - $previous[$memberId].Cpu) / ($elapsed * $logicalProcessors)
            }
            $previous[$memberId] = @{ Started = $started; Time = $sampleTime; Cpu = $cpuSeconds }
            $engineValues = if ($gpu.ContainsKey($memberId)) { $gpu[$memberId] } else { $null }
            $rows.Add([pscustomobject][ordered]@{
                sample_index = $sampleIndex
                elapsed_seconds = [math]::Round($sampleTime, 3)
                process_id = $memberId
                is_root = $memberId -eq $TargetProcessId
                role = if ($compositorIds -contains $memberId) { 'dwm' } elseif ($memberId -eq $TargetProcessId) { 'principal' } else { 'filho' }
                cpu_percent_machine = if ($null -eq $cpuPercent) { $null } else { [math]::Round($cpuPercent, 5) }
                cpu_seconds_lifetime = $cpuSeconds
                working_set_bytes = $process.WorkingSet64
                private_bytes = $process.PrivateMemorySize64
                handles = $process.HandleCount
                threads = $process.Threads.Count
                gpu_engines = $engineValues
                gpu_query_succeeded = $gpuQuerySucceeded
            })
        } catch {
            $warnings.Add('Um processo encerrou ou deixou de permitir leitura durante uma amostra.') | Out-Null
        }
    }
    $sampleIndex++
    $remaining = $DurationSeconds - $clock.Elapsed.TotalSeconds
    $untilNext = $IntervalSeconds - ($clock.Elapsed.TotalSeconds - $sampleStart)
    if ($remaining -gt 0 -and $untilNext -gt 0) {
        Start-Sleep -Milliseconds ([int](1000 * [math]::Min($remaining, $untilNext)))
    }
}

$clock.Stop()
$observer.Refresh()
if ($rows.Count -eq 0) { $status = 'sem_amostras' }
$metadata = [ordered]@{
    schema_version = 1
    scenario = $Scenario
    status = $status
    started_utc = [DateTime]::UtcNow.AddSeconds(-$clock.Elapsed.TotalSeconds).ToString('o')
    duration_seconds = [math]::Round($clock.Elapsed.TotalSeconds, 3)
    requested_duration_seconds = $DurationSeconds
    interval_seconds = $IntervalSeconds
    logical_processors = $logicalProcessors
    gpu_requested = [bool]$IncludeGpu
    gpu_available = $gpuAvailable
    dwm_requested = [bool]$IncludeDwm
    observer_cpu_seconds = [math]::Round($observer.TotalProcessorTime.TotalSeconds - $observerStartCpu, 5)
    warnings = @($warnings)
    limits = @(
        'CPU, memória, handles, threads e contadores de GPU são recursos; watts e joules não foram medidos.',
        'CPU é normalizada pela capacidade total dos processadores lógicos; a primeira amostra não tem delta.',
        'GPU agrupa motores do processo e pode exceder 100%; ausência de motor não é medição de zero.',
        'DWM, quando solicitado, é medido separadamente: seu consumo pertence ao desktop inteiro e não pode ser atribuído ao Estel.',
        'CPU do DWM usa o intervalo e a precisão dos contadores formatados do Windows; CPU do aplicativo usa diferenças entre amostras.',
        'Filhos que iniciam e encerram entre amostras podem não aparecer; memória compartilhada pode ser contada duas vezes.',
        'A consulta de métricas também consome recursos; seu tempo de CPU está registrado separadamente.',
        'Consultas WMI possuem prazo de 2 s, mas provedores do Windows podem excedê-lo durante cancelamento.'
    )
    samples = @($rows)
}
$metadata | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath "$outputBase.json" -Encoding utf8
$rows | Select-Object *, @{ Name = 'gpu_engines_json'; Expression = {
    if ($null -eq $_.gpu_engines) { '' } else { $_.gpu_engines | ConvertTo-Json -Compress }
} } -ExcludeProperty gpu_engines | Export-Csv -LiteralPath "$outputBase.csv" -NoTypeInformation -Encoding utf8
[pscustomobject]@{ estado = $status; amostras = $rows.Count; json = "$outputBase.json"; csv = "$outputBase.csv" }
