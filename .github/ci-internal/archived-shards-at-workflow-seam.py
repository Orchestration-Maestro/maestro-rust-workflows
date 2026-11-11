import seam_runner

seam = seam_runner.load('archive workflow seam')
seam['archived_shards_reuse_build_and_prepared_outputs']()
print('PASS archived_shards_reuse_build_and_prepared_outputs', flush=True)
seam['per_shard_archives_run_every_test_once']()
print('PASS per_shard_archives_run_every_test_once', flush=True)
