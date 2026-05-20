use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    models::Project,
    store::Store,
};

pub fn resolve_project(store: &Store, selector: &str) -> AppResult<Project> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Err(AppError::ToolFailed {
            tool: "project".to_string(),
            message: "project selector cannot be empty".to_string(),
        });
    }

    let projects = store.list_projects()?;

    if let Ok(uuid) = Uuid::parse_str(selector) {
        return projects
            .into_iter()
            .find(|p| p.id == uuid)
            .ok_or_else(|| AppError::ToolFailed {
                tool: "project".to_string(),
                message: format!("no project with id {selector}"),
            });
    }

    let by_name: Vec<_> = projects
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case(selector))
        .cloned()
        .collect();
    if by_name.len() == 1 {
        return Ok(by_name[0].clone());
    }
    if by_name.len() > 1 {
        return Err(AppError::ToolFailed {
            tool: "project".to_string(),
            message: format!("multiple projects named `{selector}`"),
        });
    }

    let by_prefix: Vec<_> = projects
        .iter()
        .filter(|p| p.id.to_string().starts_with(selector))
        .cloned()
        .collect();
    match by_prefix.len() {
        0 => Err(AppError::ToolFailed {
            tool: "project".to_string(),
            message: format!("no project matching `{selector}`"),
        }),
        1 => Ok(by_prefix[0].clone()),
        _ => Err(AppError::ToolFailed {
            tool: "project".to_string(),
            message: format!("ambiguous project id prefix `{selector}`"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Project, ProjectStatus};
    use uuid::Uuid;

    fn sample(id: Uuid, name: &str) -> Project {
        Project {
            id,
            name: name.to_string(),
            path: "/tmp".to_string(),
            domain: format!("{name}.test"),
            port: 3000,
            ssl: true,
            status: ProjectStatus::Stopped,
            command: "pnpm".into(),
            args: vec!["dev".into()],
            pid: None,
        }
    }

    #[test]
    fn resolves_by_name() {
        let dir = std::env::temp_dir().join(format!("domainest-resolve-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("projects.json");
        let id = Uuid::new_v4();
        let projects = vec![sample(id, "be-brand")];
        std::fs::write(&path, serde_json::to_string(&projects).unwrap()).unwrap();
        let store = Store::from_path(path);
        let p = resolve_project(&store, "be-brand").unwrap();
        assert_eq!(p.id, id);
        let _ = std::fs::remove_dir_all(dir);
    }
}
