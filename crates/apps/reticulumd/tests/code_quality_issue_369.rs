use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn production_code_does_not_silently_discard_issue_369_failures() {
    let root = workspace_root();
    let mut findings = Vec::new();
    visit_rs_files(&root.join("crates"), &mut |path| {
        let display = path.strip_prefix(&root).unwrap_or(path);
        if !is_production_source(display) {
            return;
        }
        let source = fs::read_to_string(path).expect("read Rust source");
        let display = display.display();
        for line_no in multiline_lock_ok_lines(&source) {
            findings.push(format!("{display}:{line_no}: mutex lock poison is discarded"));
        }
        for line_no in multiline_send_ok_lines(&source) {
            findings.push(format!("{display}:{line_no}: channel send failure is discarded"));
        }
        for line_no in ignored_result_lines(&source, ".send(") {
            findings.push(format!("{display}:{line_no}: channel send result is ignored"));
        }
        for line_no in ignored_result_lines(&source, ".execute(")
            .into_iter()
            .chain(ignored_result_lines(&source, ".execute_batch("))
        {
            findings.push(format!("{display}:{line_no}: database operation result is ignored"));
        }
        for line_no in suppressed_result_lines(&source, "self.store") {
            findings.push(format!("{display}:{line_no}: persistent-store error is discarded"));
        }
        for line_no in suppressed_result_lines(&source, "message_receipt_status") {
            findings.push(format!("{display}:{line_no}: receipt-status error is discarded"));
        }
        for (line, text) in source.lines().enumerate() {
            let line_no = line + 1;
            if text.contains(".lock().ok()") {
                findings.push(format!("{display}:{line_no}: mutex lock poison is discarded"));
            }
            if text.contains("if let Ok(") && text.contains(".lock()") {
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
            if text.contains("VerifyingKey::from_bytes") && text.contains("unwrap_or_default()") {
                findings.push(format!(
                    "{display}:{line_no}: invalid verifying key is replaced with a default"
                ));
            }
            if text.contains("u8::from_str_radix") && text.contains(".unwrap()") {
                findings.push(format!("{display}:{line_no}: malformed hex input can panic"));
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

fn suppressed_result_lines(source: &str, operation: &str) -> Vec<usize> {
    let mut findings = Vec::new();
    let mut pending_line = None;
    let mut expression = String::new();

    for (index, text) in source.lines().enumerate() {
        let line_no = index + 1;
        if pending_line.is_none() {
            if !text.contains(operation) {
                continue;
            }
            pending_line = Some(line_no);
        }
        expression.push_str(text);

        let suppresses_error = expression.contains(".ok()")
            || expression.contains("unwrap_or_default()")
            || expression.contains("unwrap_or(false)");
        if suppresses_error {
            findings.push(pending_line.expect("pending result suppression line"));
            pending_line = None;
            expression.clear();
        } else if expression.contains(';')
            || line_no.saturating_sub(pending_line.unwrap_or(line_no)) > 40
        {
            pending_line = None;
            expression.clear();
        }
    }

    findings
}

fn multiline_send_ok_lines(source: &str) -> Vec<usize> {
    let mut findings = Vec::new();
    let mut pending_send = None;

    for (index, text) in source.lines().enumerate() {
        let line_no = index + 1;
        if pending_send.is_none() && text.contains(".send(") {
            pending_send = Some(line_no);
        }
        if pending_send.is_some() {
            if text.contains(".ok()") {
                findings.push(pending_send.expect("pending send line"));
                pending_send = None;
            } else if text.contains(';')
                || line_no.saturating_sub(pending_send.unwrap_or(line_no)) > 40
            {
                pending_send = None;
            }
        }
    }

    findings
}

fn ignored_result_lines(source: &str, operation: &str) -> Vec<usize> {
    let mut findings = Vec::new();
    let mut pending_line = None;
    let mut statement = String::new();

    for (index, text) in source.lines().enumerate() {
        let line_no = index + 1;
        if pending_line.is_none() {
            let Some(offset) = text.find("let _ =") else {
                continue;
            };
            pending_line = Some(line_no);
            statement.push_str(&text[offset..]);
        } else {
            statement.push_str(text);
        }

        if statement.contains(';') {
            if statement.contains(operation) {
                findings.push(pending_line.expect("pending ignored-result line"));
            }
            pending_line = None;
            statement.clear();
        } else if line_no.saturating_sub(pending_line.unwrap_or(line_no)) > 40 {
            pending_line = None;
            statement.clear();
        }
    }

    findings
}

fn multiline_lock_ok_lines(source: &str) -> Vec<usize> {
    let mut findings = Vec::new();
    let mut pending_lock_line = None;
    for (index, text) in source.lines().enumerate() {
        let line_no = index + 1;
        if text.contains(".lock()") || text.contains(".try_lock()") {
            if !text.contains(".expect(") && !text.contains(".ok()") {
                pending_lock_line = Some(line_no);
            }
            continue;
        }
        if let Some(lock_line) = pending_lock_line {
            if text.contains(".ok()") {
                findings.push(lock_line);
                pending_lock_line = None;
                continue;
            }
            if text.contains(';') || text.contains(".expect(") || text.contains("match ") {
                pending_lock_line = None;
            }
        }
    }
    findings
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
        && !text.contains("crates/libs/test-support/")
        && !text.contains("/src/tests")
        && !text.contains("/tests_parts/")
}
