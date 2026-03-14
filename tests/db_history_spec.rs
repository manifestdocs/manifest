mod common;

use common::*;
use manifest::models::*;

// ============================================================
// Feature History
// ============================================================

mod feature_history {
    use super::*;

    mod create_history_entry {
        use super::*;

        #[tokio::test]
        async fn creates_history_entry_with_all_fields() {
            let db = setup().await;
            let project = create_test_project(&db).await;
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

            // Create a version to link the history entry to
            let version = db
                .create_version(
                    project.id,
                    CreateVersionInput {
                        id: None,
                        name: "v1.0.0".to_string(),
                        description: None,
                    },
                )
                .await
                .expect("Failed to create version");

            let entry = db
                .create_history_entry(CreateHistoryInput {
                    feature_id: feature.id,
                    version_id: Some(version.id),
                    details: HistoryDetails {
                        summary: "Implemented login flow".to_string(),
                        commits: vec![],
                        ..Default::default()
                    },
                })
                .await
                .expect("Failed to create history entry");

            assert_eq!(entry.feature_id, feature.id);
            assert_eq!(entry.version_id, Some(version.id));
            assert_eq!(entry.details.summary, "Implemented login flow");
        }

        #[tokio::test]
        async fn creates_entry_without_version_id() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let feature = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Manual Feature".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create feature");

            let entry = db
                .create_history_entry(CreateHistoryInput {
                    feature_id: feature.id,
                    version_id: None,
                    details: HistoryDetails {
                        summary: "Manual update".to_string(),
                        commits: vec![],
                        ..Default::default()
                    },
                })
                .await
                .expect("Failed to create history entry");

