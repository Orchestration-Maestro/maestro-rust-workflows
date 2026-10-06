# Caller-declared checks

Add `.github/ci.toml` at the caller repository root to opt into isolated tests,
real search tools and optional builds. Organization calls use that root; the
workflow's directory input exists only for its fixture self-test.

```toml
wasm_crates = ["my-browser-library"]
check_targets = ["x86_64-pc-windows-msvc"]
browser_build = false
```

## Declaration

Both arrays are required, including when empty. `browser_build` is optional
and defaults to `false`. The [schema](../.github/ci.schema.json) describes the
parsed TOML shape; there is no version key or older-format reader.

Only an absent file keeps the original four checks without isolation, search
tool installation or optional jobs. An unreadable file, malformed TOML,
duplicate key, unknown key, missing array, scalar array value, non-string
member or non-boolean browser flag fails declaration validation. An empty
present document is invalid. Integer and string booleans are not accepted.

Names are exact, without whitespace or Unicode normalization. Each wasm name
must match a package in the caller's locked Cargo metadata. Each target must
match the caller compiler's target list and the runner table below. An unknown
package, unknown target or recognized target without a runner fails with a
file/key diagnostic before optional work is emitted. Metadata or compiler
inspection failures also fail validation. Array order and repetitions are
retained; declaration text is never executed as shell syntax.

## Isolated tests

Opted-in Linux tests install real `ripgrep` and `fd-find`, exposing `fdfind` as
`fd`. Before tests start, a presence guard rejects exported provider API/OAuth
keys, GitHub/Hugging Face tokens, cloud project/profile/region selectors,
cloud credential paths/URIs and the extensive-model selector listed in the
[shared specification](https://github.com/Orchestration-Maestro/.github/blob/main/docs/specs/2026-10-04-shared-rust-ci.md).
It additionally rejects every exported name ending in `_API_KEY`, including
empty values. Diagnostics identify variable names, never credential values.
The exact guarded list is in the shared workflow's isolated-test step.

The child keeps only `PATH`, resolved `CARGO_HOME` and `RUSTUP_HOME`, fresh
`HOME`, `TMPDIR`, `TMP`, `TEMP` and all XDG configuration/data/cache/state/runtime
locations, plus `MAESTRO_NO_LOCAL_LLM=1`. `TMP` and `TEMP` equal `TMPDIR`; the
other locations are separate, initially empty directories beneath a unique
runner-temp parent. Unrelated CI state, proxy settings and application/provider
overrides are not inherited. The test argv stays
`cargo test --workspace --locked`.

The original home and auth file are never read, moved or modified. Only the
isolated directories are cleaned on success or failure; other action steps
retain the runner's normal home and cache access.

## Wasm and browser builds

For each listed package, Linux runs:

```bash
cargo build --locked --target wasm32-unknown-unknown -p "$CRATE"
```

The caller toolchain installs wasm support only when there are listed
packages. With `browser_build = true`, the workflow subsequently runs
`mise install`, then `mise exec -- just browser-build` from the caller directory,
including when the wasm array is empty. A failed wasm build prevents the
recipe. False or omitted flags skip the browser tooling and recipe.

Provide a `mise.toml` pinning the recipe's tools, including `just`, and a
`justfile` with `browser-build`. A missing tool configuration, installation
failure, missing recipe or nonzero recipe result fails CI. The shared workflow
bootstraps mise 2026.10.2, reusing that exact installed version or verifying the
published SHA-256 of its official Linux x64 binary before execution. It does
not override the caller's tool versions; the self-test uses just 1.58.0.

The caller owns the browser application, entry, temporary bundles and
application-specific diagnostics. A wasm build or recipe invocation alone is
not proof of browser runtime behavior.

## Foreign-target checks

The caller's rustup toolchain installs each requested target, then runs:

```bash
cargo check --workspace --all-targets --locked --target "$TARGET"
```

| Target | Runner |
| --- | --- |
| `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` |
| `aarch64-apple-darwin` | `macos-latest` |
| `x86_64-apple-darwin` | `macos-latest` (requested target installed) |
| `x86_64-pc-windows-msvc` | `windows-latest` |

There is no Linux fallback for Apple/MSVC targets. These compile checks do not
restore macOS/Windows runtime tests, which remain a before-first-release
requirement. All opted-in jobs run in both ordinary pull requests and merge
queue checks. Declaration errors fail CI even when dependent jobs skip.

## Adoption and verification

Keep standard Rust files and the existing organization-managed workflow call;
no extra workflow or ruleset is needed. The four original quality/docs/tests
commands remain required. Self-tests exercise production step bodies,
isolated positive/negative controls, real tools, wasm success/failure, caller
pins, bootstrap verification and native platform diagnostics. Both existing
required results aggregate the new positive caller and workflow proofs.

Releases and organization/caller pins are coordinated separately. A code
change or release tag alone does not prove adoption: observe the immutable
release, pin/ruleset read-back and required checks after repinning.
