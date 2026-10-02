//! Quality configuration cases kept beside the check to preserve its size limit.

#[cfg(test)]
mod configuration_cases {
    use crate::checks::quality_config::read_config;
    use std::{env, fs, process};

    #[test]
    fn native_cache_tables_are_validated_when_reading_quality_configuration() {
        let directory = env::temp_dir().join(format!("quality-native-{}", process::id()));
        fs::create_dir(&directory).unwrap();
        fs::write(
            directory.join("maestro-quality.toml"),
            "[native-cache]\nextra = true\n",
        )
        .unwrap();
        assert_eq!(
            read_config(&directory).err().unwrap().message.as_deref(),
            Some(
                "[native-cache] must contain only environment, platforms, key-files and published"
            )
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
