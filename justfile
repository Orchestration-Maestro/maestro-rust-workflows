set shell := ["bash", "-euo", "pipefail", "-c"]
set windows-shell := ["cmd.exe", "/d", "/s", "/c"]

local_bin := justfile_directory() / ".tools/bin"
host_path := env_var("PATH")
export PATH := if os() == "linux" { local_bin + ":" + host_path } else { host_path }

# The North Star speed target: `just check` prints its wall time against it.

export SPEED_TARGET_SECONDS := "40"

# List every recipe and what it does; this is what `just` alone prints.
help:
    @just --list --unsorted

# Install the pinned toolbelt and link it into ignored .tools/bin (Linux x64).
setup:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] || {
      echo 'Pinned tooling supports Linux x64 only' >&2; exit 1;
    }
    # Every tool, its version and the checksum of every download live in
    # mise.toml and mise.lock. mise installs them into its own store and refuses
    # bytes that differ from the lock; mise itself is the one download that
    # scripts/bootstrap.sh verifies by hand, into .tools/bin.
    mise trust mise.toml
    mise install --locked
    # Links keep the gate, the commit hooks and the documentation on the one
    # PATH entry they already use, wherever mise keeps its store.
    mkdir -p .tools/bin
    find .tools/bin -mindepth 1 ! -name mise -delete
    while IFS= read -r directory; do
      for file in "$directory"/*; do
        if [[ -f "$file" && -x "$file" ]]; then
          ln -s -- "$file" ".tools/bin/$(basename -- "$file")"
        fi
      done
    done < <(mise bin-paths)
    # The commit hooks only run if the Git hook is installed; provisioning the
    # tool without wiring it in would leave every contributor to remember this.
    prek install

# Complete local gate; CHECK_NETWORK=1 also requires a live RustSec advisory lookup.
check:
    #!/usr/bin/env bash
    set -euo pipefail
    started=$(date +%s)
    RUSTUP_TOOLCHAIN=$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)
    export RUSTUP_TOOLCHAIN
    for tool in mise just actionlint zizmor yamlfmt taplo shellcheck prek cargo rustup \
      gitleaks typos jaq readelf similarity-rs cargo-hack cargo-nextest; do
      command -v "$tool" >/dev/null ||
        { echo "Missing $tool; run scripts/bootstrap.sh" >&2; exit 1; }
    done
    just --unstable --fmt --check
    for manifest in gate tests; do
      cargo fmt --manifest-path "$manifest/Cargo.toml" --all --check
      cargo clippy --manifest-path "$manifest/Cargo.toml" --all-targets --locked -- -D warnings
    done
    cargo test --manifest-path gate/Cargo.toml --locked --offline
    # The organization's module structure rules hold this repository too:
    # every project here goes through the step consumers run.
    for project in gate tests examples/binary examples/library examples/workspace; do
      scratch=$(mktemp -d)
      PROJECT="$PWD/$project" REPORTS="$scratch" RUNNER_TEMP="$scratch" \
        GITHUB_WORKSPACE="$PWD" GITHUB_STEP_SUMMARY="$scratch/summary.md" \
        cargo run --manifest-path gate/Cargo.toml --locked --offline --quiet -- architecture
      rm -rf "$scratch"
    done
    # And the hygiene every tracked file holds to, once for the whole checkout.
    scratch=$(mktemp -d)
    PROJECT="$PWD" REPORTS="$scratch" RUNNER_TEMP="$scratch" \
      GITHUB_WORKSPACE="$PWD" GITHUB_STEP_SUMMARY="$scratch/summary.md" \
      cargo run --manifest-path gate/Cargo.toml --locked --offline --quiet -- hygiene
    rm -rf "$scratch"
    # The files the gate renders for every repository are this one's too.
    cargo run --manifest-path gate/Cargo.toml --locked --offline --quiet -- sync --check
    cargo run --manifest-path gate/Cargo.toml --locked --offline --quiet -- rules --check
    RUSTDOCFLAGS='-D warnings -D missing_docs' cargo doc --manifest-path gate/Cargo.toml \
      --no-deps --locked --offline --document-private-items
    cargo test --manifest-path tests/Cargo.toml --locked
    RUSTDOCFLAGS='-D warnings -D missing_docs' cargo doc --manifest-path tests/Cargo.toml \
      --no-deps --locked
    cargo deny --offline --config deny.toml --manifest-path tests/Cargo.toml \
      check licenses bans sources
    actionlint -config-file .github/actionlint.yml .github/workflows/*.yml
    zizmor --offline --persona=pedantic --no-progress --config .github/zizmor.yml .github/
    yamlfmt -no_global_conf -lint
    taplo fmt --check
    # Spelling over the working tree, where the hook below reads the index:
    # typos.toml names the words this repository means.
    typos
    # The same hooks a commit runs, over every tracked file. prek reads the
    # index, so before the first commit it has no file and every hook skips;
    # a skip that looks like a pass is the one thing this gate must not do.
    [[ "$(git ls-files | wc -l)" -gt 0 ]] ||
      echo 'NOT RUN: the commit hooks; git tracks no file yet, so every hook skips'
    prek run --all-files
    # The example gate is a Rust test that replays ci.yml's own step bodies,
    # read by id, against every owned fixture with the pinned toolbelt, so the
    # local gate cannot drift from CI. Plain `cargo test` skips it; this is its
    # one caller.
    cargo test --manifest-path tests/Cargo.toml --locked -- --ignored --nocapture example_gate
    # The toolbelt links, ShellCheck over every Bash line left, Gitleaks over
    # the tree and, under CHECK_NETWORK=1, the RustSec audits are contract tests.
    if [[ "${CHECK_NETWORK:-0}" != 1 ]]; then
      echo 'NOT RUN: live advisory database check; run CHECK_NETWORK=1 just check'
    fi
    elapsed=$(( $(date +%s) - started ))
    echo "SPEED: $elapsed s wall, target $SPEED_TARGET_SECONDS s"
    echo 'PASS: local gate. GitHub/registry integration is not exercised locally.'

# Move every pinned tool to its latest release wherever it is installed (network, gh).
[linux]
update-tools:
    #!/usr/bin/env bash
    set -euo pipefail
    # mise.toml first, mise.lock through mise, then each workflow install row
    # from the lock with the digest of the bytes the new asset serves, and the
    # Codecov CLI row. One line per move; no output means all current.
    declare -A was=() now=() asset=() digest=()
    version='^[0-9A-Za-z][0-9A-Za-z.+-]*$'
    while read -r name current; do
      [[ "$name" =~ ^[a-z0-9-]+$ ]] || { echo "unexpected tool name: ${name}" >&2; exit 1; }
      latest="$(mise latest "$name")"
      [[ "$latest" =~ $version ]] || { echo "${name}: no release version from mise" >&2; exit 1; }
      [[ "$latest" != "$current" ]] || continue
      sed -i -E "s/^(${name} = (\{ version = )?\")${current//./\\.}\"/\1${latest}\"/" mise.toml
      grep -q "^${name} = .*\"${latest}\"" mise.toml || {
        echo "${name}: cannot move its version in mise.toml" >&2; exit 1;
      }
      was[$name]="$current"
      now[$name]="$latest"
      major=''
      [[ "${current%%.*}" == "${latest%%.*}" ]] || major=' (major)'
      echo "${name} ${current} -> ${latest}${major}"
    done < <(jaq -r --from toml \
      '.tools | to_entries[] | "\(.key) \(.value | if type == "object" then .version else . end)"' \
      mise.toml)
    if (( ${#was[@]} )); then
      just _lock "${!was[@]}"
      # $1 is a checked tool name, $2 a literal field.
      locked() {
        jaq -r --from toml ".tools[\"$1\"][0][\"platforms.linux-x64\"].$2 // \"\"" mise.lock
      }
      locked_platform() {
        jaq -r --from toml --arg platform "$2" \
          ".tools[\"$1\"][0][\"platforms.${platform}-x64\"].$3 // \"\"" mise.lock
      }
      downloads="$(mktemp -d)"
      trap 'rm -rf "$downloads"' EXIT
      for name in "${!was[@]}"; do
        url="$(locked "$name" url)"
        [[ "$url" == https://github.com/*/releases/download/*/* ]] || {
          echo "${name}: mise.lock names no GitHub release asset" >&2; exit 1;
        }
        path="${url#https://github.com/}"
        rest="${path#*/releases/download/}"
        tag="${rest%/*}"
        # A tag with a slash is one path segment in a workflow row.
        repository="${path%%/releases/download/*}"
        asset[$name]="${repository}/releases/download/${tag//\//%2F}/${rest##*/}"
        curl --fail --silent --show-error --location --retry 4 --retry-all-errors \
          -o "$downloads/$name" "$url"
        digest[$name]="$(sha256sum "$downloads/$name" | cut -d' ' -f1)"
        checksum="$(locked "$name" checksum)"
        sha256="${checksum#sha256:}"
        if [[ "$checksum" == sha256:* && "$sha256" != "${digest[$name]}" ]]; then
          echo "${name}: the download differs from mise.lock" >&2; exit 1
        fi
      done
      row='^([[:space:]]*(TOOLS:[[:space:]])?)([a-z0-9-]+) ([a-z-]+ )?([^ ]+/releases/download/[^ ]+) ([0-9a-f]{64})( (.+))?$'
      for file in .github/workflows/*.yml; do
        moved="$(mktemp)"
        while IFS= read -r line; do
          if [[ "$line" =~ $row ]] && [[ -n "${was[${BASH_REMATCH[3]}]:-}" ]]; then
            name="${BASH_REMATCH[3]}"
            platform="${BASH_REMATCH[4]% }"
            member="${BASH_REMATCH[8]}"
            member="${member//"${was[$name]}"/"${now[$name]}"}"
            if [[ -n "$platform" ]]; then
              url="$(locked_platform "$name" "$platform" url)"
              checksum="$(locked_platform "$name" "$platform" checksum)"
              digest_value="${checksum#sha256:}"
              [[ "$url" == https://github.com/*/releases/download/*/* && "$checksum" == sha256:* ]] || {
                echo "${name} ${platform}: mise.lock has no pinned release asset" >&2; exit 1;
              }
              path="${url#https://github.com/}"
              rest="${path#*/releases/download/}"
              tag="${rest%/*}"
              repository="${path%%/releases/download/*}"
              asset_value="${repository}/releases/download/${tag//\//%2F}/${rest##*/}"
            else
              asset_value="${asset[$name]}"
              digest_value="${digest[$name]}"
            fi
            line="${BASH_REMATCH[1]}${name} ${platform:+$platform }${asset_value} ${digest_value}"
            line+="${member:+ $member}"
          fi
          printf '%s\n' "$line"
        done < "$file" > "$moved"
        cat "$moved" > "$file"
        rm "$moved"
      done
    fi
    # The Codecov CLI runs only in ci.yml's upload job, so mise does not lock
    # it: its row moves to the latest release, at the digest GitHub records.
    ci=.github/workflows/ci.yml
    cli_asset='codecov codecov/codecov-cli/releases/download/'
    used="$(grep -oE "${cli_asset}v[0-9][0-9.]*/" "$ci" | head -1)"
    used="${used#"$cli_asset"}"
    used="${used%/}"
    read -r cli recorded < <(gh api repos/codecov/codecov-cli/releases/latest --jq \
      '.tag_name + " " + (.assets[] | select(.name == "codecovcli_linux") | .digest)')
    [[ "$cli" =~ ^v[0-9]+(\.[0-9]+)*$ ]] || { echo "codecov-cli: no release tag" >&2; exit 1; }
    if [[ "$cli" != "$used" ]]; then
      cli_download="$(mktemp)"
      curl --fail --silent --show-error --location --retry 4 --retry-all-errors \
        -o "$cli_download" \
        "https://github.com/codecov/codecov-cli/releases/download/${cli}/codecovcli_linux"
      sum="$(sha256sum "$cli_download" | cut -d' ' -f1)"
      rm "$cli_download"
      [[ "$recorded" == "sha256:${sum}" ]] || {
        echo "codecov-cli: the download differs from the release's digest" >&2; exit 1;
      }
      from="${cli_asset}${used}/codecovcli_linux [0-9a-f]{64}"
      sed -i -E "s#${from}#${cli_asset}${cli}/codecovcli_linux ${sum}#" "$ci"
      echo "codecov-cli ${used} -> ${cli}"
    fi

