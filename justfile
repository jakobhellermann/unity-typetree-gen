fixtures_dir := "fixtures/bin/Release/netstandard2.0"

# Unity versions to snapshot against. Each gets its own tests/snapshots/<v>/
# directory; the harness checks the generator against every one. Spanning an
# old and a current version exercises the version-dependent template branches.
unity_versions := "6000.0.0f1 2019.4.0f1 5.0.0f1"

_default:
	just --list

build-fixtures:
    dotnet build fixtures/Fixtures.csproj -c Release
    cp {{fixtures_dir}}/Fixtures.dll tests/Fixtures.dll
    cp {{fixtures_dir}}/UnityEngine.dll tests/UnityEngine.dll
    cp {{fixtures_dir}}/UnityEngine.CoreModule.dll tests/UnityEngine.CoreModule.dll

# Regenerate the AssetsTools.NET reference snapshots from the built fixtures.
snapshots: build-fixtures
    rm -rf tests/snapshots
    for v in {{unity_versions}}; do \
        dotnet run --project snapshot-gen -c Release -- {{fixtures_dir}} $v tests/snapshots/$v; \
    done

# Rebuild fixtures + snapshots (the committed test inputs).
regen: snapshots

# Run the Rust generator tests against the committed snapshots.
test:
    cargo test
