$ErrorActionPreference = "Stop"
$exampleRoot = $PSScriptRoot
$rustOutput = Join-Path $exampleRoot "target/i686-pc-windows-msvc/release"
$includeDirectory = Join-Path $exampleRoot "dist/include"
$packages = @(
    @{ Id = "example.counter"; Source = "counter-provider"; Dll = (Join-Path $rustOutput "example_counter_provider.dll"); Header = "counter.h" },
    @{ Id = "example.counter-consumer"; Source = "counter-consumer"; Dll = (Join-Path $rustOutput "example_counter_consumer.dll"); Header = "counter.h" }
)
$cDll = Join-Path $exampleRoot "counter-c/build/Release/mod.dll"
if (Test-Path $cDll) {
    $packages += @{ Id = "example.counter-c"; Source = "counter-c"; Dll = $cDll; Header = "counter.h" }
}
$hookDll = Join-Path $exampleRoot "hook/target/i686-pc-windows-msvc/release/example_hook.dll"
if (Test-Path $hookDll) {
    $packages += @{ Id = "example.hook"; Source = "hook"; Dll = $hookDll; Header = "probe.h" }
}

$previousHeaderExport = $env:MHF_HEADERS_EXPORT_DIR
try {
    $env:MHF_HEADERS_EXPORT_DIR = $includeDirectory
    cargo test --manifest-path (Join-Path $exampleRoot "Cargo.toml") -p example-counter-provider --test headers --release --target i686-pc-windows-msvc --locked
    if ($LASTEXITCODE -ne 0) { throw "Counter header validation failed" }
    if (Test-Path $hookDll) {
        cargo test --manifest-path (Join-Path $exampleRoot "hook/Cargo.toml") --test headers --release --target i686-pc-windows-msvc --locked
        if ($LASTEXITCODE -ne 0) { throw "Hook header validation failed" }
    }
} finally {
    $env:MHF_HEADERS_EXPORT_DIR = $previousHeaderExport
}

foreach ($package in $packages) {
    $destination = Join-Path $exampleRoot "dist/mods/$($package.Id)/1.0.0"
    New-Item -ItemType Directory -Force -Path $destination | Out-Null
    Copy-Item $package.Dll (Join-Path $destination "mod.dll")
    Copy-Item (Join-Path $exampleRoot "$($package.Source)/mod.toml") $destination
    $projectLicense = Join-Path $exampleRoot "../../../LICENSE"
    if (Test-Path $projectLicense) {
        Copy-Item $projectLicense $destination
    }
    Copy-Item (Join-Path $includeDirectory "mhf_mod.h") $destination
    Copy-Item (Join-Path $includeDirectory $package.Header) $destination
    Write-Output $destination
}
