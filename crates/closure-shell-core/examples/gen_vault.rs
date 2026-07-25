//! Write a deterministic generated vault to a directory, so the GUI
//! shells can be exercised against something big.
//!
//! `cargo run -p closure-shell-core --example gen_vault -- DIR FILES HEADLINES [SEED]`
//!
//! Same generator the latency harness uses ([`closure_shell_core::gen_vault`]),
//! so a shell driven over this vault and the numbers `just bench`
//! prints are describing the same content.

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next() else {
        eprintln!("usage: gen_vault DIR FILES HEADLINES [SEED]");
        std::process::exit(2);
    };
    let parse =
        |a: Option<String>, default: usize| a.and_then(|v| v.parse().ok()).unwrap_or(default);
    let files = parse(args.next(), 200);
    let heads = parse(args.next(), 60);
    let seed = parse(args.next(), 1) as u64;

    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir)?;
    for (name, content) in closure_shell_core::gen_vault(files, heads, seed) {
        std::fs::write(dir.join(name), content)?;
    }
    println!(
        "wrote {files} files x {heads} headlines ({} total) to {}",
        files * heads,
        dir.display()
    );
    Ok(())
}
