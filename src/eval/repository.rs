use super::{
    codex::parse_codex_events,
    contracts::{nonempty, validate_path},
    fixture::Fixture,
    identity::{date, runner_revision},
    live::{Codex, codex_arguments},
    process::{CaptureOptions, CaptureResult, ExecutionError, capture},
    report::Status,
    sources::fingerprint,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    prompt: String,
    files: BTreeMap<String, String>,
    setup: String,
    oracle: BTreeMap<String, String>,
}

pub struct Options {
    pub case: PathBuf,
    pub instructions: PathBuf,
    pub report: PathBuf,
    pub model: String,
    pub effort: String,
    pub timeout_seconds: u64,
    pub tool_path: String,
    pub network: bool,
    pub docker_host: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema: String,
    date: String,
    case: String,
    case_hash: String,
    instruction_hash: String,
    runner_hash: String,
    agent_version: String,
    model: String,
    effort: String,
    timeout_seconds: u64,
    tool_path: String,
    network: bool,
    #[serde(default)]
    docker_host: Option<String>,
    #[serde(default)]
    writable_git_metadata: bool,
    environment: String,
    status: Status,
    error: Option<String>,
    artifacts: BTreeMap<String, String>,
    limitations: Vec<String>,
}

/// # Errors
/// Refuses invalid inputs, reused destinations, unavailable providers, and incomplete evidence.
pub fn run(options: &Options) -> Result<Status, String> {
    let case_bytes = fs::read(&options.case).map_err(|e| e.to_string())?;
    let case: Case = serde_json::from_slice(&case_bytes).map_err(|e| e.to_string())?;
    validate_case(&case, options)?;
    let instructions = fs::read_to_string(&options.instructions).map_err(|e| e.to_string())?;
    let codex = Codex::discover()?;
    fs::create_dir(&options.report).map_err(|e| format!("New report directory required: {e}"))?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let revision = runner_revision(&executable)?;
    let mut fixture = Fixture::prepare(&case.files, &instructions, &executable)?;
    fixture.env.insert("PATH".into(), options.tool_path.clone());
    fixture.env.insert("TZ".into(), "Europe/Paris".into());
    fixture.env.insert(
        "DOCKER_HOST".into(),
        options
            .docker_host
            .clone()
            .unwrap_or_else(|| "unix:///dev/null".into()),
    );
    let oracle_root = fixture.root.join("oracle");
    install(&oracle_root, &case.oracle)?;
    let setup_path = fixture.root.join("setup.sh");
    fs::write(&setup_path, &case.setup).map_err(|e| e.to_string())?;
    let setup = script(&setup_path, &fixture, options.timeout_seconds);
    fs::write(options.report.join("setup.log"), &setup.output).map_err(|e| e.to_string())?;
    let mut error = setup.failure_detail.map(|e| format!("Setup: {e}"));
    let mut status = Status::Invalid;
    let writable_git_metadata = fixture.workspace.join(".git").is_dir();
    if error.is_none() {
        let execution = execute_agent(
            options,
            &codex,
            &fixture,
            writable_git_metadata,
            &case.prompt,
        )?;
        fs::write(options.report.join("agent.jsonl"), &execution.output)
            .map_err(|e| e.to_string())?;
        error = execution.failure_detail;
        if error.is_none() {
            error = parse_codex_events(&execution.output).err();
        }
        retain_workspace(&options.report, "result", &fixture.workspace)?;
        if error.is_none() {
            let oracle = script(
                &oracle_root.join("run.sh"),
                &fixture,
                options.timeout_seconds,
            );
            fs::write(options.report.join("oracle.log"), &oracle.output)
                .map_err(|e| e.to_string())?;
            status = match (oracle.exit_code, oracle.error) {
                (Some(0), None) => Status::Pass,
                (Some(10), Some(ExecutionError::AgentFailed)) => Status::Fail,
                _ => Status::Invalid,
            };
            if status == Status::Invalid {
                error = oracle
                    .failure_detail
                    .or_else(|| Some("Invalid oracle exit".into()));
            }
        }
    }
    let actual_oracle = snapshot(&oracle_root)?;
    if actual_oracle != case.oracle {
        return Err("Oracle changed during trial".into());
    }
    retain_workspace(&options.report, "verified", &fixture.workspace)?;
    fs::write(options.report.join("case.json"), case_bytes).map_err(|e| e.to_string())?;
    fs::write(options.report.join("instructions.txt"), &instructions).map_err(|e| e.to_string())?;
    let report = Report {
        schema: "repository-trial-v1".into(), date: date()?, case: case.id,
        case_hash: fingerprint(fs::read(options.report.join("case.json")).map_err(|e| e.to_string())?),
        instruction_hash: fingerprint(instructions), runner_hash: revision.clone(),
        agent_version: codex.version, model: options.model.clone(), effort: options.effort.clone(),
        timeout_seconds: options.timeout_seconds, tool_path: options.tool_path.clone(), network: options.network, docker_host: options.docker_host.clone(), writable_git_metadata,
        environment: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH), status, error,
        artifacts: artifact_hashes(&options.report)?,
        limitations: [
            "Explicit instruction injection measures application, not automatic skill activation.",
            "A trial is one observation, not statistical or causal evidence of model improvement.",
            "Workspace-write is the Codex policy; no claim of adversarial isolation or tamper-proof evidence.",
            "The caller owns trusted setup and oracle scripts. Only synthetic non-sensitive fixtures are supported.",
            "Agent stdout is retained; stderr is not. Output is capped at 4 MiB per process; no token ceiling.",
            "Snapshots retain UTF-8 files and Unix modes; Git metadata and node_modules are excluded. Binary files refuse retention.",
            "Tool lookup paths do not pin tool versions; callers must retain their runtime inventory.",
        ].map(str::to_owned).to_vec(),
    };
    if runner_revision(&executable)? != revision {
        return Err("Runner executable changed during trial".into());
    }
    write_json(&options.report.join("report.json"), &report)?;
    Ok(status)
}

