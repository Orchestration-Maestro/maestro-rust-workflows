# Changelog

## [4.4.0](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v4.3.1...v4.4.0) (2026-09-27)


### Features

* shard mutation testing across remote runners ([#84](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/84)) ([6a8e673](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/6a8e6730356ad69773fd755d9fc343e0de6bdcb7))

## [Unreleased]

### Changed

* Keep the default mutation verdicts while documenting the added `mutants-plan.txt` and first-parent `mutants.diff` evidence; fail closed when Git cannot resolve the mutation scope instead of widening it to the whole workspace.
* Run up to 8 mutation workers per CI group, with plans capped at 64 shards; five concurrent merge groups may request 40 worker slots against the existing 60-runner capacity.
* Stop each mutation worker after 30 minutes, preserve its partial outcomes, and fail aggregation with the incomplete shard indices.

## [4.3.1](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v4.3.0...v4.3.1) (2026-09-26)


### Bug Fixes

* package only the members that may be published ([#82](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/82)) ([e980b30](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/e980b30c1e5f6aceabafe9373ac5b6f7b3cf5a52))

## [4.3.0](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v4.2.0...v4.3.0) (2026-09-26)


### Features

* call this repository by its new name ([#80](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/80)) ([c6036ec](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/c6036ecaaf0a9c37c18386d67665c70e3fa588ee))

## [4.2.0](https://github.com/Orchestration-Maestro/rust-workflows/compare/v4.1.0...v4.2.0) (2026-09-26)


### Features

* check private docs and justfiles centrally ([#77](https://github.com/Orchestration-Maestro/rust-workflows/issues/77)) ([9fa5246](https://github.com/Orchestration-Maestro/rust-workflows/commit/9fa524663dc85f4ffea0c9ea88d5c30d478ffd4d))
* find this repository by its run, not its name ([#78](https://github.com/Orchestration-Maestro/rust-workflows/issues/78)) ([b6a3ceb](https://github.com/Orchestration-Maestro/rust-workflows/commit/b6a3ceb3da0927c711f023e711f881b1555b9c67))

## [4.1.0](https://github.com/Orchestration-Maestro/rust-workflows/compare/v4.0.0...v4.1.0) (2026-09-26)


### Features

* run the ci locally before a push ([#75](https://github.com/Orchestration-Maestro/rust-workflows/issues/75)) ([21124d2](https://github.com/Orchestration-Maestro/rust-workflows/commit/21124d28f8be1d94a1f1d73c9c6880334d4d9e1c))


### Bug Fixes

* judge a checkout reached through a symbolic link as itself ([#73](https://github.com/Orchestration-Maestro/rust-workflows/issues/73)) ([0f825f1](https://github.com/Orchestration-Maestro/rust-workflows/commit/0f825f1ec4776b1178142341e7fc357ac9939448))
* name the failed step when the scorecard runs before reports exist ([#71](https://github.com/Orchestration-Maestro/rust-workflows/issues/71)) ([4713066](https://github.com/Orchestration-Maestro/rust-workflows/commit/4713066bf75666da499492bb16460beae295fdae))
* retry a tool download for two minutes before it fails ([#72](https://github.com/Orchestration-Maestro/rust-workflows/issues/72)) ([48604c5](https://github.com/Orchestration-Maestro/rust-workflows/commit/48604c5a6e7677ed878302295fafff4b41960c0f))

## [4.0.0](https://github.com/Orchestration-Maestro/rust-workflows/compare/v3.0.1...v4.0.0) (2026-09-25)


### ⚠ BREAKING CHANGES

* a repository deletes its own toolbelt files, and each developer runs `rust-gate setup` and adds the PATH line it prints.
* upload-sarif.yml and upload-coverage.yml are removed. Every caller of ci.yml, publish-binaries.yml or publish-crate.yml grants `security-events: write` and `id-token: write`, which GitHub checks at startup for the skipped upload jobs; a release workflow's publisher job adds those two lines. Each repository's ci.yml caller is deleted by rust-gate sync.

### Features

* set up the pinned tools from the gate, not from each repository ([#70](https://github.com/Orchestration-Maestro/rust-workflows/issues/70)) ([0f0be71](https://github.com/Orchestration-Maestro/rust-workflows/commit/0f0be71a491a1c5a6944b7b2061eaf2db8fbdb07))
* upload sarif and coverage from the central check ([#67](https://github.com/Orchestration-Maestro/rust-workflows/issues/67)) ([63740a0](https://github.com/Orchestration-Maestro/rust-workflows/commit/63740a0ddd27be032760de2188e8a2135aa22da6))
* upload the default branch's baselines from the merge queue ([#69](https://github.com/Orchestration-Maestro/rust-workflows/issues/69)) ([79bc628](https://github.com/Orchestration-Maestro/rust-workflows/commit/79bc62850ca962a6b7a2f9ee7a8e710fc3868c96))

## [3.0.1](https://github.com/Orchestration-Maestro/rust-workflows/compare/v3.0.0...v3.0.1) (2026-09-25)


### Bug Fixes

* let a subdirectory's markdown settings apply ([#65](https://github.com/Orchestration-Maestro/rust-workflows/issues/65)) ([61b2d5e](https://github.com/Orchestration-Maestro/rust-workflows/commit/61b2d5ee1b01cfc566985a16ab32df081b7271d4))

## [3.0.0](https://github.com/Orchestration-Maestro/rust-workflows/compare/v2.5.1...v3.0.0) (2026-09-25)


### ⚠ BREAKING CHANGES

* pass the organization's tool settings at run time ([#63](https://github.com/Orchestration-Maestro/rust-workflows/issues/63))
* run ci.yml from the organization ruleset with its own gate ([#61](https://github.com/Orchestration-Maestro/rust-workflows/issues/61))

### Features

* pass the organization's tool settings at run time ([#63](https://github.com/Orchestration-Maestro/rust-workflows/issues/63)) ([43ddac4](https://github.com/Orchestration-Maestro/rust-workflows/commit/43ddac4783923a48672952f2a532d5e01a1ef7cb))
* run ci.yml from the organization ruleset with its own gate ([#61](https://github.com/Orchestration-Maestro/rust-workflows/issues/61)) ([75015a9](https://github.com/Orchestration-Maestro/rust-workflows/commit/75015a91f7fdbcf02bd84e669d61a6b7040af2c8))


### Bug Fixes

* install jaq before reading a repository's settings ([#64](https://github.com/Orchestration-Maestro/rust-workflows/issues/64)) ([5bbf614](https://github.com/Orchestration-Maestro/rust-workflows/commit/5bbf614a50ce10308e8ae4df7217aa9f4e0c727d))

## [2.5.1](https://github.com/Orchestration-Maestro/rust-workflows/compare/v2.5.0...v2.5.1) (2026-09-25)


### Bug Fixes

* pin the gate that ships this release's managed files ([#58](https://github.com/Orchestration-Maestro/rust-workflows/issues/58)) ([0c50f83](https://github.com/Orchestration-Maestro/rust-workflows/commit/0c50f832833c440abcc7d26f59a01802d2df10aa))

## [2.5.0](https://github.com/Orchestration-Maestro/rust-workflows/compare/v2.4.0...v2.5.0) (2026-09-25)


### Features

* refuse file names and words the organization does not use ([#53](https://github.com/Orchestration-Maestro/rust-workflows/issues/53)) ([241c9a6](https://github.com/Orchestration-Maestro/rust-workflows/commit/241c9a62e3b082136f050c5707ebe7992d93dd4a))
* refuse pull request titles and branches outside the conventions ([#54](https://github.com/Orchestration-Maestro/rust-workflows/issues/54)) ([3628906](https://github.com/Orchestration-Maestro/rust-workflows/commit/36289062e2567ce30ce274b190b18cddf32bb749))
* refuse rust names the organization does not use ([#52](https://github.com/Orchestration-Maestro/rust-workflows/issues/52)) ([fd96251](https://github.com/Orchestration-Maestro/rust-workflows/commit/fd9625125e78b2d33cf70eb471941e4bef749855))


### Bug Fixes

* let the home change the files it is the source of ([#56](https://github.com/Orchestration-Maestro/rust-workflows/issues/56)) ([c7f8f36](https://github.com/Orchestration-Maestro/rust-workflows/commit/c7f8f36be750870cc121a5719a499916e98e5c17))

## [2.4.0](https://github.com/Orchestration-Maestro/rust-workflows/compare/v2.3.0...v2.4.0) (2026-09-25)


### Features

* keep the rule map and copilot guide current at every commit ([#48](https://github.com/Orchestration-Maestro/rust-workflows/issues/48)) ([f54152a](https://github.com/Orchestration-Maestro/rust-workflows/commit/f54152a049c3e4645866283016e70c293a8260b7))

## [2.3.0](https://github.com/Orchestration-Maestro/rust-workflows/compare/v2.2.0...v2.3.0) (2026-09-25)


### Features

* write each repository's copilot guide with rust-gate guide ([#43](https://github.com/Orchestration-Maestro/rust-workflows/issues/43)) ([c3ff7af](https://github.com/Orchestration-Maestro/rust-workflows/commit/c3ff7af08637e862a69e5aa10623ced5515fe390))


### Bug Fixes

* carry the golden rules of .github 864d855 ([#46](https://github.com/Orchestration-Maestro/rust-workflows/issues/46)) ([843ecca](https://github.com/Orchestration-Maestro/rust-workflows/commit/843ecca383ac3b7f585e6516910cee96619a4219))
* say in each guide that the golden rules come first ([#47](https://github.com/Orchestration-Maestro/rust-workflows/issues/47)) ([d11e54c](https://github.com/Orchestration-Maestro/rust-workflows/commit/d11e54c9b99215f3721f0e49dc3c3fb71fc988f1))

## [2.2.0](https://github.com/Orchestration-Maestro/rust-workflows/compare/v2.1.0...v2.2.0) (2026-09-25)


### Features

* keep the gate's rules in one list it prints ([#44](https://github.com/Orchestration-Maestro/rust-workflows/issues/44)) ([ca71bbd](https://github.com/Orchestration-Maestro/rust-workflows/commit/ca71bbdda478dd38e8f75168a371fb0039bc54aa))
* write each repository's rule map with rust-gate rules ([#41](https://github.com/Orchestration-Maestro/rust-workflows/issues/41)) ([377cbaa](https://github.com/Orchestration-Maestro/rust-workflows/commit/377cbaa5b8f854032ee25d4f28ff48594daa3681))

## [2.1.0](https://github.com/Orchestration-Maestro/rust-workflows/compare/v2.0.1...v2.1.0) (2026-09-24)


### Features

* move every rust-workflows pin with the caller ([#39](https://github.com/Orchestration-Maestro/rust-workflows/issues/39)) ([bd7b23f](https://github.com/Orchestration-Maestro/rust-workflows/commit/bd7b23fac76cee34798e2e0eb9cce44a01a8b32f))

## [2.0.1](https://github.com/Orchestration-Maestro/rust-workflows/compare/v2.0.0...v2.0.1) (2026-09-24)


### Bug Fixes

* give the hooks step the mise bootstrap.sh pins ([#38](https://github.com/Orchestration-Maestro/rust-workflows/issues/38)) ([a88ec51](https://github.com/Orchestration-Maestro/rust-workflows/commit/a88ec510980675b6b9f315765d45911bcb2d70d0))
* skip dependabot auto-merge on runs a person started ([#36](https://github.com/Orchestration-Maestro/rust-workflows/issues/36)) ([57245f6](https://github.com/Orchestration-Maestro/rust-workflows/commit/57245f6e497fd724120fdf1be7f483e037bd21bb))

## [2.0.0](https://github.com/Orchestration-Maestro/rust-workflows/compare/v1.2.1...v2.0.0) (2026-09-24)


### ⚠ BREAKING CHANGES

* the quality-preview input is removed; a caller passing it fails to start.
* finish the organization quality gate behind quality-preview ([#32](https://github.com/Orchestration-Maestro/rust-workflows/issues/32))

### Features

* finish the organization quality gate behind quality-preview ([#32](https://github.com/Orchestration-Maestro/rust-workflows/issues/32)) ([deb5670](https://github.com/Orchestration-Maestro/rust-workflows/commit/deb56707c850e204c96f6a9af093769cdda9e1e5))
* refuse module structure faults with rust-gate architecture ([#30](https://github.com/Orchestration-Maestro/rust-workflows/issues/30)) ([cb65e7c](https://github.com/Orchestration-Maestro/rust-workflows/commit/cb65e7c228b5c6b5c1b58d26e271a1fee0567c7e))
* run every organization rule on every call ([#33](https://github.com/Orchestration-Maestro/rust-workflows/issues/33)) ([207cadb](https://github.com/Orchestration-Maestro/rust-workflows/commit/207cadbb6ee4b129cc8cc7b3e33e5af7bca424af))
* score every rule family and leave the home's hooks to just check ([#35](https://github.com/Orchestration-Maestro/rust-workflows/issues/35)) ([12f8b53](https://github.com/Orchestration-Maestro/rust-workflows/commit/12f8b536457153e7e1285ccee2d77eb3c804b3a6))


### Bug Fixes

* ship the gate's build inputs and authenticate hook installs ([#34](https://github.com/Orchestration-Maestro/rust-workflows/issues/34)) ([cc14131](https://github.com/Orchestration-Maestro/rust-workflows/commit/cc141317193572396c11bb1a130d88634a590314))

## [1.2.1](https://github.com/Orchestration-Maestro/rust-workflows/compare/v1.2.0...v1.2.1) (2026-09-23)


### Bug Fixes

* compare only the libraries the base branch already has ([#25](https://github.com/Orchestration-Maestro/rust-workflows/issues/25)) ([75807b0](https://github.com/Orchestration-Maestro/rust-workflows/commit/75807b0f9d6fc3f3389f43d3890342b23943b3cb))
* keep mise's progress out of the tool moves ([#20](https://github.com/Orchestration-Maestro/rust-workflows/issues/20)) ([8e22793](https://github.com/Orchestration-Maestro/rust-workflows/commit/8e22793345cb248bc2dd1d2408975c26cc148a3d))

## [1.2.0](https://github.com/Orchestration-Maestro/rust-workflows/compare/v1.1.0...v1.2.0) (2026-09-23)


### Features

* test on macOS, Windows and Linux arm64 when asked ([#16](https://github.com/Orchestration-Maestro/rust-workflows/issues/16)) ([6290dc0](https://github.com/Orchestration-Maestro/rust-workflows/commit/6290dc03b70dc67ba024eeef1c23b37e9d927182))

## [1.1.0](https://github.com/Orchestration-Maestro/rust-workflows/compare/v1.0.1...v1.1.0) (2026-09-23)


### Features

* check public API compatibility on pull requests ([#14](https://github.com/Orchestration-Maestro/rust-workflows/issues/14)) ([a064dae](https://github.com/Orchestration-Maestro/rust-workflows/commit/a064dae4efbc2826baf0fb5513cd07eaf1361911))
* turn the API compatibility gate on by default ([#15](https://github.com/Orchestration-Maestro/rust-workflows/issues/15)) ([9deeaea](https://github.com/Orchestration-Maestro/rust-workflows/commit/9deeaead8f795fed0c81d4f0d312b380b23652bc))
* upload coverage and test results to Codecov ([#12](https://github.com/Orchestration-Maestro/rust-workflows/issues/12)) ([7528afc](https://github.com/Orchestration-Maestro/rust-workflows/commit/7528afc09e366456b442126803d4619e3d951281))

## [1.0.1](https://github.com/Orchestration-Maestro/rust-workflows/compare/v1.0.0...v1.0.1) (2026-09-23)


### Bug Fixes

* retry a dropped connection before a tool download fails ([#7](https://github.com/Orchestration-Maestro/rust-workflows/issues/7)) ([beda4ef](https://github.com/Orchestration-Maestro/rust-workflows/commit/beda4ef938e575971c99c93d683e7b884402462b))

## 1.0.0 (2026-09-23)

### Features

* reusable, security-gated Rust workflows ([4156864](https://github.com/Orchestration-Maestro/rust-workflows/commit/4156864bd3b60dce57995ce4039d0cd95d5c7dd9))
* turn SARIF on by default and upload it to code scanning ([#4](https://github.com/Orchestration-Maestro/rust-workflows/issues/4)) ([701b4dc](https://github.com/Orchestration-Maestro/rust-workflows/commit/701b4dca765deb4d4bec2b5cda04051eba219118))

### Bug Fixes

* package workspaces on Cargo 1.90 below that compiler ([#2](https://github.com/Orchestration-Maestro/rust-workflows/issues/2)) ([eabfa54](https://github.com/Orchestration-Maestro/rust-workflows/commit/eabfa547d40f60f6ee7cd040bc8ffbbf077aacbc))
