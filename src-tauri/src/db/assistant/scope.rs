pub fn validate_assistant_scope(scope: &str, workspace_folder: Option<&str>) -> Result<(), String> {
    match scope.trim() {
        "current" => {
            if workspace_folder
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                return Err("workspaceFolder is required for current scope".to_string());
            }
            Ok(())
        }
        "global" => Ok(()),
        _ => Err("assistant scope must be current or global".to_string()),
    }
}
