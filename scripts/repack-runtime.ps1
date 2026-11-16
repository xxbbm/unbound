param([Parameter(Mandatory=$true)][string]$Stage)
$ErrorActionPreference='Stop'
$project=Split-Path $PSScriptRoot
$stagePath=(Resolve-Path -LiteralPath $Stage).Path
$files=Get-ChildItem -LiteralPath $stagePath -Recurse -File | Sort-Object FullName
if(($files | Measure-Object Length -Sum).Sum -gt 256MB){throw 'Runtime exceeds extraction limit'}
$index=[ordered]@{}
foreach($file in $files){$relative=$file.FullName.Substring($stagePath.Length+1).Replace('\','/');$index[$relative]=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLower()}
[IO.File]::WriteAllText((Join-Path $project 'assets/runtime-index.json'),($index|ConvertTo-Json -Depth 4),[Text.UTF8Encoding]::new($false))
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive=Join-Path ([IO.Path]::GetTempPath()) ('atk-runtime-'+[Guid]::NewGuid().ToString('N')+'.zip')
[IO.Compression.ZipFile]::CreateFromDirectory($stagePath,$archive,[IO.Compression.CompressionLevel]::Optimal,$false)
Move-Item -LiteralPath $archive -Destination (Join-Path $project 'assets/runtime.zip') -Force
Write-Output "Runtime packaged: $($files.Count) files"
