# One install cell of installers.yml on Windows: installs a component
# selection with the NSIS setup, checks that exactly the selected components
# are present and that each present one runs, then runs the selection's
# Windows-only check (update, relaunch, rerun, or rejections).
#
# Usage: cell.ps1 <setup> <selection> <version>
#   <setup>      the NSIS -setup.exe
#   <selection>  all, workshop, gateway, gateway-stt, or rejections
#   <version>    the version Workshop must print; the Gateway must print the
#                version gateway-pin/pin.json names
#
# Environment:
#   CELL_LOGS    write the logs here (default: a new temporary directory)
param(
  [Parameter(Mandatory = $true, Position = 0)][string]$Setup,
  [Parameter(Mandatory = $true, Position = 1)][string]$Selection,
  [Parameter(Mandatory = $true, Position = 2)][string]$Version
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Fail([string]$Message) {
  throw "cell: $Message"
}

if (-not (Test-Path -LiteralPath $Setup -PathType Leaf)) { Fail "setup $Setup does not exist" }
$Setup = (Resolve-Path -LiteralPath $Setup).Path
$selections = @{
  'all'         = @{ List = 'workshop,gateway,stt'; Workshop = $true; Gateway = $true; Stt = $true }
  'workshop'    = @{ List = 'workshop'; Workshop = $true; Gateway = $false; Stt = $false }
  'gateway'     = @{ List = 'gateway'; Workshop = $false; Gateway = $true; Stt = $false }
  'gateway-stt' = @{ List = 'gateway,stt'; Workshop = $false; Gateway = $true; Stt = $true }
  'rejections'  = $null
}
if (-not $selections.ContainsKey($Selection)) {
  Fail "selection must be all, workshop, gateway, gateway-stt, or rejections; got '$Selection'"
}
$want = $selections[$Selection]
# The setup carries the Gateway release the repository pins.
$pin = Join-Path $PSScriptRoot '..\..\gateway-pin\pin.json'
$gatewayVersion = (Get-Content -LiteralPath $pin -Raw | ConvertFrom-Json).version
if (-not $gatewayVersion) { Fail "$pin names no Gateway version" }

$root = Join-Path $env:LOCALAPPDATA 'Runvil'
$profileDir = Join-Path $env:USERPROFILE '.runvil'
$workshopExe = Join-Path $root 'runvil-workshop.exe'
# The std Plugin's library installs with Workshop, in its Plugin directory.
$stdPlugin = Join-Path $root 'plugins\plugin_std.dll'
$gatewayExe = Join-Path $root 'runvil-gateway.exe'
$config = Join-Path $profileDir 'gateway.toml'
$discovery = Join-Path $profileDir 'run\gateway.json'
# Nothing listens on the discard port, so every download through this proxy
# fails at once: an update install must need none.
$deadProxy = 'http://127.0.0.1:9'
$logs = $env:CELL_LOGS
if (-not $logs) { $logs = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName()) }
New-Item -ItemType Directory -Force -Path $logs | Out-Null
Write-Host "cell: $Selection on Windows, logs in $logs"

function Invoke-Setup([string[]]$Arguments, [switch]$DeadProxy) {
  $savedHttps = $env:HTTPS_PROXY
  $savedHttp = $env:HTTP_PROXY
  if ($DeadProxy) {
    $env:HTTPS_PROXY = $deadProxy
    $env:HTTP_PROXY = $deadProxy
  }
  try {
    Write-Host "cell: running the setup with $($Arguments -join ' ')$(if ($DeadProxy) { ' behind the dead proxy' })"
    # Not -Wait, which waits for every descendant too, so a process the setup
    # leaves running would hang the cell without a message. Reading Handle
    # at once caches the process handle while the setup runs, so ExitCode
    # stays readable after the exit; a precaution here, since the reported
    # cases use -NoNewWindow or -RedirectStandard*:
    # https://github.com/PowerShell/PowerShell/issues/5421
    $process = Start-Process -FilePath $Setup -ArgumentList $Arguments -PassThru
    $null = $process.Handle
    if (-not $process.WaitForExit(15 * 60 * 1000)) { Fail "the setup with $($Arguments -join ' ') did not exit within 15 minutes" }
    return $process.ExitCode
  } finally {
    $env:HTTPS_PROXY = $savedHttps
    $env:HTTP_PROXY = $savedHttp
  }
}

function Stop-Started {
  $running = @(Get-Process -Name 'runvil-workshop', 'runvil-gateway' -ErrorAction SilentlyContinue)
  foreach ($process in $running) {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
  }
  foreach ($process in $running) {
    Wait-Process -Id $process.Id -Timeout 30 -ErrorAction SilentlyContinue
  }
}

