# Prüft im Windows-Runner das Update per MSI: alte Version installieren, die neue darüber, dann
# Downgrade-Sperre und Deinstallation. Ohne Fenster, ohne Ton (nur msiexec, kein Start der App).
# Aufruf: pwsh packaging/windows/test-upgrade.ps1 -Version 0.34.1 -Bin target\release -Msi woa-0.34.1-x64.msi
# Die alte Version wird aus denselben Programmen mit Version 0.0.1 gebaut. Gleicher UpgradeCode wie
# das neue MSI, sonst wäre das Ergebnis keine Aussage über echte Updates.
# Teil 2 (#452): Umstieg von der Installation für alle Benutzer (bis 0.41.25, -d Legacy=1) auf die
# pro Benutzer. Der Runner ist Admin; ob die Abfrage der Benutzerkontensteuerung ausbleibt, zeigt
# nur ein Gerät mit Standardkonto (Abnahme).
param(
  [Parameter(Mandatory)] [string] $Version,
  [Parameter(Mandatory)] [string] $Bin,
  [Parameter(Mandatory)] [string] $Msi
)
$ErrorActionPreference = 'Stop'
$old = '0.0.1'
$name = 'WOA - Wir on Air'
$dir = Join-Path $env:LOCALAPPDATA 'Programs\WOA'
$machineDir = Join-Path $env:ProgramFiles 'WOA'
# Ein MSI pro Benutzer schreibt keinen Uninstall-Schlüssel unter HKCU (Release-Lauf v0.41.28: 0 Treffer
# nach erfolgreicher Installation); Windows Installer führt es unter Installer\UserData\<SID>. Darum fragt
# der Test Windows Installer selbst, Kontext 2 = pro Benutzer (nicht verwaltet), 4 = für alle Benutzer.
function Get-Msi([int] $context) {
  $wi = New-Object -ComObject WindowsInstaller.Installer
  @($wi.ProductsEx('', '', $context) | ForEach-Object {
      [pscustomobject]@{ ProductCode = $_.ProductCode(); DisplayName = $_.InstallProperty('ProductName'); DisplayVersion = $_.InstallProperty('VersionString') }
    } | Where-Object { $_.DisplayName -eq $name })
}
function Get-Installed { Get-Msi 2 }
function Get-Machine { Get-Msi 4 }
function Run-Msi([string[]] $msiArgs, [int[]] $ok = @(0)) {
  $p = Start-Process msiexec.exe -ArgumentList $msiArgs -Wait -PassThru
  if ($ok -notcontains $p.ExitCode) { throw "msiexec $($msiArgs -join ' ') endete mit $($p.ExitCode)" }
}
function Assert([bool] $cond, [string] $msg) {
  if (-not $cond) { throw "FEHLER: $msg" }
  Write-Host "ok: $msg"
}

if ((Get-Installed).Count + (Get-Machine).Count -ne 0) { throw 'WOA ist auf dem Runner schon installiert; der Test braucht einen sauberen Runner.' }

$tmp = [System.IO.Path]::GetTempPath()  # nicht ins Arbeitsverzeichnis: sonst landet es im Artefakt
$oldMsi = Join-Path $tmp "woa-$old-x64.msi"
wix build packaging/windows/woa.wxs -arch x64 -d Version=$old -d Bin=$Bin -o $oldMsi
if ($LASTEXITCODE -ne 0) { throw 'wix build (alte Version) fehlgeschlagen' }
$legacyMsi = Join-Path $tmp "woa-$old-x64-allusers.msi"
wix build packaging/windows/woa.wxs -arch x64 -d Version=$old -d Bin=$Bin -d Legacy=1 -o $legacyMsi
if ($LASTEXITCODE -ne 0) { throw 'wix build (alte Version für alle Benutzer) fehlgeschlagen' }
$newMsi = (Resolve-Path $Msi).Path
$log = Join-Path $tmp 'msi-upgrade.log'

