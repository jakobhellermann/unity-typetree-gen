fixtures_dir := "fixtures/bin/Release/netstandard2.0"
unity_version := "6000.0.0"

_default:
	just --list

build-fixtures:
    dotnet build fixtures/Fixtures.csproj -c Release
    cp {{fixtures_dir}}/Fixtures.dll tests/Fixtures.dll

# Regenerate the AssetsTools.NET reference snapshots from the built fixtures.
snapshots: build-fixtures
    dotnet run --project snapshot-gen -c Release -- {{fixtures_dir}} {{unity_version}} tests/snapshots

# Rebuild fixtures + snapshots (the committed test inputs).
regen: snapshots

# Run the Rust generator tests against the committed snapshots.
test:
    cargo test
