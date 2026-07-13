param(
  [Parameter(Mandatory = $true)]
  [uint32]$OwnedPid,

  [Parameter(Mandatory = $true)]
  [ValidateNotNullOrEmpty()]
  [string]$ExpectedCreationId
)

$ErrorActionPreference = "Stop"

try {
  if ($OwnedPid -le 1) {
    throw "OwnedPid must be greater than 1"
  }

  try {
    $process = Get-Process -Id $OwnedPid -ErrorAction Stop
  }
  catch {
    if ($_.FullyQualifiedErrorId -like "NoProcessFoundForGivenId,*") {
      Write-Output "gone"
      exit 0
    }
    throw
  }

  $actualCreationId = $process.StartTime.ToUniversalTime().ToFileTimeUtc().ToString(
    [Globalization.CultureInfo]::InvariantCulture
  )
  if ($actualCreationId -cne $ExpectedCreationId) {
    Write-Output "reused"
    exit 0
  }

  # 身份核验与终止必须使用同一个 Process 实例及其句柄。
  $process.Kill()
  Write-Output "terminated"
  exit 0
}
catch {
  Write-Error -ErrorRecord $_
  exit 1
}
