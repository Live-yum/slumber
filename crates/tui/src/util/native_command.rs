//! In-process filtering/export. External shell use is an explicit normal-mode
//! opt-in.
use anyhow::{anyhow, bail};
use std::path::Path;

pub async fn run(
    shell: &[String],
    command: &str,
    body: &[u8],
) -> anyhow::Result<Vec<u8>> {
    let command = command.trim();
    let command = if command == "jq" { "." } else { command };
    if let Some(external) = command.strip_prefix('!') {
        if slumber_util::paths::portable_directory().is_some() {
            bail!(
                "External shell integrations are disabled in portable mode. Use native jq filters or save PATH"
            );
        }
        return super::run_command(shell, external.trim(), Some(body)).await;
    }
    if let Some(path) = command
        .strip_prefix("save ")
        .or_else(|| command.strip_prefix("tee "))
    {
        let path = path.trim().strip_prefix("> ").unwrap_or(path.trim());
        let path = path
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(path);
        slumber_console::editor::save_new(Path::new(path), body)?;
        return Ok(if command.starts_with("tee ") {
            body.to_vec()
        } else {
            Vec::new()
        });
    }
    if command == "cat" {
        return Ok(body.to_vec());
    }
    if let Some(count) = command.strip_prefix("head -c ") {
        let count = count.parse::<usize>()?;
        return Ok(body[..count.min(body.len())].to_vec());
    }
    if let Some(count) = command.strip_prefix("head -n ") {
        let count = count.parse::<usize>()?;
        return Ok(body
            .split_inclusive(|byte| *byte == b'\n')
            .take(count)
            .flatten()
            .copied()
            .collect());
    }
    let mut query = command.strip_prefix("jq ").unwrap_or(command).trim();
    let mut raw = false;
    let mut compact = false;
    loop {
        if let Some(rest) = query.strip_prefix("-r ") {
            raw = true;
            query = rest.trim_start();
        } else if let Some(rest) = query.strip_prefix("-c ") {
            compact = true;
            query = rest.trim_start();
        } else {
            break;
        }
    }
    let unquoted;
    if query.starts_with(['\'', '"']) {
        let args = shell_words::split(query)?;
        if args.len() != 1 {
            bail!("Use one quoted jq filter, or enter the filter directly");
        }
        unquoted = args.into_iter().next().expect("one argument");
        query = &unquoted;
    }
    slumber_core::render::query_json_bytes(query, body, raw, compact)
        .map_err(|error| anyhow!(error))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn export_is_atomic_and_refuses_overwrite() {
        let path = std::env::temp_dir()
            .join(format!("export 中文 {}.json", uuid::Uuid::new_v4()));
        let command = format!("save \"{}\"", path.display());
        let missing = vec!["nonexistent-shell-for-native-test".to_owned()];
        run(&missing, &command, b"original").await.unwrap();
        assert!(run(&missing, &command, b"replacement").await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        std::fs::remove_file(path).unwrap();
    }
    #[tokio::test]
    async fn no_external_program_is_used() {
        let missing = vec!["nonexistent-shell-for-native-test".to_owned()];
        assert_eq!(run(&missing, "jq", b"{}").await.unwrap(), b"{}\n");
        assert_eq!(
            run(&missing, "jq -r '.name'", br#"{"name":"value"}"#)
                .await
                .unwrap(),
            b"value\n"
        );
        assert_eq!(run(&missing, "head -c 1", b"abc").await.unwrap(), b"a");
        assert_eq!(
            run(&missing, ".id", br#"{"id":9007199254740993}"#)
                .await
                .unwrap(),
            b"9007199254740993\n"
        );
        assert!(
            run(&missing, "definitely-not-a-command", b"{}")
                .await
                .is_err()
        );
    }
}
