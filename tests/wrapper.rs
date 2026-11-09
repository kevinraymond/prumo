//! `docker/prumo`, the command that runs prumo from the image: what it asks Docker for.
//! `PRUMO_DOCKER=echo` makes it print the `docker run` line instead of running it.

#![cfg(unix)]

use std::process::Command;

fn run(dir: &std::path::Path, envs: &[(&str, &str)], args: &[&str]) -> String {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/docker/prumo");
    let mut cmd = Command::new("sh");
    cmd.arg(script).args(args).current_dir(dir).env("PRUMO_DOCKER", "echo");
    cmd.env_remove("PRUMO_IMAGE").env_remove("PRUMO_CACHE").env_remove("PRUMO_DOCKER_OPTS");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("sh runs the wrapper");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn wrapper_mounts_the_directory_and_the_cache_at_their_own_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let doc = tmp.path().join("thesis");
    let cache = tmp.path().join("cache");
    std::fs::create_dir(&doc).unwrap();
    let doc = doc.canonicalize().unwrap();
    let cache_s = cache.to_str().unwrap();
    let line = run(&doc, &[("PRUMO_CACHE", cache_s), ("PRUMO_IMAGE", "prumo-test")], &["time", "main.tex", "--html"]);
    let d = doc.display();
    assert!(line.starts_with("run --rm --user "), "{line}");
    assert!(line.contains(&format!(" -v {d}:{d} -w {d} ")), "{line}");
    assert!(line.contains(&format!(" -v {cache_s}:{cache_s} -e PRUMO_CACHE={cache_s} ")), "{line}");
    assert!(line.contains(&format!(" -e PRUMO_PATH_MAP={d}={d};{cache_s}={cache_s} ")), "{line}");
    assert!(
        line.contains(" -e PRUMO_IMAGE=prumo-test -e TERM -e NO_COLOR -e COLUMNS prumo-test time main.tex --html\n"),
        "{line}"
    );
    assert!(!line.contains(" -t "), "stdout is a pipe here, so no terminal: {line}");
    assert!(cache.is_dir(), "the cache directory is created so Docker does not create it root-owned");

    // The default image and the extra options.
    let line = run(&doc, &[("PRUMO_CACHE", cache_s), ("PRUMO_DOCKER_OPTS", "--platform linux/amd64")], &["doctor"]);
    assert!(
        line.contains(" --platform linux/amd64 ") && line.ends_with(" ghcr.io/kevinraymond/prumo doctor\n"),
        "{line}"
    );
}

#[test]
fn windows_wrapper_takes_the_same_knobs() {
    // prumo.cmd cannot run here; keep its contract in step with the sh script's.
    let cmd = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docker/prumo.cmd")).unwrap();
    let sh = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docker/prumo")).unwrap();
    for knob in [
        "PRUMO_IMAGE",
        "PRUMO_CACHE",
        "PRUMO_DOCKER",
        "PRUMO_DOCKER_OPTS",
        "PRUMO_PATH_MAP",
        "ghcr.io/kevinraymond/prumo",
    ] {
        assert!(cmd.contains(knob) && sh.contains(knob), "{knob}");
    }
    assert!(cmd.contains("%LOCALAPPDATA%\\prumo") && cmd.contains("IsOutputRedirected"));
}