function Test-Presence([string]$Name, [bool]$Wanted, [string]$Path) {
  $exists = Test-Path -LiteralPath $Path
  if ($Wanted -and -not $exists) { Fail "$Name was selected, but $Path is missing" }
  if (-not $Wanted -and $exists) { Fail "$Name was not selected, but $Path exists" }
}

function Get-ModelFiles {
  @(Get-ChildItem -LiteralPath (Join-Path $profileDir 'models') -Recurse -File -ErrorAction SilentlyContinue)
}

# The Speech to Text gate the setup writes for the Gateway's first start:
# $null when absent (speech on), else the DWORD (0 = speech off).
function Get-InstallStt {
  $keys = @(Get-Item -Path 'HKCU:\Software\*\Runvil' -ErrorAction SilentlyContinue)
  if ($keys.Count -ne 1) { Fail "exactly one HKCU:\Software\*\Runvil key must exist after an install; found $($keys.Count)" }
  return $keys[0].GetValue('InstallSTT')
}

function Get-ProcessPath([int]$Id) {
  $process = Get-Process -Id $Id -ErrorAction SilentlyContinue
  if ($process) { return $process.Path }
  return $null
}

# Waits until the discovery file names a live Gateway running the installed
# executable, other than $NotPid, and returns its pid and port.
function Wait-InstalledGateway([int]$NotPid = 0, [int]$Seconds = 120) {
  $found = 'no discovery file'
  foreach ($attempt in 1..$Seconds) {
    if (Test-Path -LiteralPath $discovery) {
      try {
        $record = Get-Content -LiteralPath $discovery -Raw | ConvertFrom-Json
        $path = Get-ProcessPath $record.pid
        if ($path -and $record.pid -ne $NotPid -and
          [IO.Path]::GetFullPath($path) -ieq [IO.Path]::GetFullPath($gatewayExe)) {
          return $record
        }
        $found = "pid $($record.pid) running '$path'"
      } catch {
        $found = "an unreadable discovery file ($($_.Exception.Message))"
      }
    }
    Start-Sleep -Seconds 1
  }
  $other = if ($NotPid) { " other than pid $NotPid" } else { '' }
  Fail "$discovery must name a live Gateway$other running $gatewayExe; found $found"
}

function Test-ConfigPage([string]$Origin) {
  $page = Join-Path $logs 'gateway-config.html'
  try {
    Invoke-WebRequest -Uri "$Origin/config/" -OutFile $page -TimeoutSec 10
  } catch {
    Fail "$Origin/config/ did not answer: $($_.Exception.Message)"
  }
  if (-not (Select-String -LiteralPath $page -SimpleMatch '<title>Runvil Gateway Config</title>' -Quiet)) {
    Fail "$Origin/config/ lacks <title>Runvil Gateway Config</title>"
  }
  Write-Host "cell: the Gateway serves /config/ at $Origin"
}

# A shared-read open, since the Gateway still holds the file it writes to.
function Read-SharedText([string]$Path) {
  try {
    $stream = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite')
  } catch {
    return ''
  }
  try {
    return (New-Object IO.StreamReader($stream)).ReadToEnd()
  } finally {
    $stream.Dispose()
  }
}

function Get-InstallState {
  $keys = @(Get-Item -Path 'HKCU:\Software\*\Runvil\Components' -ErrorAction SilentlyContinue)
  if ($keys.Count -ne 1) { Fail "exactly one HKCU:\Software\*\Runvil\Components key must hold the persisted selection; found $($keys.Count)" }
  $components = ($keys[0].GetValueNames() | Sort-Object | ForEach-Object { "$_=$($keys[0].GetValue($_))" }) -join ','
  $models = (@(Get-ModelFiles) | Sort-Object FullName | ForEach-Object { "$($_.FullName):$($_.Length)" }) -join ','
  [pscustomobject]@{
    Config     = [Convert]::ToBase64String([IO.File]::ReadAllBytes($config))
    Models     = $models
    Components = $components
  }
}

function Assert-SameState($Before, $After, [string]$What) {
  if ($After.Config -ne $Before.Config) { Fail "$What changed $config" }
  if ($After.Models -ne $Before.Models) { Fail "$What changed the model files from [$($Before.Models)] to [$($After.Models)]" }
  if ($After.Components -ne $Before.Components) { Fail "$What changed the persisted selection from [$($Before.Components)] to [$($After.Components)]" }
}

