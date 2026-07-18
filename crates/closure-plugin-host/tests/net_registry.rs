//! Q8-P2: network registry fetch — curl shell-out against a loopback
//! server (hermetic sockets; skips gracefully when curl is absent).
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::io::{Read as _, Write as _};
use std::net::TcpListener;

fn curl_available() -> bool {
    std::process::Command::new("curl")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn serve_once(listener: TcpListener, body: &'static str) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf);
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        }
    })
}

const PKG: &str = "#+BEGIN_SRC closure-package\nname = remote-pack\nversion = 3.0.0\n#+END_SRC\n";

#[test]
fn fetch_package_downloads_and_validates() {
    if !curl_available() {
        eprintln!("skipped: no curl on this host");
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = serve_once(listener, PKG);
    let dir = tempfile::tempdir().expect("tmp");
    let dest = dir.path().join("remote-pack.org");
    closure_plugin_host::fetch_package(&format!("http://{addr}/remote-pack.org"), &dest)
        .expect("fetch");
    handle.join().expect("join");
    let packages = closure_plugin_host::load_packages(dir.path()).expect("load");
    assert!(packages.contains_key("remote-pack"), "{packages:?}");
}

#[test]
fn fetch_rejects_non_package_payloads() {
    if !curl_available() {
        eprintln!("skipped: no curl on this host");
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = serve_once(listener, "<html>not a package</html>");
    let dir = tempfile::tempdir().expect("tmp");
    let dest = dir.path().join("bad.org");
    let err = closure_plugin_host::fetch_package(&format!("http://{addr}/x"), &dest);
    handle.join().expect("join");
    assert!(err.is_err(), "non-package payload rejected");
    assert!(!dest.exists(), "rejected file removed");
}
