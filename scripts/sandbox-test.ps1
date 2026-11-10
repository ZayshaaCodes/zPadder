# Tries a ZPadder release zip on a clean Windows in Windows Sandbox: a throwaway copy of Windows
# with no developer tools or runtimes, wiped when its window closes.
#
#   powershell -ExecutionPolicy Bypass -File scripts\sandbox-test.ps1            # latest release
#   powershell -ExecutionPolicy Bypass -File scripts\sandbox-test.ps1 -Zip dist\zpadder-0.3.0-windows-x64.zip
#
# Windows Sandbox comes with Windows 10/11 Pro, Enterprise and Education. Turn it on once, in an
# administrator PowerShell, then restart:
#
#   Enable-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM -All
#
# The sandbox gets the zip in a read-only folder, unpacks it to its desktop, puts a checklist next
# to it and opens the release page, so the download a user makes (and its SmartScreen warning)
# can be tried too. Sandboxes see no USB or Bluetooth devices, so controllers can't be tested in
# one; try a controller on another PC.

param(
    # A local release zip to test instead of downloading the latest release.
    [string]$Zip,
    # The GitHub repository the release comes from.
    [string]$Repository = 'ZayshaaCodes/zPadder',
    # Prepare the files but don't start the sandbox.
    [switch]$NoLaunch
)

$ErrorActionPreference = 'Stop'
$sandbox = Join-Path $env:WINDIR 'System32\WindowsSandbox.exe'
if (-not $NoLaunch -and -not (Test-Path $sandbox)) {
    throw "Windows Sandbox is not turned on. In an administrator PowerShell run`n" +
        "  Enable-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM -All`n" +
        "then restart Windows and run this script again."
}

$work = Join-Path $env:TEMP ("zpadder-sandbox-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $work | Out-Null

if ($Zip) {
    Copy-Item $Zip $work
    $zipName = Split-Path -Leaf $Zip
    $releasePage = "https://github.com/$Repository/releases"
} else {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $release = Invoke-RestMethod "https://api.github.com/repos/$Repository/releases/latest" `
        -Headers @{ 'User-Agent' = 'zpadder-sandbox-test' }
    $asset = $release.assets | Where-Object { $_.name -match 'windows-x64\.zip$' -and $_.name -notmatch 'symbols' } |
        Select-Object -First 1
    if (-not $asset) { throw "Release $($release.tag_name) has no Windows zip." }
    $sums = $release.assets | Where-Object { $_.name -eq 'SHA256SUMS.txt' } | Select-Object -First 1
    $zipName = $asset.name
    Write-Host "Downloading $zipName from release $($release.tag_name)..."
    Invoke-WebRequest $asset.browser_download_url -OutFile (Join-Path $work $zipName) -UseBasicParsing
    if ($sums) {
        $expected = ((Invoke-WebRequest $sums.browser_download_url -UseBasicParsing).Content -split "`n" |
            Where-Object { $_ -match [regex]::Escape($zipName) } | Select-Object -First 1) -replace '\s.*$', ''
        $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $work $zipName)).Hash.ToLowerInvariant()
        if ($expected -ne $actual) { throw "$zipName does not match SHA256SUMS.txt" }
        Write-Host "Checksum matches SHA256SUMS.txt."
    }
    $releasePage = $release.html_url
}

# What to try, left on the sandbox's desktop.
$checklist = @"
ZPadder clean-machine test
==========================

This sandbox is a fresh Windows with no developer tools or runtimes. ZPadder is unpacked in the
"ZPadder" folder on this desktop.

1. Starting
   [ ] zpadder.exe in the ZPadder folder starts. No "VCRUNTIME140.dll was not found" or other
       missing-DLL message.
   [ ] The window shows a drawn controller and "No controller connected". (A sandbox sees no
       USB or Bluetooth devices, so that is expected.)
   [ ] If a message says ZPadder needs OpenGL 2, write that down: the sandbox's graphics have
       no OpenGL driver, and some virtual machines and remote desktops won't either.

2. Using it
   [ ] File > New from template > First-person shooter shows its labels.
   [ ] File > Save as saves it to Documents; File > Open opens it again.
   [ ] View > Layout > Sheet shows the labels beside the controller.
   [ ] Closing the window leaves ZPadder in the notification area (tray); Quit from the tray
       menu closes it.
   [ ] Help > Open log folder opens a folder with zpadder.log in it.
   [ ] The "Xbox controller (all games)" template says ViGEmBus is needed, and
       "Get ViGEmBus" opens its download page.

3. Command line (in a Command Prompt, in the ZPadder folder)
   [ ] zpadder-cli.exe check profiles\notepad-test.toml prints OK.

4. Downloading it as a user would
   The release page is open in Edge. Download the zip there, right-click it > Extract All,
   then run zpadder.exe from the extracted folder.
   [ ] SmartScreen says "Windows protected your PC" (the build isn't signed yet).
       More info > Run anyway starts ZPadder.

Not testable here: controllers, mapping into games, ViGEmBus. Try those on another PC.
Closing the sandbox window deletes everything in it.
"@
Set-Content -Path (Join-Path $work 'CLEAN-MACHINE-TEST.txt') -Value $checklist -Encoding ASCII

# Runs inside the sandbox when it signs in.
$setup = @"
`$desktop = [Environment]::GetFolderPath('Desktop')
Expand-Archive -LiteralPath 'C:\zpadder-test\$zipName' -DestinationPath "`$desktop\ZPadder-unpacked" -Force
`$inner = Get-ChildItem "`$desktop\ZPadder-unpacked" -Directory | Select-Object -First 1
Move-Item `$inner.FullName "`$desktop\ZPadder"
Remove-Item "`$desktop\ZPadder-unpacked"
Copy-Item 'C:\zpadder-test\CLEAN-MACHINE-TEST.txt' `$desktop
Start-Process notepad.exe "`$desktop\CLEAN-MACHINE-TEST.txt"
Start-Process explorer.exe "`$desktop\ZPadder"
Start-Process 'microsoft-edge:$releasePage'
"@
Set-Content -Path (Join-Path $work 'setup.ps1') -Value $setup -Encoding ASCII

$hostFolder = [System.Security.SecurityElement]::Escape($work)
$config = @"
<Configuration>
  <VGpu>Enable</VGpu>
  <Networking>Enable</Networking>
  <MemoryInMB>4096</MemoryInMB>
  <MappedFolders>
    <MappedFolder>
      <HostFolder>$hostFolder</HostFolder>
      <SandboxFolder>C:\zpadder-test</SandboxFolder>
      <ReadOnly>true</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>powershell.exe -ExecutionPolicy Bypass -WindowStyle Hidden -File C:\zpadder-test\setup.ps1</Command>
  </LogonCommand>
</Configuration>
"@
$wsb = Join-Path $work 'zpadder.wsb'
Set-Content -Path $wsb -Value $config -Encoding ASCII

if ($NoLaunch) {
    Write-Host "Prepared $wsb (not started)."
} else {
    Write-Host "Starting Windows Sandbox. The checklist opens on its desktop once it has signed in."
    Start-Process $wsb
}
