mod common;

use common::*;
use manifest::models::*;

// ============================================================
// Projects
// ============================================================

mod projects {
    use super::*;

    mod create_project {
        use super::*;

        #[tokio::test]
        async fn creates_project_with_required_fields() {
            let db = setup().await;
            let project = db
                .create_project(CreateProjectInput {
                    slug: None,
                    name: "My Project".to_string(),
                    description: None,
                    instructions: None,
                    key_prefix: None,
                })
                .await
                .expect("Failed to create project");

            assert_eq!(project.name, "My Project");
            assert!(project.description.is_none());
        }

        #[tokio::test]
        async fn creates_project_with_all_fields() {
            let db = setup().await;
            let project = db
                .create_project(CreateProjectInput {
                    slug: None,
                    name: "Full Project".to_string(),
                    description: Some("A complete project".to_string()),
                    instructions: Some("Use cargo test to run tests".to_string()),
                    key_prefix: None,
                })
                .await
                .expect("Failed to create project");

            assert_eq!(project.name, "Full Project");
            assert_eq!(project.description, Some("A complete project".to_string()));
            assert_eq!(
                project.instructions,
                Some("Use cargo test to run tests".to_string())
            );
        }
    }

    mod get_project {
        use super::*;

        #[tokio::test]
        async fn returns_none_for_nonexistent_project() {
            let db = setup().await;
            let result = db
                .get_project(ProjectId::new())
                .await
                .expect("Query failed");
            assert!(result.is_none());
        }

        #[tokio::test]
        async fn returns_project_by_id() {
            let db = setup().await;
            let created = db
                .create_project(CreateProjectInput {
                    slug: None,
                    name: "Test".to_string(),
                    description: None,
                    instructions: None,
                    key_prefix: None,
                })
                .await
                .expect("Failed to create");

            let found = db.get_project(created.id).await.expect("Query failed");
            assert!(found.is_some());
            assert_eq!(found.unwrap().name, "Test");
        }
    }

    mod get_all_projects {
        use super::*;

        #[tokio::test]
        async fn returns_empty_list_when_no_projects() {
            let db = setup().await;
            let projects = db.get_all_projects().await.expect("Query failed");
            assert!(projects.is_empty());
        }

        #[tokio::test]
        async fn returns_all_projects_ordered_by_name() {
            let db = setup().await;
            db.create_project(CreateProjectInput {
                slug: None,
                name: "Zebra".to_string(),
                description: None,
                instructions: None,
                key_prefix: None,
            })
            .await
            .expect("Failed to create");

            db.create_project(CreateProjectInput {
                slug: None,
                name: "Alpha".to_string(),
                description: None,
                instructions: None,
                key_prefix: None,
            })
            .await
            .expect("Failed to create");

            let projects = db.get_all_projects().await.expect("Query failed");
            assert_eq!(projects.len(), 2);
            assert_eq!(projects[0].name, "Alpha");
            assert_eq!(projects[1].name, "Zebra");
        }
    }

    mod delete_project {
        use super::*;

        #[tokio::test]
        async fn deletes_project_and_cascades_to_features() {
            let db = setup().await;
            let project = create_test_project(&db).await;

            db.create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Feature".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create feature");

            db.delete_project(project.id)
                .await
                .expect("Failed to delete");

            let features = db
                .get_features_by_project(project.id)
                .await
                .expect("Query failed");
            assert!(features.is_empty());
        }
    }
}

// ============================================================
// Project Directories
// ============================================================

mod project_directories {
    use super::*;

    mod add_project_directory {
        use super::*;

        #[tokio::test]
        async fn adds_directory_to_project() {
            let db = setup().await;
            let project = create_test_project(&db).await;

            let dir = db
                .add_project_directory(
                    project.id,
                    AddDirectoryInput {
                        path: "/home/user/project".to_string(),
                        git_remote: Some("git@github.com:user/project.git".to_string()),
                        is_primary: true,
                        instructions: Some("Run npm test".to_string()),
                    },
                )
                .await
                .expect("Failed to add directory");

            assert_eq!(dir.project_id, project.id);
            assert_eq!(dir.path, "/home/user/project");
            assert!(dir.is_primary);
            assert_eq!(dir.instructions, Some("Run npm test".to_string()));
        }
    }

    mod get_project_directories {
        use super::*;

        #[tokio::test]
        async fn returns_directories_ordered_by_primary_then_path() {
            let db = setup().await;
            let project = create_test_project(&db).await;

            db.add_project_directory(
                project.id,
                AddDirectoryInput {
                    path: "/b/path".to_string(),
                    git_remote: None,
                    is_primary: false,
                    instructions: None,
                },
            )
            .await
            .expect("Failed");

            db.add_project_directory(
                project.id,
                AddDirectoryInput {
                    path: "/a/path".to_string(),
                    git_remote: None,
                    is_primary: true,
                    instructions: None,
                },
            )
            .await
            .expect("Failed");

            let dirs = db
                .get_project_directories(project.id)
                .await
                .expect("Query failed");
            assert_eq!(dirs.len(), 2);
            assert!(dirs[0].is_primary); // Primary first
            assert_eq!(dirs[1].path, "/b/path");
        }
    }
}
