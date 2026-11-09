//! A throwaway MySQL/MariaDB server for tests: `STOKE_TEST_MYSQLD` names a mysqld or mariadbd.

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A throwaway MySQL or MariaDB server: `STOKE_TEST_MYSQLD` names its `mysqld`/`mariadbd`, the
/// install tool and `mysql` client sit next to it. Free port, temp data folder, stopped at the end.
pub(crate) struct TestServer {
    child: std::process::Child,
    pub port: u16,
    bin: PathBuf,
    _data: tempfile::TempDir,
}

impl TestServer {
    pub fn start() -> Option<TestServer> {
        let server = PathBuf::from(std::env::var_os("STOKE_TEST_MYSQLD")?);
        let bin = server.parent()?.to_path_buf();
        let data = tempfile::tempdir().ok()?;
        let exe = std::env::consts::EXE_SUFFIX;
        let install = ["mariadb-install-db", "mysql_install_db"]
            .iter()
            .map(|n| bin.join(format!("{n}{exe}")))
            .find(|p| p.is_file())?;
        let ok = std::process::Command::new(install)
            .arg(format!("--datadir={}", data.path().join("d").display()))
            .output()
            .ok()?
            .status
            .success();
        assert!(ok, "the test server's data folder could not be set up");
        let port = std::net::TcpListener::bind("127.0.0.1:0").ok()?.local_addr().ok()?.port();
        let child = std::process::Command::new(&server)
            .args(["--no-defaults", "--console", "--bind-address=127.0.0.1"])
            .arg(format!("--datadir={}", data.path().join("d").display()))
            .arg(format!("--port={port}"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "the test server did not start");
            std::thread::sleep(Duration::from_millis(200));
        }
        Some(TestServer { child, port, bin, _data: data })
    }

    pub fn sql(&self, sql: &str) -> String {
        let out = std::process::Command::new(self.bin.join(format!("mysql{}", std::env::consts::EXE_SUFFIX)))
            .args(["-h", "127.0.0.1", "-u", "root", "-N", "-B"])
            .arg(format!("-P{}", self.port))
            .args(["-e", sql])
            .output()
            .unwrap();
        assert!(out.status.success(), "{sql}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