fn execute_agent(
    options: &Options,
    codex: &Codex,
    fixture: &Fixture,
    writable_git_metadata: bool,
    prompt: &str,
) -> Result<CaptureResult, String> {
    let mut args = codex_arguments(&options.model, &options.effort);
    if options.network {
        for argument in &mut args {
            if argument == "sandbox_workspace_write.network_access=false" {
                *argument = "sandbox_workspace_write.network_access=true".into();
            }
        }
    }
    if writable_git_metadata {
        let _ = args.pop();
        args.extend([
            "-c".into(),
            format!(
                "sandbox_workspace_write.writable_roots={}",
                serde_json::to_string(&[fixture.workspace.join(".git")])
                    .map_err(|e| e.to_string())?
            ),
            "-".into(),
        ]);
    }
    let execution = capture(
        &codex.command,
        &args,
        CaptureOptions {
            cwd: &fixture.workspace,
            env: &codex.environment(fixture)?,
            stdin: prompt,
            timeout: Duration::from_secs(options.timeout_seconds),
        },
    );
    Ok(execution)
}

/// # Errors
/// Refuses implicit, empty or non-loopback Docker endpoints.
pub fn dedicated_docker_host(value: &str) -> Result<String, String> {
    let valid = value.strip_prefix("tcp://127.0.0.1:").is_some_and(|port| {
        !port.is_empty()
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && port.parse::<u16>().is_ok_and(|port| port > 0)
    });
    if valid {
        Ok(value.into())
    } else {
        Err("Dedicated Docker endpoint must be tcp://127.0.0.1:<1-65535>".into())
    }
}

fn validate_case(case: &Case, options: &Options) -> Result<(), String> {
    nonempty(&case.id)?;
    nonempty(&case.prompt)?;
    nonempty(&options.model)?;
    nonempty(&options.tool_path)?;
    if let Some(host) = &options.docker_host {
        let _ = dedicated_docker_host(host)?;
    }
    if case.files.is_empty()
        || !case.oracle.contains_key("run.sh")
        || !(1..=600).contains(&options.timeout_seconds)
    {
        return Err("Nonempty fixture, oracle/run.sh and timeout 1–600 required".into());
    }
    for path in case.files.keys().chain(case.oracle.keys()) {
        validate_path(path)?;
        if path
            .split('/')
            .any(|part| [".git", ".eval-bin", ".observations.jsonl", "AGENTS.md"].contains(&part))
        {
            return Err(format!("Reserved fixture path: {path}"));
        }
    }
    Ok(())
}

