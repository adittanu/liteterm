# Runs the team vault tests on the linked Supabase project inside one
# transaction that is rolled back: nothing is left behind.
#   pwsh supabase/tests/run.ps1          tests against the schema as applied
#   pwsh supabase/tests/run.ps1 -Fresh   runs the migration first (a project
#                                        that does not have it yet)
# Needs the Supabase CLI, logged in and linked (supabase link --project-ref …).
param([switch]$Fresh)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$migration = if ($Fresh) {
  (Get-ChildItem (Join-Path $root 'migrations') -Filter '2026111600000*_*.sql' | Sort-Object Name | ForEach-Object { Get-Content -Raw $_.FullName }) -join "`n"
} else { '' }
$tests = Get-Content -Raw (Join-Path $PSScriptRoot 'team_vaults_test.sql')
$script = "begin;`n$migration`n$tests`nrollback;`n"
$file = Join-Path ([IO.Path]::GetTempPath()) 'liteterm-team-vaults-test.sql'
[IO.File]::WriteAllText($file, $script)
try {
  supabase db query --linked -f $file
} finally {
  Remove-Item $file -ErrorAction SilentlyContinue
}
