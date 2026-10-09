#requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Executable)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Executable = (Resolve-Path -LiteralPath $Executable).Path
$existing = @(Get-Process -Name estel,estel-portable-x86_64 -ErrorAction SilentlyContinue)
if ($existing.Count -ne 0) { throw 'Feche o Estel normalmente antes de testar os processos.' }

function Wait-Condition {
    param([scriptblock]$Condition, [string]$Message, [int]$Seconds = 20)
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    do {
        if (& $Condition) { return }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    throw $Message
}

function Find-Host {
    param([int]$LauncherId)
    ,@(Get-Process -Name estel,estel-portable-x86_64 -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $Executable -and $null -ne $_.Parent -and $_.Parent.Id -eq $LauncherId })
}

function Find-Panel {
    ,@(Get-Process -Name estel,estel-portable-x86_64 -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $Executable -and $_.MainWindowTitle -eq 'Estel' })
}

function Wait-Ready {
    param([int]$LauncherId, [int]$Attempt)
    Wait-Condition {
        if (-not (Test-Path -LiteralPath $launcherLog)) { return $false }
        $records = Get-Content -LiteralPath $launcherLog -Raw
        $hosts = Find-Host $LauncherId
        $hosts.Count -eq 1 -and
            $records -match "processo principal iniciado pid=$($hosts[0].Id) attempt=$Attempt(?:\s|$)" -and
            $records -match "inicialização confirmada pid=$($hosts[0].Id)(?:\s|$)"
    } "A tentativa $Attempt não confirmou a inicialização."
}

# Close only the terminal failure dialog created by this test's launcher.
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class EstelTestWindows {
    public delegate bool EnumProc(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr parameter);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    public static bool HasVisibleWindow(int process) {
        bool found = false;
        EnumWindows((window, parameter) => {
            uint owner; GetWindowThreadProcessId(window, out owner);
            if (owner == process && IsWindowVisible(window)) found = true;
            return true;
        }, IntPtr.Zero);
        return found;
    }
    public static void CloseDialog(int process) {
        EnumWindows((window, parameter) => {
            uint owner; GetWindowThreadProcessId(window, out owner);
            if (owner == process) PostMessage(window, 0x0010, IntPtr.Zero, IntPtr.Zero);
            return true;
        }, IntPtr.Zero);
    }
}
'@

function Read-RegistryValue {
    param([string]$SubKey)
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($SubKey)
    if ($null -eq $key) { return $null }
    try { ,$key.GetValue('Estel', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) }
    finally { $key.Dispose() }
}

function Restore-RegistryValue {
    param([string]$SubKey, [object]$Value)
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($SubKey, $true)
    if ($null -eq $key) {
        if ($null -eq $Value) { return }
        $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($SubKey)
    }
    try {
        if ($null -eq $Value) { $key.DeleteValue('Estel', $false) }
        elseif ($Value -is [byte[]]) { $key.SetValue('Estel', $Value, [Microsoft.Win32.RegistryValueKind]::Binary) }
        else { $key.SetValue('Estel', $Value, [Microsoft.Win32.RegistryValueKind]::String) }
    } finally { $key.Dispose() }
}

