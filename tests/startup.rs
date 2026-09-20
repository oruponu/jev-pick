use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "jev-pick-startup-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        // Stop dotenv's parent-directory search without changing the process environment.
        fs::write(path.join(".env"), "").unwrap();
        Self(path)
    }

    fn run(&self, values: &[(&str, &str)]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_jev-pick"))
            .current_dir(&self.0)
            .env_clear()
            .envs(values.iter().copied())
            .output()
            .unwrap()
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn missing_configuration_exits_unsuccessfully_before_connecting() {
    let directory = TestDirectory::new();
    let output = directory.run(&[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("DISCORD_TOKEN"));
}

#[test]
fn malformed_dotenv_fails_without_printing_its_content() {
    let directory = TestDirectory::new();
    fs::write(directory.0.join(".env"), "DISCORD_TOKEN='sensitive-marker").unwrap();
    let output = directory.run(&[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("malformed"));
    assert!(!stderr.contains("sensitive-marker"));
}
