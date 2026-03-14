mod common;

use common::*;
use manifest::models::*;

// ============================================================
// Features
// ============================================================

mod features {
    use super::*;

    mod create_feature {
        use super::*;

        #[tokio::test]
        async fn creates_feature_with_required_fields() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let input = CreateFeatureInput {
                id: None,
                parent_id: None,
                title: "User Login".to_string(),
                details: None,
                priority: None,
                target_version_id: None,
                state: None,
            };

            let feature = db
                .create_feature(project.id, input)
                .await
                .expect("Failed to create feature");

            assert_eq!(feature.title, "User Login");
            assert_eq!(feature.project_id, project.id);
            assert_eq!(feature.state, FeatureState::Proposed);
        }

        #[tokio::test]
        async fn creates_feature_with_all_fields() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let input = CreateFeatureInput {
                id: None,
                parent_id: None,
                title: "OAuth Integration".to_string(),
                details: Some("As a user, I want to log in with OAuth.\n\n## Technical Notes\n\nUse PKCE flow".to_string()),
                state: Some(FeatureState::Implemented),
                priority: None,
                target_version_id: None,
            };

            let feature = db
                .create_feature(project.id, input)
                .await
                .expect("Failed to create feature");

            assert_eq!(feature.title, "OAuth Integration");
            assert_eq!(feature.state, FeatureState::Implemented);
            assert!(feature.details.as_ref().unwrap().contains("As a user"));
            assert!(feature.details.as_ref().unwrap().contains("PKCE"));
        }
    }

    mod get_feature {
        use super::*;

        #[tokio::test]
        async fn returns_none_for_nonexistent_feature() {
            let db = setup().await;
            let result = db
                .get_feature(FeatureId::new())
                .await
                .expect("Query failed");
            assert!(result.is_none());
        }

        #[tokio::test]
        async fn returns_feature_by_id() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let input = CreateFeatureInput {
                id: None,
                parent_id: None,
                title: "Rate Limiting".to_string(),
                details: None,
                priority: None,
                target_version_id: None,
                state: None,
            };
            let created = db
                .create_feature(project.id, input)
                .await
                .expect("Failed to create");

            let found = db.get_feature(created.id).await.expect("Query failed");

            assert!(found.is_some());
            assert_eq!(found.unwrap().title, "Rate Limiting");
        }
    }

    mod get_all_features {
        use super::*;

        #[tokio::test]
        async fn returns_empty_list_when_no_features() {
            let db = setup().await;
            let features = db.get_all_features().await.expect("Query failed");
            assert!(features.is_empty());
        }

        #[tokio::test]
        async fn returns_all_features_ordered_by_title() {
            let db = setup().await;
            let project = create_test_project(&db).await;

            db.create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Zebra Feature".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create");

            db.create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Alpha Feature".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create");

            let features = db.get_all_features().await.expect("Query failed");

            // With the root feature model, each project has a root feature
            // So we have: project root + 2 created = 3 features
            assert_eq!(features.len(), 3);
            assert_eq!(features[0].title, "Alpha Feature");
            assert_eq!(features[1].title, "Test Project"); // Root feature has project name
            assert_eq!(features[2].title, "Zebra Feature");
        }
    }

    mod update_feature {
        use super::*;

        #[tokio::test]
        async fn returns_none_for_nonexistent_feature() {
            let db = setup().await;
            let input = UpdateFeatureInput {
                parent_id: None,
                title: Some("New Title".to_string()),
                details: None,
                desired_details: None,
                details_summary: None,
                priority: None,
                target_version_id: None,
                state: None,
                blocked_by: None,
            };

            let result = db
                .update_feature(FeatureId::new(), input)
                .await
                .expect("Query failed");
            assert!(result.is_none());
        }

        #[tokio::test]
        async fn updates_only_provided_fields() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let created = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Original Title".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: Some(FeatureState::Proposed),
                    },
                )
                .await
                .expect("Failed to create");

            let updated = db
                .update_feature(
                    created.id,
                    UpdateFeatureInput {
                        parent_id: None,
                        title: Some("Updated Title".to_string()),
                        details: None,
                        desired_details: None,
                        details_summary: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                        blocked_by: None,
                    },
                )
                .await
                .expect("Query failed")
                .expect("Feature not found");

            assert_eq!(updated.title, "Updated Title");
            assert_eq!(updated.state, FeatureState::Proposed);
        }

        #[tokio::test]
        async fn transitions_feature_state() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let created = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Feature".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: Some(FeatureState::Proposed),
                    },
                )
                .await
                .expect("Failed to create");

            let updated = db
                .update_feature(
                    created.id,
                    UpdateFeatureInput {
                        parent_id: None,
                        title: None,
                        details: None,
                        desired_details: None,
                        details_summary: None,
                        priority: None,
                        target_version_id: None,
                        state: Some(FeatureState::Implemented),
                        blocked_by: None,
                    },
                )
                .await
                .expect("Query failed")
                .expect("Feature not found");

            assert_eq!(updated.state, FeatureState::Implemented);
        }
    }

    mod delete_feature {
        use super::*;

        #[tokio::test]
        async fn returns_false_for_nonexistent_feature() {
            let db = setup().await;
            let result = db
                .delete_feature(FeatureId::new())
                .await
                .expect("Query failed");
            assert!(!result);
        }

        #[tokio::test]
        async fn deletes_feature_and_returns_true() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let created = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "To Delete".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            let deleted = db.delete_feature(created.id).await.expect("Query failed");
            assert!(deleted);

            let found = db.get_feature(created.id).await.expect("Query failed");
            assert!(found.is_none());
        }
    }

    mod get_feature_diff {
        use super::*;

        #[tokio::test]
        async fn returns_has_changes_false_when_no_desired_details() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let feature = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Feature".to_string(),
                        details: Some("Current details".to_string()),
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            let diff = db
                .get_feature_diff(feature.id)
                .await
                .expect("Query failed")
                .unwrap();
            assert!(!diff.has_changes);
            assert_eq!(diff.current, Some("Current details".to_string()));
            assert!(diff.desired.is_none());
        }

        #[tokio::test]
        async fn returns_has_changes_true_when_desired_details_differs() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let feature = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Feature".to_string(),
                        details: Some("Current".to_string()),
                        priority: None,
                        target_version_id: None,
                        state: Some(FeatureState::Implemented),
                    },
                )
                .await
                .expect("Failed to create");

            db.update_feature(
                feature.id,
                UpdateFeatureInput {
                    parent_id: None,
                    title: None,
                    details: None,
                    desired_details: Some(Some("Desired".to_string())),
                    details_summary: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                    blocked_by: None,
                },
            )
            .await
            .expect("Failed to update");

            let diff = db
                .get_feature_diff(feature.id)
                .await
                .expect("Query failed")
                .unwrap();
            assert!(diff.has_changes);
            assert_eq!(diff.current, Some("Current".to_string()));
            assert_eq!(diff.desired, Some("Desired".to_string()));
        }

        #[tokio::test]
        async fn returns_none_for_nonexistent_feature() {
            let db = setup().await;
            let result = db
                .get_feature_diff(FeatureId::new())
                .await
                .expect("Query failed");
            assert!(result.is_none());
        }
    }

    mod desired_details {
        use super::*;

        #[tokio::test]
        async fn stores_and_retrieves_desired_details_on_implemented_feature() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let feature = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Feature".to_string(),
                        details: Some("Current".to_string()),
                        priority: None,
                        target_version_id: None,
                        state: Some(FeatureState::Implemented),
                    },
                )
                .await
                .expect("Failed to create");

            let updated = db
                .update_feature(
                    feature.id,
                    UpdateFeatureInput {
                        parent_id: None,
                        title: None,
                        details: None,
                        desired_details: Some(Some("Desired".to_string())),
                        details_summary: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                        blocked_by: None,
                    },
                )
                .await
                .expect("Failed to update")
                .unwrap();

            assert_eq!(updated.details, Some("Current".to_string()));
            assert_eq!(updated.desired_details, Some("Desired".to_string()));
        }

        #[tokio::test]
        async fn redirects_desired_details_to_details_on_non_implemented_feature() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let feature = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Feature".to_string(),
                        details: Some("Original".to_string()),
                        priority: None,
                        target_version_id: None,
                        state: None, // proposed
                    },
                )
                .await
                .expect("Failed to create");

            let updated = db
                .update_feature(
                    feature.id,
                    UpdateFeatureInput {
                        parent_id: None,
                        title: None,
                        details: None,
                        desired_details: Some(Some("New spec".to_string())),
                        details_summary: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                        blocked_by: None,
                    },
                )
                .await
                .expect("Failed to update")
                .unwrap();

            // desired_details should be redirected to details
            assert_eq!(updated.details, Some("New spec".to_string()));
            assert!(updated.desired_details.is_none());
        }
    }

    mod search_features {
        use super::*;

        #[tokio::test]
        async fn returns_empty_list_when_no_matches() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            db.create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "User Login".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create");

            let results = db
                .search_features("nonexistent", None, None)
                .await
                .expect("Query failed");
            assert!(results.is_empty());
        }

        #[tokio::test]
        async fn matches_title_case_insensitively() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            db.create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "User Authentication".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create");

            let results = db
                .search_features("user", None, None)
                .await
                .expect("Query failed");
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].title, "User Authentication");

            let results = db
                .search_features("USER", None, None)
                .await
                .expect("Query failed");
            assert_eq!(results.len(), 1);
        }

        #[tokio::test]
        async fn matches_details_content() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            db.create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "OAuth Integration".to_string(),
                    details: Some("Implement Google OAuth using PKCE flow".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create");

            let results = db
                .search_features("PKCE", None, None)
                .await
                .expect("Query failed");
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].title, "OAuth Integration");
        }

        #[tokio::test]
        async fn ranks_title_matches_before_details_matches() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            db.create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "User Login".to_string(),
                    details: Some("Some login details".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create");

            db.create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "OAuth Flow".to_string(),
                    details: Some("User must click login button".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create");

            let results = db
                .search_features("login", None, None)
                .await
                .expect("Query failed");
            assert_eq!(results.len(), 2);
            // "User Login" should be first (title match)
            assert_eq!(results[0].title, "User Login");
            // "OAuth Flow" should be second (details match)
            assert_eq!(results[1].title, "OAuth Flow");
        }

        #[tokio::test]
        async fn filters_by_project_id() {
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
                .expect("Failed to create project");

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
                .expect("Failed to create project");

            db.create_feature(
                project1.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Auth Feature".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create");

            db.create_feature(
                project2.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Auth in Project 2".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create");

            let results = db
                .search_features("Auth", Some(project1.id), None)
                .await
                .expect("Query failed");
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].title, "Auth Feature");
        }

        #[tokio::test]
        async fn respects_limit_parameter() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            for i in 1..=5 {
                db.create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: format!("Feature {}", i),
                        details: None,
                        priority: None,
                        state: None,
                        target_version_id: None,
                    },
                )
                .await
                .expect("Failed to create");
            }

            let results = db
                .search_features("Feature", None, Some(2))
                .await
                .expect("Query failed");
            assert_eq!(results.len(), 2);
        }

        #[tokio::test]
        async fn defaults_limit_to_10() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            for i in 1..=15 {
                db.create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: format!("Feature {}", i),
                        details: None,
                        priority: None,
                        state: None,
                        target_version_id: None,
                    },
                )
                .await
                .expect("Failed to create");
            }

            let results = db
                .search_features("Feature", None, None)
                .await
                .expect("Query failed");
            assert_eq!(results.len(), 10);
        }

        #[tokio::test]
        async fn returns_feature_summary_not_full_feature() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let feature = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Unique Feature Title".to_string(),
                        details: Some("Detailed description".to_string()),
                        priority: Some(5),
                        target_version_id: None,
                        state: Some(FeatureState::Implemented),
                    },
                )
                .await
                .expect("Failed to create");

            // Search for something specific to avoid matching root feature ("Test Project")
            let results = db
                .search_features("Unique Feature", None, None)
                .await
                .expect("Query failed");
            assert_eq!(results.len(), 1);

            let summary = &results[0];
            assert_eq!(summary.id, feature.id);
            assert_eq!(summary.title, "Unique Feature Title");
            assert_eq!(summary.state, FeatureState::Implemented);
            assert_eq!(summary.priority, 5);
            // FeatureSummary doesn't have details field - that's the point!
        }
    }
}
