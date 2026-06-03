use super::*;

impl CortexService {
    pub async fn import_shell_history(
        &self,
        path: PathBuf,
        shell: String,
    ) -> ServiceResult<CommandLogImportResult> {
        self.run_db("import_shell_history", move |pool| {
            command_log::import_zsh_history(pool, &path, &shell)
        })
        .await
    }

    pub async fn import_atuin_history(
        &self,
        path: PathBuf,
    ) -> ServiceResult<CommandLogImportResult> {
        self.run_db("import_atuin_history", move |pool| {
            command_log::import_atuin_history(pool, &path)
        })
        .await
    }

    pub async fn import_agent_command_spool(
        &self,
        path: PathBuf,
    ) -> ServiceResult<CommandLogImportResult> {
        self.run_db("import_agent_command_spool", move |pool| {
            command_log::import_agent_command_spool(pool, &path)
        })
        .await
    }
}
