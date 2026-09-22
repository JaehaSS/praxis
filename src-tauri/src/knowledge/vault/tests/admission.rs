use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

use sqlx::sqlite::SqlitePoolOptions;

#[tokio::test]
async fn exclusive_lock_blocks_a_second_file_backed_pool() {
    let path = crate::testtmp::dir().join(format!("vault-admission-{}.sqlite", std::process::id()));
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let first = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let second = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let guard = crate::knowledge::vault::exclusive_admission(&first)
        .await
        .unwrap();
    let blocked = tokio::time::timeout(
        Duration::from_millis(50),
        crate::knowledge::vault::shared_admission(&second),
    )
    .await;
    assert!(blocked.is_err());
    drop(guard);
    assert!(tokio::time::timeout(
        Duration::from_secs(1),
        crate::knowledge::vault::shared_admission(&second)
    )
    .await
    .unwrap()
    .is_ok());
}

#[tokio::test]
async fn killed_process_releases_the_file_backed_lock() {
    let path = crate::testtmp::dir().join(format!(
        "vault-admission-crash-{}.sqlite",
        std::process::id()
    ));
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let lock = path.parent().unwrap().join(format!(
        ".{}.vault-admission.lock",
        path.file_name().unwrap().to_string_lossy()
    ));
    let shared = crate::knowledge::vault::shared_admission(&pool)
        .await
        .unwrap();
    let mut child = Command::new("python3")
        .args(["-c", "import fcntl,sys,time; f=open(sys.argv[1],'a+'); fcntl.flock(f,fcntl.LOCK_EX); print('READY',flush=True); time.sleep(60)", lock.to_str().unwrap()])
        .stdout(Stdio::piped()).spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut ready = tokio::task::spawn_blocking(move || {
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line).map(|_| line)
    });
    assert!(tokio::time::timeout(Duration::from_millis(50), &mut ready)
        .await
        .is_err());
    drop(shared);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), &mut ready)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .trim(),
        "READY"
    );
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(tokio::time::timeout(
        Duration::from_secs(1),
        crate::knowledge::vault::exclusive_admission(&pool)
    )
    .await
    .unwrap()
    .is_ok());
}