# Lock each tool named, or every tool, on every platform its `os` field allows,
# and hash each download GitHub publishes no digest for, so `--locked` verifies
# every download `rust-gate setup` makes on every platform.
[linux]
_lock *$TOOLS:
    #!/usr/bin/env bash
    set -euo pipefail
    read -r -a names <<< "$TOOLS"
    if (( ! ${#names[@]} )); then
      mapfile -t names < <(jaq -r --from toml '.tools | keys[]' mise.toml)
    fi
    allowed='.tools[env.TOOL] | if type == "object" and has("os") then .os[]'
    allowed+=' else "linux", "macos", "windows" end'
    for name in "${names[@]}"; do
      [[ "$name" =~ ^[a-z0-9-]+$ ]] || { echo "unexpected tool name: ${name}" >&2; exit 1; }
      platforms=''
      while read -r os; do
        case "$os" in
          linux) platforms+=,linux-x64,linux-x64-musl,linux-arm64 ;;
          linux/x64) platforms+=,linux-x64,linux-x64-musl ;;
          linux/arm64) platforms+=,linux-arm64 ;;
          macos) platforms+=,macos-x64,macos-arm64 ;;
          macos/x64 | macos/arm64) platforms+=",macos-${os#macos/}" ;;
          windows) platforms+=,windows-x64 ;;
          *) echo "${name}: unknown os entry ${os}" >&2; exit 1 ;;
        esac
      done < <(TOOL="$name" jaq -r --from toml "$allowed" mise.toml)
      # Progress goes to stderr: stdout is the list of moves, a commit message.
      mise lock --platform "${platforms#,}" "$name" >&2
    done
    downloads="$(mktemp -d)"
    trap 'rm -rf "$downloads"' EXIT
    unsigned='.tools | to_entries[] | .key + " " + (.value[] | to_entries[]'
    unsigned+=' | select((.key | startswith("platforms.")) and .value.url != null)'
    unsigned+=' | select(.value.checksum == null)'
    unsigned+=' | (.key | ltrimstr("platforms.")) + " " + .value.url)'
    while read -r name platform url; do
      [[ "$url" == https://github.com/*/releases/download/*/* ]] || {
        echo "${name} ${platform}: mise.lock names no GitHub release asset" >&2; exit 1;
      }
      curl --fail --silent --show-error --location --retry 4 --retry-all-errors \
        -o "$downloads/asset" "$url"
      digest="$(sha256sum "$downloads/asset" | cut -d' ' -f1)"
      table="^\\[tools\\.${name}\\.\"platforms\\.${platform}\"\\]\$"
      sed -i "/${table}/a checksum = \"sha256:${digest}\"" mise.lock
    done < <(jaq -r --from toml "$unsigned" mise.lock)

# Regenerate every generated document: the steps, every generated table, the
# managed files, and the organization's lints in every crate's manifest.
[linux]
docs:
    cargo run --manifest-path gate/Cargo.toml --locked --offline --quiet -- describe \
      > docs/steps.md
    just _tables
    cargo run --manifest-path gate/Cargo.toml --locked --offline --quiet -- sync
    cargo run --manifest-path gate/Cargo.toml --locked --offline --quiet -- rules
    for crate in gate tests examples/binary examples/library examples/workspace; do \
      (cd "$crate" && cargo run --manifest-path "{{ justfile_directory() }}/gate/Cargo.toml" \
        --locked --offline --quiet -- lints --write); \
    done

# Commit every changed file of the checkout onto $BRANCH as the organization's
# bot: through createCommitOnBranch, which GitHub signs, where a commit made on
# the runner would be unsigned and the organization refuses it. The new commit's
# parent is $HEAD, which must still be the branch's head. Reads GH_TOKEN,
# GITHUB_REPOSITORY, BRANCH, HEAD, TITLE, BODY, a file, and PATHS, the pathspecs
# a commit may take; unset, every changed file.
_commit-as-bot:
    #!/usr/bin/env bash
    set -euo pipefail
    : "${GH_TOKEN:?}" "${GITHUB_REPOSITORY:?}" "${BRANCH:?}" "${HEAD:?}" "${TITLE:?}" "${BODY:?}"
    read -r -a paths <<< "${PATHS:-}"
    files="$(mktemp)"
    while IFS= read -r path; do
      jaq -n --arg path "$path" --arg contents "$(base64 -w0 "$path")" \
        "{path: \$path, contents: \$contents}" >> "$files"
    done < <(git diff --name-only -- "${paths[@]}")
    if [[ ! -s "$files" ]]; then
      echo "Nothing changed; nothing to commit."
      exit 0
    fi
    mutation="mutation(\$input: CreateCommitOnBranchInput!) {"
    mutation+=" createCommitOnBranch(input: \$input) { commit { oid } } }"
    input="{branch: {repositoryNameWithOwner: \$repo, branchName: \$branch},"
    input+=" expectedHeadOid: \$head, message: {headline: \$title, body: \$body},"
    input+=" fileChanges: {additions: \$files}}"
    jaq -n --arg query "$mutation" --arg repo "$GITHUB_REPOSITORY" --arg branch "$BRANCH" \
      --arg head "$HEAD" --arg title "$TITLE" --rawfile body "$BODY" \
      --slurpfile files "$files" "{query: \$query, variables: {input: ${input}}}" \
      > "$files.json"
    gh api graphql --input "$files.json" --jq '.data.createCommitOnBranch.commit.oid'

# Rewrite each table between generated markers in README.md and docs/ from its
# source: workflow inputs and outputs, `# Contract:` lines, mise.toml's `# tool:`
# lines and docs/gates.toml. A test runs it on a copy and refuses a stale table.
_tables:
    #!/usr/bin/env bash
    set -euo pipefail
    workflows=.github/workflows
    # A Markdown code span, kept out of single quotes where ShellCheck reads a
    # backtick as a command substitution.
    tick=$'\x60'
    generate() {
      local kind="$1" file name contract first group language does
      shift
      case "$kind" in
        inputs)
          jaq -r --from yaml --arg columns "$2" -f docs/generators/inputs.jq "$workflows/$1" ;;
        shared-inputs | own-inputs)
          jaq -r --from yaml --slurpfile other <(jaq --from yaml -c . "$workflows/$2") \
            -f "docs/generators/$kind.jq" "$workflows/$1" ;;
        outputs)
          jaq -r --from yaml -f docs/generators/outputs.jq "$workflows/$1" ;;
        gates)
          jaq -r --from toml --arg kind "$1" -f docs/generators/gates.jq docs/gates.toml ;;
        workflows)
          printf '%s\n' '| Workflow | Contract |' '| --- | --- |'
          # ci.yml first: it is the workflow every consumer calls.
          first=1
          for file in "$workflows/ci.yml" "$workflows"/*.yml; do
            if [[ "$file" == "$workflows/ci.yml" ]]; then
              (( first )) || continue
              first=0
            fi
            grep -q '^  workflow_call:' "$file" || continue
            contract="$(grep -m1 '^# Contract: ' "$file")" || {
              echo "${file}: a reusable workflow needs a '# Contract:' line" >&2; return 1;
            }
            name="${file##*/}"
            printf '| [%s](%s) | %s |\n' "${tick}${name}${tick}" "$file" \
              "${contract#\# Contract: }"
          done ;;
        toolbelt)
          printf '%s\n' '| Tool | What it does | Built with |' '| --- | --- | --- |'
          while IFS='|' read -r name group language does; do
            read -r name <<< "${name#\# tool: }"
            read -r group <<< "$group"
            read -r language <<< "$language"
            read -r does <<< "$does"
            [[ "$group" == "$1" ]] || continue
            printf '| %s | %s | %s |\n' "${tick}${name}${tick}" "$does" "$language"
          done < <(grep '^# tool: ' mise.toml) ;;
        gate-rules)
          printf '%s\n' '| Rule | Name | What it holds | Exception |' '| --- | --- | --- | --- |'
          while IFS=$'\t' read -r name group language does; do
            [[ -n "$name" && "$name" != \#* ]] || continue
            if [[ "$language" == exception ]]; then language=yes; else language=no; fi
            printf '| %s | %s | %s | %s |\n' "$name" "$group" "$does" "$language"
          done < gate/src/checks/gate_rules.tsv ;;
        *)
          echo "unknown generated table: ${kind}" >&2; return 1 ;;
      esac
    }
    # A tool pinned without its `# tool:` line would vanish from the README.
    while read -r pinned; do
      grep -q "^# tool: ${pinned} |" mise.toml || {
        echo "mise.toml pins ${pinned} without a '# tool:' line above it" >&2; exit 1;
      }
    done < <(jaq -r --from toml '.tools | keys[]' mise.toml)
    # The diagram quotes how many controls the scorecard reports; a test runs
    # the scorecard and holds this count to it.
    controls="$(sed -n '/^fn controls()/,/^}/p; /^fn organization_controls()/,/^}/p' \
      gate/src/steps/quality_scorecard/step.rs | grep -oE '"(enforced|optional)"' | wc -l)"
    sed -i -E "s/>[0-9]+ controls</>${controls} controls</" .github/assets/how-it-works.svg
    for document in README.md docs/*.md; do
      rendered="$(mktemp)"
      inside=0
      while IFS= read -r line; do
        if [[ "$line" =~ ^'<!-- generated by just docs: '(.+)' -->'$ ]]; then
          read -r -a spec <<< "${BASH_REMATCH[1]}"
          printf '%s\n\n' "$line"
          generate "${spec[@]}"
          printf '\n'
          inside=1
        elif [[ "$line" == '<!-- end generated -->' ]]; then
          printf '%s\n' "$line"
          inside=0
        elif (( ! inside )); then
          printf '%s\n' "$line"
        fi
      done < "$document" > "$rendered"
      cat "$rendered" > "$document"
      rm "$rendered"
    done
