use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub fn tm_bin() -> PathBuf {
    match std::env::var_os("TM_BIN") {
        None => PathBuf::from(env!("CARGO_BIN_EXE_tm")),
        Some(value) => {
            let path = PathBuf::from(value);
            assert!(path.is_absolute(), "TM_BIN must be absolute: {}", path.display());
            let metadata = std::fs::metadata(&path)
                .unwrap_or_else(|error| panic!("TM_BIN {}: {error}", path.display()));
            assert!(
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
                "TM_BIN is not an executable file: {}",
                path.display()
            );
            path
        }
    }
}
