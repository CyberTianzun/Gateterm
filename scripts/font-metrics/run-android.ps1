param(
    [string]$Sdk = 'C:\Programs\android-sdk',
    [string]$Serial = '',
    [string]$BuildTools = '36.1.0',
    [ValidateSet('metrics', 'blocks')][string]$Mode = 'metrics'
)
$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$outputDir = Join-Path $repoRoot 'target/font-metrics'
New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
$classes = Join-Path $outputDir 'classes'
$dex = Join-Path $outputDir 'dex'
New-Item -ItemType Directory -Force -Path $classes,$dex | Out-Null
$androidJar = Join-Path $Sdk 'platforms/android-35/android.jar'
$toolDir = Join-Path $Sdk "build-tools/$BuildTools"
$adb = Join-Path $Sdk 'platform-tools/adb.exe'
$deviceArgs = @()
if ($Serial) { $deviceArgs = @('-s', $Serial) }

function Check-Exit([string]$Step) {
    if ($LASTEXITCODE -ne 0) { throw "$Step failed ($LASTEXITCODE)" }
}

$termSource = Join-Path $repoRoot 'clients/android/app/src/main/java/ai/eclosion/octoterm/android/term'
& "$env:JAVA_HOME/bin/javac.exe" -encoding UTF-8 --release 8 -classpath $androidJar -d $classes (Join-Path $PSScriptRoot 'FontMetricsProbe.java') (Join-Path $PSScriptRoot 'BlockGlyphProbe.java') (Join-Path $termSource 'FontAdvance.java') (Join-Path $termSource 'TerminalCellMetrics.java') (Join-Path $termSource 'TerminalFontMetrics.java') (Join-Path $termSource 'TerminalBlockGlyphs.java')
Check-Exit 'javac'
$classFiles = @(Get-ChildItem -LiteralPath $classes -Recurse -Filter '*.class' | Select-Object -ExpandProperty FullName)
& (Join-Path $toolDir 'd8.bat') --lib $androidJar --min-api 26 --output $dex @classFiles
Check-Exit 'd8'
$unsigned = Join-Path $outputDir 'probe-unsigned.apk'
& (Join-Path $toolDir 'aapt.exe') package -f -M (Join-Path $PSScriptRoot 'AndroidManifest.xml') -I $androidJar -A (Join-Path $repoRoot 'clients/android/app/src/main/assets') -F $unsigned
Check-Exit 'aapt'
& "$env:JAVA_HOME/bin/jar.exe" uf $unsigned -C $dex classes.dex
Check-Exit 'jar'
$apk = Join-Path $outputDir 'probe.apk'
& (Join-Path $toolDir 'zipalign.exe') -f 4 $unsigned $apk
Check-Exit 'zipalign'
$keystore = Join-Path $outputDir 'probe.keystore'
if (-not (Test-Path -LiteralPath $keystore)) {
    & "$env:JAVA_HOME/bin/keytool.exe" -genkeypair -keystore $keystore -storepass android -keypass android -alias probe -keyalg RSA -keysize 2048 -validity 3650 -dname 'CN=Octoterm Diagnostics'
    Check-Exit 'keytool'
}
& (Join-Path $toolDir 'apksigner.bat') sign --ks $keystore --ks-pass pass:android --key-pass pass:android --ks-key-alias probe $apk
Check-Exit 'apksigner'
& $adb @deviceArgs install -r $apk
Check-Exit 'install probe'
$log = & $adb @deviceArgs shell am instrument -w -e mode $Mode ai.eclosion.octoterm.diagnostics.fontmetrics/.FontMetricsProbe
Check-Exit 'run probe'
$stem = if ($Mode -eq 'blocks') { 'android-blocks' } else { 'android' }
$prefix = if ($Mode -eq 'blocks') { 'BLOCK_GLYPHS_JSON=' } else { 'FONT_METRICS_JSON=' }
$log | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $outputDir "$stem.log")
$line = $log | Where-Object { $_.StartsWith($prefix) } | Select-Object -First 1
if (-not $line) { throw "Probe returned no JSON. See $outputDir/$stem.log" }
$json = $line.Substring($prefix.Length)
$json | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $outputDir "$stem.json")
$report = $json | ConvertFrom-Json
$report | Select-Object platform,model,sdk,densityDpi,density,fontScale | Format-List
if ($Mode -eq 'blocks') {
    foreach ($image in $report.images) {
        & $adb @deviceArgs pull $image (Join-Path $outputDir ([IO.Path]::GetFileName($image)))
        Check-Exit 'pull probe image'
    }
    Write-Output "Checked $($report.cases.Count) pixel cases; reproduced old seams in $($report.reproducedOldSeams)."
} else { Write-Output "Measured $($report.rows.Count) cases." }
Write-Output "Results: $outputDir/$stem.json"
