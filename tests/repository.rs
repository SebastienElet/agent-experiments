use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn trial(root: &Path, oracle: &str, provider: &str, name: &str) -> Result<Value> {
    let bin = root.join("bin");
    fs::create_dir_all(&bin)?;
    let codex = bin.join("codex");
    fs::write(
        &codex,
        format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo fixture-codex; exit; fi\n{provider}\n"
        ),
    )?;
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o755))?;
    fs::write(root.join("instructions.md"), "Synthetic instruction.\n")?;
    fs::write(
        root.join("case.json"),
        serde_json::to_vec(&json!({
            "id": "example", "prompt": "Implement the answer.",
            "files": {"input.txt": "fixture"}, "setup": "true\n",
            "oracle": {"run.sh": oracle}
        }))?,
    )?;
    let output = Command::new(env!("CARGO_BIN_EXE_agent-experiments"))
        .args(["repository-run", "--case"])
        .arg(root.join("case.json"))
        .arg("--instructions")
        .arg(root.join("instructions.md"))
        .args(["--model", "fixture-model", "--report"])
        .arg(root.join(name))
        .args([
            "--timeout-seconds",
            "1",
            "--tool-path",
            "/usr/bin:/bin",
            "--docker-host",
            "tcp://127.0.0.1:65534",
        ])
        .env_clear()
        .env("HOME", root)
        .env("CODEX_API_KEY", "synthetic-test-key")
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()?;
    let path = root.join(name).join("report.json");
    assert!(path.exists(), "{}", String::from_utf8_lossy(&output.stderr));
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

const PROVIDER: &str = "cat >/dev/null\nprintf '42' > answer.txt\nprintf '%s\\n' '{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1}}'";

#[test]
fn retains_edits_and_scores_a_real_oracle_without_exposing_credentials() -> Result {
    let root = tempfile::tempdir()?;
    let report = trial(
        root.path(),
        "test \"$(cat answer.txt)\" = 42 || exit 10\necho observed-answer\n",
        PROVIDER,
        "run",
    )?;
    assert_eq!(report.get("status"), Some(&json!("PASS")));
    assert_eq!(report.get("model"), Some(&json!("fixture-model")));
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(root.path().join("run/result.json"))?)?
            .get("answer.txt"),
        Some(&json!("42"))
    );
    assert!(fs::read_to_string(root.path().join("run/oracle.log"))?.contains("observed-answer"));
    assert!(!serde_json::to_string(&report)?.contains("synthetic-test-key"));
    Ok(())
}

#[test]
fn distinguishes_assertion_failure_from_broken_oracle_and_broken_agent() -> Result {
    let root = tempfile::tempdir()?;
    assert_eq!(
        trial(root.path(), "exit 10\n", PROVIDER, "fault")?.get("status"),
        Some(&json!("FAIL"))
    );
    assert_eq!(
        trial(root.path(), "exit 12\n", PROVIDER, "broken")?.get("status"),
        Some(&json!("INVALID"))
    );
    assert_eq!(
        trial(root.path(), "true\n", "exit 3", "agent")?.get("status"),
        Some(&json!("INVALID"))
    );
    assert_eq!(
        trial(root.path(), "true\n", "sleep 3", "timeout")?.get("status"),
        Some(&json!("INVALID"))
    );
    Ok(())
}

#[test]
fn comparison_rejects_changed_inputs_and_tampered_evidence() -> Result {
    let root = tempfile::tempdir()?;
    trial(root.path(), "exit 10\n", PROVIDER, "one")?;
    trial(root.path(), "true\n", PROVIDER, "two")?;
    let compare = |left: &str, right: &str| {
        Command::new(env!("CARGO_BIN_EXE_agent-experiments"))
            .arg("repository-compare")
            .arg(root.path().join(left))
            .arg(root.path().join(right))
            .output()
    };
    assert!(!compare("one", "two")?.status.success());
    fs::write(root.path().join("one/result.json"), "tampered")?;
    assert!(!compare("one", "one")?.status.success());
    Ok(())
}

#[test]
fn refuses_to_overwrite_evidence() -> Result {
    let root = tempfile::tempdir()?;
    trial(root.path(), "true\n", PROVIDER, "run")?;
    let before = fs::read(root.path().join("run/report.json"))?;
    trial(root.path(), "exit 10\n", PROVIDER, "run")?;
    assert_eq!(before, fs::read(root.path().join("run/report.json"))?);
    Ok(())
}