function Test-Selection($Want) {
  # 1. Install at the default location.
  $code = Invoke-Setup @('/S', "/COMPONENTS=$($Want.List)")
  if ($code -ne 0) { Fail "the setup exited $code, required 0" }

  # 2. Exactly the selected components are present, the install wrote
  # nothing into the profile, and the Speech to Text gate records the
  # choice for the Gateway's first start.
  Test-Presence 'Workshop' $Want.Workshop $workshopExe
  Test-Presence "Workshop's std Plugin" $Want.Workshop $stdPlugin
  Test-Presence 'Gateway' $Want.Gateway $gatewayExe
  if (Test-Path -LiteralPath $config) { Fail "the install must leave the Gateway config to the Gateway's first start, but $config exists" }
  $downloaded = @(Get-ModelFiles)
  if ($downloaded.Count -ne 0) { Fail "the install must download nothing, but models exist: $($downloaded.FullName -join ', ')" }
  $gate = Get-InstallStt
  if ($Want.Stt -and $null -ne $gate) { Fail "Speech to Text was selected, so InstallSTT must be absent; it is $gate" }
  if (-not $Want.Stt -and $gate -ne 0) { Fail "Speech to Text was not selected, so InstallSTT must be 0; it is '$gate'" }

  # 3. Every present executable prints the version.
  foreach ($check in @(
      @{ Wanted = $Want.Workshop; Exe = $workshopExe; Expected = "runvil-workshop $Version" },
      @{ Wanted = $Want.Gateway; Exe = $gatewayExe; Expected = "runvil-gateway $gatewayVersion" })) {
    if (-not $check.Wanted) { continue }
    $output = (& $check.Exe --version | Out-String).Trim()
    if ($output -ne $check.Expected) { Fail "$($check.Exe) --version printed '$output', required '$($check.Expected)'" }
  }

  # Workshop loads the std Plugin it installed, which holds the file tools.
  if ($Want.Workshop) {
    $listed = @(& $workshopExe --plugins 2>(Join-Path $logs 'plugins.stderr.log'))
    if (-not ($listed -match '^std \d')) {
      Fail "$workshopExe --plugins must list the std Plugin; it printed '$($listed -join ', ')' and exited $LASTEXITCODE"
    }
    Write-Host "cell: Workshop loads its std Plugin ($($listed -join ', '))"
  }

  # 4. Workshop serves its page, attached to the installed Gateway when
  # there is one and to the fixture endpoint otherwise.
  $origin = $null
  if ($Want.Workshop) {
    New-Item -ItemType Directory -Force -Path $profileDir | Out-Null
    if (-not $Want.Gateway) {
      @'
[gateway]
base_url = "http://127.0.0.1:1"
api_key = "release-smoke"
'@ | Set-Content -LiteralPath (Join-Path $profileDir 'workshop.toml')
    }
    $stdout = Join-Path $logs 'workshop.stdout.log'
    $stderr = Join-Path $logs 'workshop.stderr.log'
    $workshop = Start-Process $workshopExe -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    # With -RedirectStandard*, ExitCode stays empty unless the handle is
    # cached first: https://github.com/PowerShell/PowerShell/issues/5421
    $null = $workshop.Handle
    $port = $null
    foreach ($attempt in 1..120) {
      $workshop.Refresh()
      if ($workshop.HasExited) { Fail "Workshop exited with code $($workshop.ExitCode) before serving its page; see $stdout and $stderr" }
      $ports = @(Get-NetTCPConnection -State Listen -OwningProcess $workshop.Id -ErrorAction SilentlyContinue |
          Where-Object { $_.LocalAddress -eq '127.0.0.1' } | Select-Object -ExpandProperty LocalPort -Unique)
      foreach ($candidate in $ports) {
        try {
          $response = Invoke-WebRequest -Uri "http://127.0.0.1:$candidate/" -TimeoutSec 2
          if ($response.Content -match '<title>Runvil</title>') {
            $port = $candidate
            break
          }
        } catch {
        }
      }
      if ($port) { break }
      Start-Sleep -Seconds 1
    }
    if (-not $port) { Fail 'Workshop never served <title>Runvil</title> on a loopback port within 120 seconds' }
    $script = Join-Path $logs 'app.js'
    Invoke-WebRequest -Uri "http://127.0.0.1:$port/app.js" -OutFile $script
    $size = (Get-Item -LiteralPath $script).Length
    # The minified workbench is about 2.5 MB.
    if ($size -eq 0 -or $size -ge 3000000) { Fail "app.js must be under 3000000 bytes; it is $size" }
    Write-Host "cell: Workshop serves its page on port $port"

    if ($Want.Gateway) {
      $record = Wait-InstalledGateway
      $origin = "http://127.0.0.1:$($record.port)"
      $reported = $null
      foreach ($attempt in 1..30) {
        try { $reported = (Invoke-RestMethod -Uri "http://127.0.0.1:$port/gateway/origin" -TimeoutSec 5).origin } catch { }
        if ($reported -eq $origin) { break }
        Start-Sleep -Seconds 1
      }
      if ($reported -ne $origin) { Fail "Workshop's /gateway/origin must be $origin, the discovery file's; it is '$reported'" }
      Write-Host "cell: Workshop is attached to the installed Gateway at $origin"
    }
  }

  # 5. The Gateway serves its Settings page. Nothing launched it without
  # Workshop, so the cell starts it, from a directory holding no config.
  if ($Want.Gateway) {
    if (-not $origin) {
      $workdir = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
      New-Item -ItemType Directory -Path $workdir | Out-Null
      $stdout = Join-Path $logs 'gateway.stdout.log'
      $stderr = Join-Path $logs 'gateway.stderr.log'
      $gateway = Start-Process $gatewayExe -ArgumentList '--print-url' -WorkingDirectory $workdir -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
      foreach ($attempt in 1..120) {
        # The log shares stdout with the URL, which is a line of its own.
        $match = [regex]::Match((Read-SharedText $stdout), '(?m)^(http://127\.0\.0\.1:\d+)/')
        if ($match.Success) {
          $origin = $match.Groups[1].Value
          break
        }
        $gateway.Refresh()
        if ($gateway.HasExited) { break }
        Start-Sleep -Seconds 1
      }
      if (-not $origin) { Fail "the Gateway must print its http://127.0.0.1 URL under --print-url within 120 seconds; see $stdout and $stderr" }
    }
    Test-ConfigPage $origin
  }

  # 6. The Gateway's first start wrote its config from the gate: with the
  # speech models exactly when Speech to Text was selected.
  if ($Want.Gateway) {
    if (-not (Test-Path -LiteralPath $config)) { Fail "the Gateway's first start must write $config; it is missing" }
    $declares = Select-String -LiteralPath $config -Pattern '^\[\[stt_model\]\]' -Quiet
    if ($Want.Stt -and -not $declares) { Fail "Speech to Text was selected, but the Gateway wrote $config without an [[stt_model]]" }
    if (-not $Want.Stt -and $declares) { Fail "Speech to Text was not selected, but the Gateway wrote $config with an [[stt_model]]" }
    Write-Host "cell: the Gateway's first start wrote $config from the Speech to Text gate"
  }
}

