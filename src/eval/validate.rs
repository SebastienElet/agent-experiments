use super::{
    contracts::validate_path,
    evidence::read_report,
    sources::{load_cases, load_trigger, read_source, source_path},
};
use serde::Deserialize;
use std::{fs, path::Path, process::Command};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    skills: Vec<String>,
    historical_evidence: Vec<String>,
}

/// # Errors
/// Rejects invalid selections, inaccessible sources, numbered guidance, or invalid activation contracts.
pub fn validate_evaluations(repository: &Path) -> Result<String, String> {
    let cases = load_cases(repository)?;
    let selection: Selection = serde_json::from_str(&read_source(repository, "selection.json")?)
        .map_err(|e| e.to_string())?;
    if selection.skills.is_empty() {
        return Err("Empty skill selection".into());
    }
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repository)
        .args(["ls-files", "-z", "--"]);
    for path in &selection.skills {
        validate_path(path)?;
        source_path(repository, path)?;
        command.arg(format!(":(literal){path}"));
    }
    for path in &selection.historical_evidence {
        validate_path(path)?;
    }
    let result = command
        .output()
        .map_err(|e| format!("Cannot enumerate tracked skills and activation contracts: {e}"))?;
    if !result.status.success() {
        return Err("Cannot enumerate tracked skills and activation contracts".into());
    }
    let output = String::from_utf8(result.stdout).map_err(|e| e.to_string())?;
    let paths = output
        .split('\0')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Err("Empty tracked skill selection".into());
    }
    for path in &paths {
        let text = read_source(repository, path)
            .map_err(|e| format!("Cannot read tracked skill: {path}: {e}"))?;
        if !selection
            .historical_evidence
            .iter()
            .any(|exception| exception == path)
            && text.as_bytes().windows(2).any(|pair| {
                pair.first() == Some(&b'#') && pair.get(1).is_some_and(u8::is_ascii_digit)
            })
        {
            return Err(format!(
                "Forbidden numbered PR/issue reference in skill: {path}"
            ));
        }
    }
    let triggers = paths
        .iter()
        .filter(|p| p.ends_with("/evals/trigger-queries.json"))
        .collect::<Vec<_>>();
    for path in &triggers {
        load_trigger(repository, path)
            .map_err(|e| format!("Invalid activation contract: {path}: {e}"))?;
    }
    Ok(format!(
        "{} behavioral cases; {} activation contracts valid",
        cases.len(),
        triggers.len()
    ))
}

/// # Errors
/// Rejects inaccessible evidence directories and invalid selected reports.
pub fn validate_evidence(repository: &Path) -> Result<String, String> {
    let directory = source_path(repository, "evidence")?;
    let mut files = fs::read_dir(directory)
        .map_err(|e| e.to_string())?
        .map(|entry| entry.map(|e| e.path()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    files.retain(|path| path.extension().is_some_and(|ext| ext == "json"));
    files.sort();
    for file in &files {
        let name = file
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Invalid evidence filename")?;
        let confined = source_path(repository, &format!("evidence/{name}"))?;
        read_report(&confined).map_err(|e| format!("Invalid evidence: {}: {e}", file.display()))?;
    }
    Ok(format!(
        "{} historical reports valid; live evidence is optional",
        files.len()
    ))
}