#[test]
fn oracle_edits_are_separate_from_agent_edits_and_identical_instructions_show_no_gain() -> Result {
    let root = tempfile::tempdir()?;
    trial(root.path(), "echo 99 > answer.txt\n", PROVIDER, "before")?;
    let before: Value = serde_json::from_slice(&fs::read(root.path().join("before/result.json"))?)?;
    let after: Value =
        serde_json::from_slice(&fs::read(root.path().join("before/verified.json"))?)?;
    assert_eq!(before.get("answer.txt"), Some(&json!("42")));
    assert_eq!(after.get("answer.txt"), Some(&json!("99\n")));
    let output = Command::new(env!("CARGO_BIN_EXE_agent-experiments"))
        .arg("repository-compare")
        .arg(root.path().join("before"))
        .arg(root.path().join("before"))
        .output()?;
    let comparison: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(comparison.get("observed_gain"), Some(&json!(false)));
    Ok(())
}

#[test]
fn preserves_executable_modes_and_requires_an_instruction_change_for_gain() -> Result {
    let root = tempfile::tempdir()?;
    let provider =
        format!("{PROVIDER}\nprintf '#!/bin/sh\\nexit 0\\n' > check.sh\nchmod +x check.sh");
    trial(root.path(), "./check.sh\n", &provider, "modes")?;
    let modes: Value =
        serde_json::from_slice(&fs::read(root.path().join("modes/result-modes.json"))?)?;
    assert_eq!(
        modes
            .get("check.sh")
            .and_then(Value::as_u64)
            .map(|mode| mode & 0o111),
        Some(0o111)
    );
    let oracle = "test \"$(cat answer.txt)\" = 42 || exit 10\n";
    let wrong = PROVIDER.replace("'42'", "'0'");
    trial(root.path(), oracle, &wrong, "wrong")?;
    trial(root.path(), oracle, PROVIDER, "right")?;
    let output = Command::new(env!("CARGO_BIN_EXE_agent-experiments"))
        .arg("repository-compare")
        .arg(root.path().join("wrong"))
        .arg(root.path().join("right"))
        .output()?;
    assert!(output.status.success());
    let comparison: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(comparison.get("observed_gain"), Some(&json!(false)));
    Ok(())
}

#[test]
fn dedicated_docker_endpoint_reaches_the_agent_environment() -> Result {
    let root = tempfile::tempdir()?;
    let provider = format!("{PROVIDER}\nprintf '%s' \"$DOCKER_HOST\" > endpoint.txt");
    trial(root.path(), "true\n", &provider, "endpoint")?;
    let result: Value =
        serde_json::from_slice(&fs::read(root.path().join("endpoint/result.json"))?)?;
    assert_eq!(
        result.get("endpoint.txt"),
        Some(&json!("tcp://127.0.0.1:65534"))
    );
    Ok(())
}

#[test]
fn empty_or_implicit_docker_endpoints_refuse_before_execution() -> Result {
    let root = tempfile::tempdir()?;
    trial(root.path(), "true\n", PROVIDER, "initial")?;
    for host in [
        "",
        " ",
        "unix://",
        "unix:///var/run/docker.sock",
        "tcp://127.0.0.1:0",
        "tcp://127.0.0.1:65536",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-experiments"))
            .args(["repository-run", "--case"])
            .arg(root.path().join("case.json"))
            .arg("--instructions")
            .arg(root.path().join("instructions.md"))
            .args(["--model", "fixture-model", "--report"])
            .arg(root.path().join("rejected"))
            .args(["--tool-path", "/usr/bin:/bin", "--docker-host", host])
            .output()?;
        assert!(!output.status.success());
        assert!(!root.path().join("rejected").exists());
        let options = agent_experiments::eval::repository::Options {
            case: root.path().join("case.json"),
            instructions: root.path().join("instructions.md"),
            report: root.path().join("rejected"),
            model: "fixture-model".into(),
            effort: "low".into(),
            timeout_seconds: 1,
            tool_path: "/usr/bin:/bin".into(),
            network: false,
            docker_host: Some(host.into()),
        };
        let result = agent_experiments::eval::repository::run(&options);
        assert!(result.is_err_and(|error| error.contains("Dedicated Docker endpoint")));
    }
    Ok(())
}
