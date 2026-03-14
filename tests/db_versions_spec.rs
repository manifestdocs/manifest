mod common;

use chrono::Utc;
use common::*;
use manifest::db::Database;
use manifest::models::*;

// ============================================================
// Version Guard Rails
// ============================================================

mod version_guard_rails {
    use super::*;

    async fn create_released_version(db: &Database, project_id: ProjectId) -> Version {
        let version = db
            .create_version(
                project_id,
                CreateVersionInput {
                    id: None,
                    name: "v1.0.0".to_string(),
                    description: None,
                },
            )
            .await
            .expect("Failed to create version");

        db.update_version(
            version.id,
            UpdateVersionInput {
                name: None,
                description: None,
                released_at: Some(Utc::now()),
            },
        )
        .await
        .expect("Failed to release version")
        .expect("Version not found");

        db.get_version(version.id)
            .await
            .expect("Failed to get version")
            .expect("Version not found")
    }

    #[tokio::test]
    async fn rejects_non_semver_version_names() {
        let db = setup().await;
        let project = create_test_project(&db).await;

        for name in [
            "now", "Next", "MVP", "Sprint 1", "latest", "1.0", "1", "abc",
        ] {
            let result = db
                .create_version(
                    project.id,
                    CreateVersionInput {
                        id: None,
                        name: name.to_string(),
                        description: None,
                    },
                )
                .await;

            assert!(
                result.is_err(),
                "Expected '{}' to be rejected as a version name",
                name
            );
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains("not a valid semantic version"),
                "Expected semver error for '{}', got: {}",
                name,
                err
            );
        }
    }

    #[tokio::test]
    async fn accepts_valid_semver_version_names() {
        let db = setup().await;
        let project = create_test_project(&db).await;

        for name in ["0.1.0", "v0.1.0", "1.0.0", "v2.3.1"] {
            let result = db
                .create_version(
                    project.id,
                    CreateVersionInput {
                        id: None,
                        name: name.to_string(),
                        description: None,
                    },
                )
                .await;

            assert!(
                result.is_ok(),
                "Expected '{}' to be accepted, got: {}",
                name,
                result.unwrap_err()
            );
        }
    }

    #[tokio::test]
    async fn rejects_version_creation_beyond_cap() {
        let db = setup().await;
        let project = create_test_project(&db).await;

        for i in 1..=6 {
            db.create_version(
                project.id,
                CreateVersionInput {
                    id: None,
                    name: format!("0.{}.0", i),
                    description: None,
                },
            )
            .await
            .expect("Failed to create version");
        }

        let result = db
            .create_version(
                project.id,
                CreateVersionInput {
                    id: None,
                    name: "0.7.0".to_string(),
                    description: None,
                },
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("max 6"),
            "Expected max versions error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn rejects_update_feature_to_released_version() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let released = create_released_version(&db, project.id).await;

        let feature = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Test Feature".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create feature");

        let result = db
            .update_feature(
                feature.id,
                UpdateFeatureInput {
                    parent_id: None,
                    title: None,
                    details: None,
                    desired_details: None,
                    details_summary: None,
                    priority: None,
                    target_version_id: Some(Some(released.id)),
                    state: None,
                    blocked_by: None,
                },
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("released version"),
            "Expected released version error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn rejects_create_feature_with_released_version() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let released = create_released_version(&db, project.id).await;

        let result = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Test Feature".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: Some(released.id),
                    state: None,
                },
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("released version"),
            "Expected released version error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn allows_assignment_to_unreleased_version() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let version = db
            .create_version(
                project.id,
                CreateVersionInput {
                    id: None,
                    name: "v2.0.0".to_string(),
                    description: None,
                },
            )
            .await
            .expect("Failed to create version");

        let feature = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Test Feature".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: Some(version.id),
                    state: None,
                },
            )
            .await
            .expect("Failed to create feature");

        assert_eq!(feature.target_version_id, Some(version.id));

        // Also test update path
        let feature2 = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Another Feature".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create feature");

        let updated = db
            .update_feature(
                feature2.id,
                UpdateFeatureInput {
                    parent_id: None,
                    title: None,
                    details: None,
                    desired_details: None,
                    details_summary: None,
                    priority: None,
                    target_version_id: Some(Some(version.id)),
                    state: None,
                    blocked_by: None,
                },
            )
            .await
            .expect("Failed to update feature")
            .expect("Feature not found");

        assert_eq!(updated.target_version_id, Some(version.id));
    }
}
