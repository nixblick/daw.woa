# Prüft im Windows-Runner das Update per MSI: alte Version installieren, die neue darüber, dann
# Downgrade-Sperre und Deinstallation. Ohne Fenster, ohne Ton (nur msiexec, kein Start der App).
# Aufruf: pwsh packaging/windows/test-upgrade.ps1 -Version 0.34.1 -Bin target\release -Msi woa-0.34.1-x64.msi
# Die alte Version wird aus denselben Programmen mit Version 0.0.1 gebaut. Gleicher UpgradeCode wie
# das neue MSI, sonst wäre das Ergebnis keine Aussage über echte Updates.
param(
  [Parameter(Mandatory)] [string] $Version,
  [Parameter(Mandatory)] [string] $Bin,
  [Parameter(Mandatory)] [string] $Msi
)
$ErrorActionPreference = 'Stop'
$old = '0.0.1'
$name = 'WOA - Wir on Air'
$dir = Join-Path $env:ProgramFiles 'WOA'

function Get-Installed {
  $keys = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
          'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'
  @(Get-ItemProperty $keys -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -eq $name })
}
function Run-Msi([string[]] $msiArgs, [int[]] $ok = @(0)) {
  $p = Start-Process msiexec.exe -ArgumentList $msiArgs -Wait -PassThru
  if ($ok -notcontains $p.ExitCode) { throw "msiexec $($msiArgs -join ' ') endete mit $($p.ExitCode)" }
}
function Assert([bool] $cond, [string] $msg) {
  if (-not $cond) { throw "FEHLER: $msg" }
  Write-Host "ok: $msg"
}

if ((Get-Installed).Count -ne 0) { throw 'WOA ist auf dem Runner schon installiert; der Test braucht einen sauberen Runner.' }

$oldMsi = Join-Path ([System.IO.Path]::GetTempPath()) "woa-$old-x64.msi"  # nicht ins Arbeitsverzeichnis: sonst landet es im Artefakt
wix build packaging/windows/woa.wxs -arch x64 -d Version=$old -d Bin=$Bin -o $oldMsi
if ($LASTEXITCODE -ne 0) { throw 'wix build (alte Version) fehlgeschlagen' }
$newMsi = (Resolve-Path $Msi).Path
$log = Join-Path ([System.IO.Path]::GetTempPath()) 'msi-upgrade.log'

Run-Msi @('/i', "`"$oldMsi`"", '/qn', '/norestart', '/l*v', "`"$log`"")
$inst = Get-Installed
Assert ($inst.Count -eq 1 -and $inst[0].DisplayVersion -eq $old) "alte Version $old installiert"
Assert (Test-Path (Join-Path $dir 'woa.exe')) 'woa.exe liegt im Programmordner'

Run-Msi @('/i', "`"$newMsi`"", '/qn', '/norestart', '/l*v', "`"$log`"")
$inst = Get-Installed
Assert ($inst.Count -eq 1) 'nach dem Update genau ein Eintrag (die alte Version wurde ersetzt)'
Assert ($inst[0].DisplayVersion -eq $Version) "neue Version $Version installiert"
foreach ($f in 'woa.exe', 'woa-headless.exe', 'woa-streamd.exe', 'LICENSE.txt') {
  Assert (Test-Path (Join-Path $dir $f)) "$f vorhanden"
}
Assert ((Get-Item (Join-Path $dir 'woa.exe')).Length -eq (Get-Item (Join-Path $Bin 'woa.exe')).Length) 'woa.exe stammt aus dem neuen Paket'

# Downgrade: MajorUpgrade mit DowngradeErrorMessage bricht ab (Fehler 1603, in manchen Fällen 1638 = andere Version schon da).
Run-Msi @('/i', "`"$oldMsi`"", '/qn', '/norestart') -ok @(1603, 1638)
Assert ((Get-Installed)[0].DisplayVersion -eq $Version) 'Downgrade abgelehnt, die neue Version bleibt'

Run-Msi @('/x', "`"$newMsi`"", '/qn', '/norestart')
Assert ((Get-Installed).Count -eq 0) 'Deinstallation entfernt den Eintrag'
Assert (-not (Test-Path (Join-Path $dir 'woa.exe'))) 'woa.exe nach der Deinstallation weg'
Write-Host "MSI-Upgrade $old -> $Version bestanden"
