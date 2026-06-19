use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn production_code_does_not_silently_discard_issue_369_failures() {
    let root = workspace_root();
    let mut findings = Vec::new();
    visit_rs_files(&root.join("crates"), &mut |path| {
        if !is_production_source(path) {
            return;
        }
        let source = fs::read_to_string(path).expect("read Rust source");
        let display = path.strip_prefix(&root).unwrap_or(path).display();
        for (line, text) in source.lines().enumerate() {
            let line_no = line + 1;
            if text.contains(".lock().ok()") {
                findings.push(format!("{display}:{line_no}: mutex lock poison is discarded"));
            }
            if text.contains("let _ = Hkdf::<Sha256>::new")
                || text.contains(".expand(&[], &mut okm).ok()?")
            {
                findings.push(format!("{display}:{line_no}: HKDF failure is discarded"));
            }
            if text.contains("encode_frame(&response).unwrap_or_default()") {
                findings.push(format!("{display}:{line_no}: RPC encode failure sends empty frame"));
            }
            if text.contains("let _ =") && text.contains(".try_send(") {
                findings.push(format!("{display}:{line_no}: channel send failure is discarded"));
            }
            if text.contains("let _ =") && text.contains("rx_channel.send(") {
                findings
                    .push(format!("{display}:{line_no}: interface RX send failure is discarded"));
            }
            if text.contains("let _ = stream.write_all(&response).await") {
                findings
                    .push(format!("{display}:{line_no}: RPC response write failure is discarded"));
            }
            if text.contains("String::from_utf8") && text.contains(".ok()") {
                findings
                    .push(format!("{display}:{line_no}: UTF-8 error is conflated with absence"));
            }
            if text.contains("std::str::from_utf8") && text.contains(".ok()") {
                findings
                    .push(format!("{display}:{line_no}: UTF-8 error is conflated with absence"));
            }
            if text.contains("core::str::from_utf8") && text.contains(".ok()") {
                findings
                    .push(format!("{display}:{line_no}: UTF-8 error is conflated with absence"));
            }
            if text.contains("rmp_serde::to_vec") && text.contains(".ok()") {
                findings.push(format!("{display}:{line_no}: msgpack encode error is discarded"));
            }
            if text.contains("rmp_serde::from_slice") && text.contains(".ok()") {
                findings.push(format!("{display}:{line_no}: msgpack decode error is discarded"));
            }
        }
    });

    findings.sort();
    assert!(
        findings.is_empty(),
        "issue #369 silent failure patterns remain:\n{}",
        findings.join("\n")
    );
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn visit_rs_files(dir: &Path, f: &mut impl FnMut(&Path)) {
    let entries = fs::read_dir(dir).expect("read source directory");
    for entry in entries {
        let path = entry.expect("read directory entry").path();
        if path.is_dir() {
            visit_rs_files(&path, f);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            f(&path);
        }
    }
}

fn is_production_source(path: &Path) -> bool {
    let text = path.to_string_lossy().replace('\\', "/");
    text.contains("/src/")
        && !text.contains("/crates/libs/test-support/")
        && !text.contains("/src/tests")
        && !text.contains("/tests_parts/")
}