fn retain_workspace(report: &Path, name: &str, root: &Path) -> Result<(), String> {
    let files = snapshot(root)?;
    let modes = files
        .keys()
        .map(|path| {
            fs::metadata(root.join(path))
                .map(|metadata| (path.clone(), metadata.permissions().mode() & 0o777))
                .map_err(|e| e.to_string())
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    write_json(&report.join(format!("{name}.json")), &files)?;
    write_json(&report.join(format!("{name}-modes.json")), &modes)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    text.push('\n');
    fs::write(path, text).map_err(|e| e.to_string())
}

fn script(path: &Path, fixture: &Fixture, timeout: u64) -> CaptureResult {
    capture(
        Path::new("/bin/bash"),
        &[
            "-e".into(),
            path.to_string_lossy().into_owned(),
            fixture.workspace.to_string_lossy().into_owned(),
        ],
        CaptureOptions {
            cwd: &fixture.workspace,
            env: &fixture.env,
            stdin: "",
            timeout: Duration::from_secs(timeout),
        },
    )
}

fn install(root: &Path, files: &BTreeMap<String, String>) -> Result<(), String> {
    for (path, content) in files {
        validate_path(path)?;
        let destination = root.join(path);
        fs::create_dir_all(destination.parent().ok_or("Missing parent")?)
            .map_err(|e| e.to_string())?;
        fs::write(destination, content).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn snapshot(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut files = BTreeMap::new();
    collect(root, root, &mut files)?;
    Ok(files)
}

fn collect(root: &Path, path: &Path, files: &mut BTreeMap<String, String>) -> Result<(), String> {
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if [".git", ".eval-bin", ".observations.jsonl", "node_modules"]
            .iter()
            .any(|skip| name == *skip)
        {
            continue;
        }
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            return Err("Symlinks are not supported in evidence".into());
        }
        if kind.is_dir() {
            collect(root, &entry.path(), files)?;
        } else if kind.is_file() {
            if entry.metadata().map_err(|e| e.to_string())?.len() > 4_194_304 || files.len() >= 512
            {
                return Err("Evidence exceeds file bounds".into());
            }
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into_owned();
            let text = fs::read_to_string(entry.path()).map_err(|e| e.to_string())?;
            files.insert(relative, text);
        } else {
            return Err("Non-regular evidence file".into());
        }
    }
    Ok(())
}

fn artifact_hashes(root: &Path) -> Result<BTreeMap<String, String>, String> {
    Ok(snapshot(root)?
        .into_iter()
        .filter(|(path, _)| path != "report.json")
        .map(|(path, text)| (path, fingerprint(text)))
        .collect())
}

fn read_report(root: &Path) -> Result<Report, String> {
    let report: Report =
        serde_json::from_slice(&fs::read(root.join("report.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if report.schema != "repository-trial-v1" || artifact_hashes(root)? != report.artifacts {
        return Err("Evidence integrity mismatch".into());
    }
    for (path, hash) in [
        ("case.json", &report.case_hash),
        ("instructions.txt", &report.instruction_hash),
    ] {
        if report.artifacts.get(path) != Some(hash) {
            return Err("Input identity mismatch".into());
        }
    }
    Ok(report)
}

/// # Errors
/// Rejects damaged evidence, invalid runs, and reports whose controlled inputs differ.
pub fn compare(left: &Path, right: &Path) -> Result<serde_json::Value, String> {
    let a = read_report(left)?;
    let b = read_report(right)?;
    if a.status == Status::Invalid || b.status == Status::Invalid {
        return Err("INVALID runs cannot establish a comparison".into());
    }
    if a.case_hash != b.case_hash
        || a.runner_hash != b.runner_hash
        || a.agent_version != b.agent_version
        || a.model != b.model
        || a.effort != b.effort
        || a.timeout_seconds != b.timeout_seconds
        || a.tool_path != b.tool_path
        || a.network != b.network
        || a.docker_host != b.docker_host
        || a.writable_git_metadata != b.writable_git_metadata
        || a.environment != b.environment
    {
        return Err("Incompatible repository trials".into());
    }
    Ok(
        serde_json::json!({"case": a.case, "baseline": a.status, "candidate": b.status,
        "observed_gain": a.status == Status::Fail && b.status == Status::Pass && a.instruction_hash != b.instruction_hash,
        "instruction_changed": a.instruction_hash != b.instruction_hash}),
    )
}