try {
  switch ($Selection) {
    'rejections' {
      foreach ($list in @('stt', 'gateway,bogus')) {
        $code = Invoke-Setup @('/S', "/COMPONENTS=$list")
        if ($code -ne 2) { Fail "/COMPONENTS=$list must exit 2; it exited $code" }
        if (Test-Path -LiteralPath $root) { Fail "/COMPONENTS=$list must install nothing, but $root exists" }
        if (Test-Path -LiteralPath $config) { Fail "/COMPONENTS=$list must write no config, but $config exists" }
      }
    }
    'all' {
      Test-Selection $want
      Stop-Started
      $before = Get-InstallState
      $code = Invoke-Setup @('/P', '/UPDATE') -DeadProxy
      if ($code -ne 0) { Fail "the update install exited $code, required 0" }
      Assert-SameState $before (Get-InstallState) 'the update install'
      Write-Host 'cell: the update install downloaded nothing and kept the config, the models, and the selection'
    }
    'gateway' {
      Test-Selection $want
      Stop-Started
      Start-Process $gatewayExe -ArgumentList '--login' | Out-Null
      $first = Wait-InstalledGateway
      $code = Invoke-Setup @('/P', '/UPDATE')
      if ($code -ne 0) { Fail "the update install exited $code, required 0" }
      $second = Wait-InstalledGateway -NotPid $first.pid -Seconds 60
      Test-ConfigPage "http://127.0.0.1:$($second.port)"
      Write-Host "cell: the update install relaunched the Gateway (pid $($first.pid), then $($second.pid))"
    }
    'gateway-stt' {
      Test-Selection $want
      Stop-Started
      $before = Get-InstallState
      $code = Invoke-Setup @('/S', '/COMPONENTS=gateway,stt') -DeadProxy
      if ($code -ne 0) { Fail "the second install exited $code, required 0" }
      Assert-SameState $before (Get-InstallState) 'the second install'
      Write-Host 'cell: the second install downloaded nothing and kept the config and the models'
    }
    default {
      Test-Selection $want
    }
  }
  Write-Host "cell: $Selection passed"
} finally {
  Stop-Started
  $profileLogs = Join-Path $profileDir 'logs'
  if (Test-Path -LiteralPath $profileLogs) {
    Copy-Item -LiteralPath $profileLogs -Destination (Join-Path $logs 'runvil-logs') -Recurse -Force -ErrorAction SilentlyContinue
  }
}