            assert!(entry.version_id.is_none());
        }
    }

    mod get_feature_history {
        use super::*;

        #[tokio::test]
        async fn returns_empty_list_when_no_history() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let feature = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "New Feature".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create feature");

            let history = db
                .get_feature_history(feature.id)
                .await
                .expect("Query failed");
            assert!(history.is_empty());
        }

        #[tokio::test]
        async fn returns_history_entries_in_reverse_chronological_order() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let feature = db
                .create_feature(
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

            db.create_history_entry(CreateHistoryInput {
                feature_id: feature.id,
                version_id: None,
                details: HistoryDetails {
                    summary: "First change".to_string(),
                    commits: vec![],
                    ..Default::default()
                },
            })
            .await
            .expect("Failed to create");

            db.create_history_entry(CreateHistoryInput {
                feature_id: feature.id,
                version_id: None,
                details: HistoryDetails {
                    summary: "Second change".to_string(),
                    commits: vec![],
                    ..Default::default()
                },
            })
            .await
            .expect("Failed to create");

            let history = db
                .get_feature_history(feature.id)
                .await
                .expect("Query failed");

            assert_eq!(history.len(), 2);
            assert_eq!(history[0].details.summary, "Second change");
            assert_eq!(history[1].details.summary, "First change");
        }

        #[tokio::test]
        async fn only_returns_history_for_specified_feature() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let feature1 = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Feature 1".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            let feature2 = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Feature 2".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            db.create_history_entry(CreateHistoryInput {
                feature_id: feature1.id,
                version_id: None,
                details: HistoryDetails {
                    summary: "Change to feature 1".to_string(),
                    commits: vec![],
                    ..Default::default()
                },
            })
            .await
            .expect("Failed to create");

            db.create_history_entry(CreateHistoryInput {
                feature_id: feature2.id,
                version_id: None,
                details: HistoryDetails {
                    summary: "Change to feature 2".to_string(),
                    commits: vec![],
                    ..Default::default()
                },
            })
            .await
            .expect("Failed to create");

            let history = db
                .get_feature_history(feature1.id)
                .await
                .expect("Query failed");

            assert_eq!(history.len(), 1);
            assert_eq!(history[0].details.summary, "Change to feature 1");
        }
    }

    mod cascade_delete {
        use super::*;

        #[tokio::test]
        async fn deletes_history_when_feature_is_deleted() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let feature = db
                .create_feature(
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
                .expect("Failed to create");

            db.create_history_entry(CreateHistoryInput {
                feature_id: feature.id,
                version_id: None,
                details: HistoryDetails {
                    summary: "Some work".to_string(),
                    commits: vec![],
                    ..Default::default()
                },
            })
            .await
            .expect("Failed to create");

            db.delete_feature(feature.id)
                .await
                .expect("Failed to delete");

            // History should be gone (cascade delete)
            let history = db
                .get_feature_history(feature.id)
                .await
                .expect("Query failed");
            assert!(history.is_empty());
        }
    }

    mod get_project_history {
        use super::*;

        #[tokio::test]
        async fn returns_empty_list_when_no_history() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let history = db
                .get_project_history(project.id, None, None, None, None)
                .await
                .expect("Query failed");
            assert!(history.is_empty());
        }

        #[tokio::test]
        async fn returns_history_entries_across_all_project_features() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let feature1 = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Feature 1".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            let feature2 = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Feature 2".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            db.create_history_entry(CreateHistoryInput {
                feature_id: feature1.id,
                version_id: None,
                details: HistoryDetails {
                    summary: "Work on feature 1".to_string(),
                    commits: vec![],
                    ..Default::default()
                },
            })
            .await
            .expect("Failed to create");

            db.create_history_entry(CreateHistoryInput {
                feature_id: feature2.id,
                version_id: None,
                details: HistoryDetails {
                    summary: "Work on feature 2".to_string(),
                    commits: vec![],
                    ..Default::default()
                },
            })
            .await
            .expect("Failed to create");

            let history = db
                .get_project_history(project.id, None, None, None, None)
                .await
                .expect("Query failed");
            assert_eq!(history.len(), 2);
        }

        #[tokio::test]
        async fn includes_feature_title_and_state_in_entries() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let feature = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "User Authentication".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: Some(FeatureState::Implemented),
                    },
                )
                .await
                .expect("Failed to create");

            db.create_history_entry(CreateHistoryInput {
                feature_id: feature.id,
                version_id: None,
                details: HistoryDetails {
                    summary: "Implemented OAuth flow".to_string(),
                    commits: vec![],
                    ..Default::default()
                },
            })
            .await
            .expect("Failed to create");

            let history = db
                .get_project_history(project.id, None, None, None, None)
                .await
                .expect("Query failed");
            assert_eq!(history.len(), 1);
            assert_eq!(history[0].feature_title, "User Authentication");
            assert_eq!(history[0].feature_state, FeatureState::Implemented);
            assert_eq!(history[0].summary, "Implemented OAuth flow");
        }

        #[tokio::test]
        async fn returns_entries_in_reverse_chronological_order() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let feature = db
                .create_feature(
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
                .expect("Failed to create");

            db.create_history_entry(CreateHistoryInput {
                feature_id: feature.id,
                version_id: None,
                details: HistoryDetails {
                    summary: "First change".to_string(),
                    commits: vec![],
                    ..Default::default()
                },
            })
            .await
            .expect("Failed to create");

            db.create_history_entry(CreateHistoryInput {
                feature_id: feature.id,
                version_id: None,
                details: HistoryDetails {
                    summary: "Second change".to_string(),
                    commits: vec![],
                    ..Default::default()
                },
            })
            .await
            .expect("Failed to create");

            let history = db
                .get_project_history(project.id, None, None, None, None)
                .await
                .expect("Query failed");
            assert_eq!(history.len(), 2);
            assert_eq!(history[0].summary, "Second change");
            assert_eq!(history[1].summary, "First change");
        }

        #[tokio::test]
        async fn respects_limit_parameter() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let feature = db
                .create_feature(
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
                .expect("Failed to create");

            for i in 1..=5 {
                db.create_history_entry(CreateHistoryInput {
                    feature_id: feature.id,
                    version_id: None,
                    details: HistoryDetails {
                        summary: format!("Change {}", i),
                        commits: vec![],
                        ..Default::default()
                    },
                })
                .await
                .expect("Failed to create");
            }

            let history = db
                .get_project_history(project.id, None, Some(2), None, None)
                .await
                .expect("Query failed");
            assert_eq!(history.len(), 2);
        }

        #[tokio::test]
        async fn respects_offset_parameter() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let feature = db
                .create_feature(
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
                .expect("Failed to create");

            for i in 1..=5 {
                db.create_history_entry(CreateHistoryInput {
                    feature_id: feature.id,
                    version_id: None,
                    details: HistoryDetails {
                        summary: format!("Change {}", i),
                        commits: vec![],
                        ..Default::default()
                    },
                })
                .await
                .expect("Failed to create");
            }

            let history = db
                .get_project_history(project.id, None, Some(2), Some(2), None)
                .await
                .expect("Query failed");
            assert_eq!(history.len(), 2);
            // Should skip the 2 most recent entries (5 and 4) and return 3 and 2
            assert_eq!(history[0].summary, "Change 3");
            assert_eq!(history[1].summary, "Change 2");
        }

        #[tokio::test]
        async fn excludes_history_from_other_projects() {
            let db = setup().await;
            let project1 = db
                .create_project(CreateProjectInput {
                    id: None,
                    slug: None,
                    name: "Project 1".to_string(),
                    description: None,
                    instructions: None,
                    key_prefix: None,
                    skip_default_versions: false,
                })
                .await
                .expect("Failed to create");

            let project2 = db
                .create_project(CreateProjectInput {
                    id: None,
                    slug: None,
                    name: "Project 2".to_string(),
                    description: None,
                    instructions: None,
                    key_prefix: None,
                    skip_default_versions: false,
                })
                .await
                .expect("Failed to create");

            let feature1 = db
                .create_feature(
                    project1.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Feature in P1".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            let feature2 = db
                .create_feature(
                    project2.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Feature in P2".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            db.create_history_entry(CreateHistoryInput {
                feature_id: feature1.id,
                version_id: None,
                details: HistoryDetails {
                    summary: "Work in project 1".to_string(),
                    commits: vec![],
                    ..Default::default()
                },
            })
            .await
            .expect("Failed to create");

            db.create_history_entry(CreateHistoryInput {
                feature_id: feature2.id,
                version_id: None,
                details: HistoryDetails {
                    summary: "Work in project 2".to_string(),
                    commits: vec![],
                    ..Default::default()
                },
            })
            .await
            .expect("Failed to create");

            let history1 = db
                .get_project_history(project1.id, None, None, None, None)
                .await
                .expect("Query failed");
            assert_eq!(history1.len(), 1);
            assert_eq!(history1[0].summary, "Work in project 1");

            let history2 = db
                .get_project_history(project2.id, None, None, None, None)
                .await
                .expect("Query failed");
            assert_eq!(history2.len(), 1);
            assert_eq!(history2[0].summary, "Work in project 2");
        }
    }
}
