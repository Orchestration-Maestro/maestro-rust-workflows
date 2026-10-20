# Changelog

## [0.11.2](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.11.1...v0.11.2) (2026-10-20)


### Bug Fixes

* retain timed-out shard diagnostics ([#100](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/100)) ([1c3cee5](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/1c3cee5be34ad38540af093000d052879e9f5f55))

## [0.11.1](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.11.0...v0.11.1) (2026-10-17)


### Bug Fixes

* accept exact advisory exceptions for multiple versions ([#88](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/88)) ([f597f0c](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/f597f0c684520f3c0642fa0c11f0f324425d2b2e))
* **ci:** provide caller tools before native compilation ([#89](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/89)) ([59ed124](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/59ed124385020db98e2760d72de00e23b3bb040f))

## [0.11.0](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.10.0...v0.11.0) (2026-10-17)


### Features

* **ci:** gate advisories on dependency changes and scan main ([#75](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/75)) ([9832a4a](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/9832a4a1f8aacc51b42e7fc5008251fed24fb385))


### Bug Fixes

* advance the coverage proof snapshot past the zip advisories ([#79](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/79)) ([a7c00bc](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/a7c00bc4848f7a9b949c2abbc760d8af035f1de7))
* **ci:** preserve test toolchains and shard failures ([#85](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/85)) ([28fd419](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/28fd4195ef75c0e4ffc15ef63535865078e42f98))


### Performance

* **ci:** free instrumented build space in the background ([#77](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/77)) ([1c55da2](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/1c55da26302b04ccc5736567894d421e524708e1)), closes [#76](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/76)

## [0.10.0](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.9.0...v0.10.0) (2026-10-16)


### Features

* **ci:** provision additional locked manifests ([#68](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/68)) ([1021c93](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/1021c93e64f5dad585c578193f2471cb02bc11f4))


### Bug Fixes

* advance the coverage proof snapshot ([#71](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/71)) ([da85a43](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/da85a43c862325a24a49b33fea2f5763edd78e0e))

## [0.9.0](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.8.0...v0.9.0) (2026-10-16)


### Features

* **ci:** allow exact yanked-crate exceptions ([#64](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/64)) ([3fe29f8](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/3fe29f8589817a607f674e14aaa6f9f0c1842203))

## [0.8.0](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.7.4...v0.8.0) (2026-10-14)


### Features

* reserve caller-declared ports on Linux test runners ([#58](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/58)) ([332b0c7](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/332b0c78368951b0266a63bbf3fcb8318e1d45a2))


### Bug Fixes

* verify the hakari package in shared CI ([#56](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/56)) ([4029356](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/40293560fd66b5a8701e4e3c5d9f3a17bd64dd36)), closes [#55](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/55)

## [0.7.4](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.7.3...v0.7.4) (2026-10-14)


### Continuous Integration

* list performance changes in the release notes ([#53](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/53)) ([32e7039](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/32e703997ad7b2b86360c27d82645f508db281e6))


### Performance

* cache compiled crates with sccache in CI ([#47](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/47)) ([be0b10d](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/be0b10dc3e0a179538c4addc25fcaf84ce48ad77))
* collect coverage in automatic nextest partitions ([#40](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/40)) ([2417378](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/2417378e6a6161a3ab86cf03d73db26ecd3b37e5))

## [0.7.3](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.7.2...v0.7.3) (2026-10-14)


### Bug Fixes

* let cargo-machete accept a declared hakari package ([#50](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/50)) ([0e920c8](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/0e920c81ad533d4bef8d4d2c6aea0b758d1585fc)), closes [#49](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/49)

## [0.7.2](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.7.1...v0.7.2) (2026-10-14)


### Bug Fixes

* scan source secrets before restoring coverage output ([#42](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/42)) ([0f205a4](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/0f205a4f5606f2c2f0868184ce9cf804fe4d9bc2)), closes [#41](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/41)

## [0.7.1](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.7.0...v0.7.1) (2026-10-14)


### Bug Fixes

* **ci:** fetch every dependency before a test shard ([#37](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/37)) ([a3aa454](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/a3aa454fbe540ed8bb7e6f05394ef61a18ea47d7)), closes [#36](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/36)

## [0.7.0](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.6.1...v0.7.0) (2026-10-14)


### Features

* **ci:** cache coverage builds and shard workspace tests ([#34](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/34)) ([b115883](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/b1158831bc620e22bcf52098a3c8e246333d081b)), closes [#33](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/33)

## [0.6.1](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.6.0...v0.6.1) (2026-10-11)


### Bug Fixes

* **ci:** bound caller test and coverage steps to 45 minutes ([#31](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/31)) ([4463766](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/44637664f36e6357ed1a3ddbab62874a88876925))

## [0.6.0](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.5.1...v0.6.0) (2026-10-10)


### Features

* accept exact advisory exceptions in the caller declaration ([#28](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/28)) ([768a8e1](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/768a8e1b22d2e1edfa64c1606d235bb38cd915bf))

## [0.5.1](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.5.0...v0.5.1) (2026-10-07)


### Bug Fixes

* **ci:** install search tools from verified releases ([#25](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/25)) ([35943b9](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/35943b9f3d0a25fcedfcfbb058c9898b69b5ca8a))

## [0.5.0](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.4.0...v0.5.0) (2026-10-07)


### Features

* **ci:** add supply-chain, spelling, secret, workflow, coverage checks ([#20](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/20)) ([a00bfed](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/a00bfed5cbc17ecf716c703ac2e285155e96b471))
* **ci:** allow declared privileged-trigger workflows ([#23](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/23)) ([a7c5878](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/a7c58784582aebfcb30de22bb5aea889f223f3fa)), closes [#22](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/22)

## [0.4.0](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.3.0...v0.4.0) (2026-10-06)


### Features

* **ci:** expose caller-pinned tools to isolated tests ([#17](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/17)) ([e4bfbd6](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/e4bfbd6de8e19bdd30bc1a2c223b61c3fceaaf02)), closes [#16](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/16)

## [0.3.0](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.2.1...v0.3.0) (2026-10-06)


### Features

* **ci:** run caller-declared isolated and target checks ([#12](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/12)) ([b85d671](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/b85d67123cc4be1b29acf58b66aea1a129a0f2fe))

## [0.2.1](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.2.0...v0.2.1) (2026-10-05)


### Bug Fixes

* cancel a pull request's older run on a new push ([#8](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/8)) ([2918f61](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/2918f613fcd28b83c32f30e3411cf20fb9741262))

## [0.2.0](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.1.1...v0.2.0) (2026-10-05)


### Features

* enforce public documentation and run tests on Linux ([#6](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/6)) ([d5b030b](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/d5b030bb9c8dbb7b251075d17f3bdcf98e8e6753))

## [0.1.1](https://github.com/Orchestration-Maestro/maestro-rust-workflows/compare/v0.1.0...v0.1.1) (2026-10-04)


### Continuous Integration

* cache cargo builds and attest release assets ([#3](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/3)) ([d8e07d8](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/d8e07d817eff59dab47760e008a91d780f2701c9))

## 0.1.0 (2026-10-04)


### Continuous Integration

* prove the shared Rust workflow with consumer fixtures ([#1](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/1)) ([848e72b](https://github.com/Orchestration-Maestro/maestro-rust-workflows/commit/848e72bab2b159518894b6e3cc7388bfb050fa1c))