# --- Teil 1: Update pro Benutzer ---
Run-Msi @('/i', "`"$oldMsi`"", '/qn', '/norestart', '/l*v', "`"$log`"")
$inst = Get-Installed
Assert ($inst.Count -eq 1 -and $inst[0].DisplayVersion -eq $old) "alte Version $old pro Benutzer installiert"
Assert ((Get-Machine).Count -eq 0) 'kein Eintrag für alle Benutzer (HKLM)'
Assert (Test-Path (Join-Path $dir 'woa.exe')) 'woa.exe liegt unter %LOCALAPPDATA%\Programs\WOA'
Assert (-not (Test-Path (Join-Path $machineDir 'woa.exe'))) 'nichts unter Program Files'

Run-Msi @('/i', "`"$newMsi`"", '/qn', '/norestart', '/l*v', "`"$log`"")
$inst = Get-Installed
Assert ($inst.Count -eq 1) 'nach dem Update genau ein Eintrag (die alte Version wurde ersetzt)'
Assert ($inst[0].DisplayVersion -eq $Version) "neue Version $Version installiert"
foreach ($f in 'woa.exe', 'woa-headless.exe', 'woa-streamd.exe', 'LICENSE.txt', 'sounds\GeneralUser-GS.sf2', 'sounds\GeneralUser-GS-LICENSE.txt') {
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

# --- Teil 2: Umstieg für alle Benutzer -> pro Benutzer (#452) ---
$data = Join-Path $env:APPDATA 'woa'
New-Item -ItemType Directory -Force $data | Out-Null
$probe = Join-Path $data 'test-upgrade-452.txt'
Set-Content $probe 'nutzerdaten'

Run-Msi @('/i', "`"$legacyMsi`"", '/qn', '/norestart', '/l*v', "`"$log`"")
$m = Get-Machine
Assert ($m.Count -eq 1 -and $m[0].DisplayVersion -eq $old) "alte Installation für alle Benutzer $old (HKLM)"
Assert (Test-Path (Join-Path $machineDir 'woa.exe')) 'woa.exe liegt unter Program Files'

Run-Msi @('/i', "`"$newMsi`"", '/qn', '/norestart', '/l*v', "`"$log`"")
Assert ((Get-Installed).Count -eq 1) 'neue Version pro Benutzer installiert'
# Erwartet: MajorUpgrade sieht die alte Installation nicht (anderer Kontext). Deshalb löst sie die App ab.
Assert ((Get-Machine).Count -eq 1) 'MSI allein lässt die Installation für alle Benutzer stehen (Grund für legacy.rs)'

# Was die App beim ersten Start tut (crates/updater/src/legacy.rs): dieselbe reg-Abfrage, dann msiexec /x.
$reg = & reg query 'HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall' /s /f $name /d /e /reg:64 | Out-String
$codes = @([regex]::Matches($reg, '\\(\{[0-9A-Fa-f-]{36}\})\s*\r?\n\s+DisplayName\s+REG_SZ\s+' + [regex]::Escape($name) + '\s*\r?\n') | ForEach-Object { $_.Groups[1].Value })
# Der Uninstall-Zweig listet auch die neue Installation pro Benutzer; legacy.rs behält nur Codes, die
# Windows Installer für alle Benutzer führt (Classes\Installer\Products\<gepackte GUID>).
Assert ($codes.Count -ge 1) "reg query findet die alte (und ggf. die neue) Installation ($($codes -join ', '))"
function Get-Packed([string] $code) {
  $h = $code -replace '[{}-]', ''
  $r = { param($t) -join ($t.ToCharArray()[($t.Length - 1)..0]) }
  $p = (& $r $h.Substring(0, 8)) + (& $r $h.Substring(8, 4)) + (& $r $h.Substring(12, 4))
  for ($i = 16; $i -lt 32; $i += 2) { $p += "$($h[$i + 1])$($h[$i])" }
  $p.ToUpper()
}
$codes = @($codes | Where-Object { & reg query "HKLM\SOFTWARE\Classes\Installer\Products\$(Get-Packed $_)" /reg:64 2>$null | Out-Null; $LASTEXITCODE -eq 0 })
Assert ($codes.Count -eq 1) "davon genau einer für alle Benutzer ($($codes -join ', '))"
Assert ($codes[0] -eq (Get-Machine)[0].ProductCode) 'das ist die alte Installation'
$global:LASTEXITCODE = 0
Run-Msi @('/x', $codes[0], '/qn', '/norestart')
Assert ((Get-Machine).Count -eq 0) 'Installation für alle Benutzer entfernt'
Assert (-not (Test-Path (Join-Path $machineDir 'woa.exe'))) 'Program Files\WOA leer'
$inst = Get-Installed
Assert ($inst.Count -eq 1 -and $inst[0].DisplayVersion -eq $Version) 'pro Benutzer bleibt die neue Version, genau ein Eintrag'
Assert (Test-Path (Join-Path $dir 'woa.exe')) 'woa.exe pro Benutzer noch da'
Assert ((Get-Content $probe) -eq 'nutzerdaten') 'Nutzerdaten in %APPDATA%\woa unverändert'

Run-Msi @('/x', "`"$newMsi`"", '/qn', '/norestart')
Assert ((Get-Installed).Count -eq 0) 'Deinstallation pro Benutzer entfernt den Eintrag'
Assert (Test-Path $probe) 'Nutzerdaten überstehen die Deinstallation'
Remove-Item $probe
Write-Host "Umstieg für alle Benutzer -> pro Benutzer bestanden"