$runKey = 'Software\Microsoft\Windows\CurrentVersion\Run'
$approvedKey = 'Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
$originalRun = Read-RegistryValue $runKey
$originalApproval = Read-RegistryValue $approvedKey
$originalAppData = $env:APPDATA
$originalRustLog = $env:RUST_LOG
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('estel-runtime-' + [guid]::NewGuid().ToString('N'))
$configDir = Join-Path $testRoot 'condado\estel\config'
$launcherLog = Join-Path $configDir 'launcher.log'
$launchers = [Collections.Generic.List[Diagnostics.Process]]::new()
New-Item -ItemType Directory -Path $configDir -Force | Out-Null
Set-Content -LiteralPath (Join-Path $configDir 'config.toml') -Encoding utf8 -Value @'
setup_completed = true
color_critical_work = true
start_with_windows = false
weather_enabled = false
noise_enabled = false
ambient_enabled = false
ambient_camera_id = 'estel-runtime-unavailable-camera'
'@
$configHash = (Get-FileHash -LiteralPath (Join-Path $configDir 'config.toml')).Hash
try {
    $env:APPDATA = $testRoot
    $env:RUST_LOG = 'warn,estel=debug'
    $launcher = Start-Process -FilePath $Executable -ArgumentList '--startup' -WindowStyle Hidden -PassThru
    $launchers.Add($launcher)
    Wait-Ready $launcher.Id 0
    $duplicate = Start-Process -FilePath $Executable -ArgumentList '--startup' -WindowStyle Hidden -PassThru
    if (-not $duplicate.WaitForExit(10000) -or $duplicate.ExitCode -ne 0) { throw 'A abertura duplicada não terminou normalmente.' }
    if ((Find-Host $launcher.Id).Count -ne 1) { throw 'A abertura duplicada criou outro processo principal.' }
    $openPanel = Start-Process -FilePath $Executable -ArgumentList '--settings' -WindowStyle Hidden -PassThru
    if (-not $openPanel.WaitForExit(10000) -or $openPanel.ExitCode -ne 0) { throw 'A abertura do painel falhou.' }
    $settingsLog = Join-Path $configDir 'settings.log'
    Wait-Condition {
        (Test-Path -LiteralPath $settingsLog) -and
            (Get-Content -LiteralPath $settingsLog -Raw) -match 'painel acompanhando as atualizações do Estel'
    } 'O painel não iniciou a escuta de atualizações.'
    Wait-Condition { (Find-Panel).Count -eq 1 } 'O painel não exibiu uma única janela.'
    $panels = Find-Panel
    $panelId = $panels[0].Id
    Write-Output 'PASS: inicialização, abertura duplicada e painel único visível.'
    $hostProcess = (Find-Host $launcher.Id)[0]
    Stop-Process -Id $hostProcess.Id
    Wait-Ready $launcher.Id 1
    $settingsBefore = ([regex]::Matches((Get-Content -LiteralPath $settingsLog -Raw), 'painel recebeu atualização de estado')).Count
    $event = [Threading.EventWaitHandle]::OpenExisting('Local\EstelStatusChanged')
    try { [void]$event.Set() } finally { $event.Dispose() }
    Wait-Condition {
        ([regex]::Matches((Get-Content -LiteralPath $settingsLog -Raw), 'painel recebeu atualização de estado')).Count -gt $settingsBefore
    } 'O painel deixou de receber eventos após a recuperação.'
    $reopenPanel = Start-Process -FilePath $Executable -ArgumentList '--settings' -WindowStyle Hidden -PassThru
    if (-not $reopenPanel.WaitForExit(10000) -or $reopenPanel.ExitCode -ne 0) { throw 'A reabertura do painel falhou.' }
    Wait-Condition {
        $panels = Find-Panel
        $panels.Count -eq 1 -and $panels[0].Id -eq $panelId
    } 'A recuperação duplicou ou substituiu o painel existente.'
    $quit = Start-Process -FilePath $Executable -ArgumentList '--quit' -WindowStyle Hidden -PassThru
    if (-not $quit.WaitForExit(10000) -or $quit.ExitCode -ne 0) { throw 'O comando de encerramento falhou.' }
    if (-not $launcher.WaitForExit(35000) -or $launcher.ExitCode -ne 0) { throw 'O encerramento reiniciou ou não fechou o Estel.' }
    if ((Find-Host $launcher.Id).Count -ne 0) { throw 'O processo principal ficou aberto após encerrar.' }
    if ((Get-FileHash -LiteralPath (Join-Path $configDir 'config.toml')).Hash -ne $configHash) { throw 'A recuperação alterou as preferências.' }
    Write-Output 'PASS: recuperação, painel atualizado, encerramento e preferências preservadas.'

    $launcher = Start-Process -FilePath $Executable -ArgumentList '--startup' -WindowStyle Hidden -PassThru
    $launchers.Add($launcher)
    Wait-Ready $launcher.Id 0
    $settingsBefore = ([regex]::Matches((Get-Content -LiteralPath $settingsLog -Raw), 'painel recebeu atualização de estado')).Count
    $event = [Threading.EventWaitHandle]::OpenExisting('Local\EstelStatusChanged')
    try { [void]$event.Set() } finally { $event.Dispose() }
    Wait-Condition {
        ([regex]::Matches((Get-Content -LiteralPath $settingsLog -Raw), 'painel recebeu atualização de estado')).Count -gt $settingsBefore
    } 'O painel deixou de receber eventos após fechar e reabrir o Estel.'
    [EstelTestWindows]::CloseDialog($panelId)
    Wait-Condition { $null -eq (Get-Process -Id $panelId -ErrorAction SilentlyContinue) } 'O painel não fechou normalmente.'
    $quit = Start-Process -FilePath $Executable -ArgumentList '--quit' -WindowStyle Hidden -PassThru
    if (-not $quit.WaitForExit(10000) -or -not $launcher.WaitForExit(35000)) { throw 'A nova sessão não encerrou.' }
    Write-Output 'PASS: painel atualizado entre sessões e fechamento normal.'

    Remove-Item -LiteralPath $launcherLog
    $launcher = Start-Process -FilePath $Executable -ArgumentList '--startup' -WindowStyle Hidden -PassThru
    $launchers.Add($launcher)
    for ($attempt = 0; $attempt -le 3; $attempt++) {
        Wait-Ready $launcher.Id $attempt
        Stop-Process -Id (Find-Host $launcher.Id)[0].Id
    }
    Wait-Condition {
        [EstelTestWindows]::CloseDialog($launcher.Id)
        $launcher.HasExited
    } 'O limite de recuperação não encerrou o iniciador.'
    if ($launcher.ExitCode -eq 0) { throw 'O limite de recuperação foi comunicado como sucesso.' }
    $records = Get-Content -LiteralPath $launcherLog -Raw
    if (([regex]::Matches($records, 'processo principal iniciado')).Count -ne 4) { throw 'O limite de três reinícios não foi respeitado.' }
    Write-Output 'PASS: limite de três reinícios e falha comunicada com código de saída.'

    $launcher = Start-Process -FilePath $Executable -ArgumentList '--startup' -WindowStyle Hidden -PassThru
    $launchers.Add($launcher)
    Wait-Ready $launcher.Id 0
    Stop-Process -Id (Find-Host $launcher.Id)[0].Id
    $manual = Start-Process -FilePath $Executable -ArgumentList '--settings' -WindowStyle Hidden -PassThru
    if (-not $manual.WaitForExit(10000) -or $manual.ExitCode -ne 0) { throw 'A abertura manual durante recuperação falhou.' }
    Wait-Condition { [EstelTestWindows]::HasVisibleWindow($launcher.Id) } 'A recuperação iniciada pelo Windows não mostrou progresso na abertura manual.'
    [EstelTestWindows]::CloseDialog($launcher.Id)
    if (-not $launcher.WaitForExit(10000) -or $launcher.ExitCode -ne 0) { throw 'Cancelar a recuperação não encerrou normalmente.' }
    if ((Find-Host $launcher.Id).Count -ne 0) { throw 'Cancelar a recuperação deixou um processo principal aberto.' }
    Write-Output 'PASS: inicialização, abertura duplicada, recuperação de crash, painel único atualizado entre sessões, encerramento, limite de reinícios, progresso e cancelamento, preferências preservadas.'
    Write-Output "Diagnósticos: $configDir"
} finally {
    try {
        $ownedProcesses = @(Get-Process -Name estel,estel-portable-x86_64 -ErrorAction SilentlyContinue |
            Where-Object { $_.Path -eq $Executable })
        foreach ($process in $ownedProcesses) {
            [EstelTestWindows]::CloseDialog($process.Id)
            if (-not $process.HasExited -and -not $process.WaitForExit(5000)) { Stop-Process -Id $process.Id }
        }
    } finally {
        $env:APPDATA = $originalAppData
        $env:RUST_LOG = $originalRustLog
        Restore-RegistryValue $runKey $originalRun
        Restore-RegistryValue $approvedKey $originalApproval
    }
}
