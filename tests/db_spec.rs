use chrono::Utc;
use manifest::db::Database;
use manifest::models::*;
use uuid::Uuid;

async fn setup() -> Database {
    let db = Database::open_memory()
        .await
        .expect("Failed to create in-memory database");
    db.migrate().await.expect("Failed to run migrations");
    db
}

async fn create_test_project(db: &Database) -> Project {
    db.create_project(CreateProjectInput {
        slug: None,
        name: "Test Project".to_string(),
        description: None,
        instructions: None,
        key_prefix: None,
    })
    .await
    .expect("Failed to create project")
}

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
                    slug: None,
                    name: "Project 1".to_string(),
                    description: None,
                    instructions: None,
                    key_prefix: None,
                })
                .await
                .expect("Failed to create project");

            let project2 = db
                .create_project(CreateProjectInput {
                    slug: None,
                    name: "Project 2".to_string(),
                    description: None,
                    instructions: None,
                    key_prefix: None,
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

// ============================================================
// Feature Hierarchy
// ============================================================

mod feature_hierarchy {
    use super::*;

    mod nested_features {
        use super::*;

        #[tokio::test]
        async fn creates_child_feature_under_parent() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let parent = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Authentication".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create parent");

            let child = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: Some(parent.id),
                        title: "Login".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create child");

            assert_eq!(child.parent_id, Some(parent.id));
        }

        #[tokio::test]
        async fn creates_deeply_nested_features() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let root = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Authentication".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            let level1 = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: Some(root.id),
                        title: "OAuth".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            let level2 = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: Some(level1.id),
                        title: "Google".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            assert_eq!(level2.parent_id, Some(level1.id));

            let found = db
                .get_feature(level2.id)
                .await
                .expect("Query failed")
                .unwrap();
            assert_eq!(found.parent_id, Some(level1.id));
        }
    }

    mod get_root_features {
        use super::*;

        #[tokio::test]
        async fn returns_only_features_without_parents() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let root1 = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Root 1".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            let _root2 = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Root 2".to_string(),
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
                    parent_id: Some(root1.id),
                    title: "Child".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create");

            let roots = db
                .get_root_features(project.id)
                .await
                .expect("Query failed");

            assert_eq!(roots.len(), 2);
            // With the root feature model, "root" features are children of the project's
            // root_feature, so they have parent_id = root_feature_id (not None)
            assert!(roots.iter().all(|f| f.parent_id == project.root_feature_id));
        }
    }

    mod get_children {
        use super::*;

        #[tokio::test]
        async fn returns_empty_list_when_feature_has_no_children() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let leaf = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Leaf".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            let children = db.get_children(leaf.id).await.expect("Query failed");
            assert!(children.is_empty());
        }

        #[tokio::test]
        async fn returns_direct_children_ordered_by_title() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let parent = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Parent".to_string(),
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
                    parent_id: Some(parent.id),
                    title: "Zebra Child".to_string(),
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
                    parent_id: Some(parent.id),
                    title: "Alpha Child".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create");

            let children = db.get_children(parent.id).await.expect("Query failed");

            assert_eq!(children.len(), 2);
            assert_eq!(children[0].title, "Alpha Child");
            assert_eq!(children[1].title, "Zebra Child");
        }

        #[tokio::test]
        async fn does_not_return_grandchildren() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let parent = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Parent".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            let child = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: Some(parent.id),
                        title: "Child".to_string(),
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
                    parent_id: Some(child.id),
                    title: "Grandchild".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create");

            let children = db.get_children(parent.id).await.expect("Query failed");
            assert_eq!(children.len(), 1);
            assert_eq!(children[0].title, "Child");
        }
    }

    mod is_leaf {
        use super::*;

        #[tokio::test]
        async fn returns_true_for_feature_with_no_children() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let leaf = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Leaf".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            assert!(db.is_leaf(leaf.id).await.expect("Query failed"));
        }

        #[tokio::test]
        async fn returns_false_for_feature_with_children() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let parent = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Parent".to_string(),
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
                    parent_id: Some(parent.id),
                    title: "Child".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create");

            assert!(!db.is_leaf(parent.id).await.expect("Query failed"));
        }
    }

    mod cascade_delete {
        use super::*;

        #[tokio::test]
        async fn deletes_children_when_parent_is_deleted() {
            let db = setup().await;
            let project = create_test_project(&db).await;
            let parent = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: None,
                        title: "Parent".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            let child = db
                .create_feature(
                    project.id,
                    CreateFeatureInput {
                        id: None,
                        parent_id: Some(parent.id),
                        title: "Child".to_string(),
                        details: None,
                        priority: None,
                        target_version_id: None,
                        state: None,
                    },
                )
                .await
                .expect("Failed to create");

            db.delete_feature(parent.id)
                .await
                .expect("Failed to delete");

            let found = db.get_feature(child.id).await.expect("Query failed");
            assert!(found.is_none());
        }
    }
}

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
                    slug: None,
                    name: "Project 1".to_string(),
                    description: None,
                    instructions: None,
                    key_prefix: None,
                })
                .await
                .expect("Failed to create");

            let project2 = db
                .create_project(CreateProjectInput {
                    slug: None,
                    name: "Project 2".to_string(),
                    description: None,
                    instructions: None,
                    key_prefix: None,
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

// ============================================================
// Derived Parent State
// ============================================================

mod derived_parent_state {
    use super::*;

    #[tokio::test]
    async fn parent_with_all_proposed_children_returns_proposed() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let parent = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Auth".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create parent");

        db.create_feature(
            project.id,
            CreateFeatureInput {
                id: None,
                parent_id: Some(parent.id),
                title: "Login".to_string(),
                details: None,
                priority: None,
                target_version_id: None,
                state: Some(FeatureState::Proposed),
            },
        )
        .await
        .expect("Failed to create child");

        db.create_feature(
            project.id,
            CreateFeatureInput {
                id: None,
                parent_id: Some(parent.id),
                title: "OAuth".to_string(),
                details: None,
                priority: None,
                target_version_id: None,
                state: Some(FeatureState::Proposed),
            },
        )
        .await
        .expect("Failed to create child");

        let fetched = db
            .get_feature(parent.id)
            .await
            .expect("Query failed")
            .expect("Feature not found");

        assert_eq!(fetched.state, FeatureState::Proposed);
    }

    #[tokio::test]
    async fn parent_with_all_implemented_children_returns_implemented() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let parent = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Auth".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create parent");

        db.create_feature(
            project.id,
            CreateFeatureInput {
                id: None,
                parent_id: Some(parent.id),
                title: "Login".to_string(),
                details: None,
                priority: None,
                target_version_id: None,
                state: Some(FeatureState::Implemented),
            },
        )
        .await
        .expect("Failed to create child");

        db.create_feature(
            project.id,
            CreateFeatureInput {
                id: None,
                parent_id: Some(parent.id),
                title: "OAuth".to_string(),
                details: None,
                priority: None,
                target_version_id: None,
                state: Some(FeatureState::Implemented),
            },
        )
        .await
        .expect("Failed to create child");

        let fetched = db
            .get_feature(parent.id)
            .await
            .expect("Query failed")
            .expect("Feature not found");

        assert_eq!(fetched.state, FeatureState::Implemented);
    }

    #[tokio::test]
    async fn parent_with_mixed_children_returns_in_progress() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let parent = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Auth".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create parent");

        db.create_feature(
            project.id,
            CreateFeatureInput {
                id: None,
                parent_id: Some(parent.id),
                title: "Login".to_string(),
                details: None,
                priority: None,
                target_version_id: None,
                state: Some(FeatureState::Implemented),
            },
        )
        .await
        .expect("Failed to create child");

        db.create_feature(
            project.id,
            CreateFeatureInput {
                id: None,
                parent_id: Some(parent.id),
                title: "OAuth".to_string(),
                details: None,
                priority: None,
                target_version_id: None,
                state: Some(FeatureState::Proposed),
            },
        )
        .await
        .expect("Failed to create child");

        let fetched = db
            .get_feature(parent.id)
            .await
            .expect("Query failed")
            .expect("Feature not found");

        assert_eq!(fetched.state, FeatureState::InProgress);
    }

    #[tokio::test]
    async fn feature_tree_reflects_derived_states() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let parent = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Auth".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create parent");

        db.create_feature(
            project.id,
            CreateFeatureInput {
                id: None,
                parent_id: Some(parent.id),
                title: "Login".to_string(),
                details: None,
                priority: None,
                target_version_id: None,
                state: Some(FeatureState::Implemented),
            },
        )
        .await
        .expect("Failed to create child");

        db.create_feature(
            project.id,
            CreateFeatureInput {
                id: None,
                parent_id: Some(parent.id),
                title: "OAuth".to_string(),
                details: None,
                priority: None,
                target_version_id: None,
                state: Some(FeatureState::Implemented),
            },
        )
        .await
        .expect("Failed to create child");

        let tree = db
            .get_feature_tree(project.id)
            .await
            .expect("Failed to get tree");

        // Find Auth node in tree (should be under root)
        let root = &tree[0];
        let auth = root
            .children
            .iter()
            .find(|n| n.feature.title == "Auth")
            .expect("Auth node not found");

        assert_eq!(auth.feature.state, FeatureState::Implemented);
    }

    #[tokio::test]
    async fn get_feature_with_context_reflects_derived_state() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let parent = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Auth".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .expect("Failed to create parent");

        db.create_feature(
            project.id,
            CreateFeatureInput {
                id: None,
                parent_id: Some(parent.id),
                title: "Login".to_string(),
                details: None,
                priority: None,
                target_version_id: None,
                state: Some(FeatureState::Implemented),
            },
        )
        .await
        .expect("Failed to create child");

        let ctx = db
            .get_feature_with_context(parent.id)
            .await
            .expect("Query failed")
            .expect("Feature not found");

        assert_eq!(ctx.feature.state, FeatureState::Implemented);
    }

    #[tokio::test]
    async fn leaf_feature_keeps_db_state() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let leaf = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Standalone".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: Some(FeatureState::Proposed),
                },
            )
            .await
            .expect("Failed to create leaf");

        let fetched = db
            .get_feature(leaf.id)
            .await
            .expect("Query failed")
            .expect("Feature not found");

        assert_eq!(fetched.state, FeatureState::Proposed);
    }

    #[tokio::test]
    async fn parent_with_blocked_children_treated_as_proposed() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let parent = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Parent".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .unwrap();

        let child_a = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: Some(parent.id),
                    title: "Child A".to_string(),
                    details: Some("Spec A".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .unwrap();
        db.create_feature(
            project.id,
            CreateFeatureInput {
                id: None,
                parent_id: Some(parent.id),
                title: "Child B".to_string(),
                details: Some("Spec B".to_string()),
                priority: None,
                target_version_id: None,
                state: None,
            },
        )
        .await
        .unwrap();

        // Block child_a — parent should remain proposed (blocked treated as proposed)
        db.update_feature(
            child_a.id,
            UpdateFeatureInput {
                parent_id: None,
                title: None,
                details: None,
                desired_details: None,
                details_summary: None,
                state: Some(FeatureState::Blocked),
                priority: None,
                target_version_id: None,
                blocked_by: Some(vec![parent.id]), // block by parent for testing
            },
        )
        .await
        .unwrap();

        let fetched = db.get_feature(parent.id).await.unwrap().unwrap();

        // Blocked children are treated like proposed for derivation
        assert_eq!(fetched.state, FeatureState::Proposed);
    }

    #[tokio::test]
    async fn parent_with_blocked_and_implemented_children() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let root = db
            .get_feature(project.root_feature_id.unwrap())
            .await
            .unwrap()
            .unwrap();

        let parent = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: Some(root.id),
                    title: "Parent".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .unwrap();

        let blocker = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: Some(root.id),
                    title: "Blocker".to_string(),
                    details: Some("Spec".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .unwrap();

        let child_a = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: Some(parent.id),
                    title: "Child A".to_string(),
                    details: Some("Spec A".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: Some(FeatureState::Implemented),
                },
            )
            .await
            .unwrap();
        let child_b = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: Some(parent.id),
                    title: "Child B".to_string(),
                    details: Some("Spec B".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .unwrap();

        // Block child_b by blocker
        db.update_feature(
            child_b.id,
            UpdateFeatureInput {
                parent_id: None,
                title: None,
                details: None,
                desired_details: None,
                details_summary: None,
                state: Some(FeatureState::Blocked),
                priority: None,
                target_version_id: None,
                blocked_by: Some(vec![blocker.id]),
            },
        )
        .await
        .unwrap();

        // Parent has one implemented + one blocked → should be in_progress
        // (blocked is neither Implemented nor InProgress, but there's an implemented child)
        let fetched = db.get_feature(parent.id).await.unwrap().unwrap();

        // Blocked = not implemented, so with implemented + blocked:
        // any_implemented = true, all_implemented = false → InProgress
        assert_eq!(fetched.state, FeatureState::InProgress);

        let _ = child_a;
    }
}

// ============================================================
// Data Resilience
// ============================================================

mod data_resilience {
    use super::*;

    #[tokio::test]
    async fn returns_error_for_corrupted_uuid_in_feature() {
        let db = setup().await;
        let project = create_test_project(&db).await;

        // Insert a feature with a corrupt UUID via raw SQL
        sqlx::query(
            "INSERT INTO features (id, project_id, title, state, priority, created_at, updated_at)
             VALUES ('not-a-uuid', ?1, 'Corrupt Feature', 'proposed', 0, datetime('now'), datetime('now'))",
        )
        .bind(project.id.to_string())
        .execute(db.pool())
        .await
        .expect("Raw insert should succeed");

        // Reading all features should return an error, not panic
        let result = db.get_features_by_project(project.id).await;
        assert!(result.is_err(), "Expected error for corrupted UUID, got Ok");
    }

    #[tokio::test]
    async fn returns_error_for_corrupted_datetime_in_feature() {
        let db = setup().await;
        let project = create_test_project(&db).await;

        // Insert a feature with a corrupt datetime via raw SQL
        sqlx::query(
            "INSERT INTO features (id, project_id, title, state, priority, created_at, updated_at)
             VALUES (?1, ?2, 'Corrupt Dates', 'proposed', 0, 'not-a-date', 'also-not-a-date')",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(project.id.to_string())
        .execute(db.pool())
        .await
        .expect("Raw insert should succeed");

        // Reading should return an error, not panic
        let result = db.get_features_by_project(project.id).await;
        assert!(
            result.is_err(),
            "Expected error for corrupted datetime, got Ok"
        );
    }

    #[tokio::test]
    async fn foreign_key_enforcement_rejects_orphan_feature() {
        let db = setup().await;
        let fake_project_id = Uuid::new_v4();

        // Inserting a feature with a non-existent project_id should fail
        // because PRAGMA foreign_keys = ON is set pool-wide
        let result = sqlx::query(
            "INSERT INTO features (id, project_id, title, state, priority, created_at, updated_at)
             VALUES (?1, ?2, 'Orphan', 'proposed', 0, datetime('now'), datetime('now'))",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(fake_project_id.to_string())
        .execute(db.pool())
        .await;

        assert!(result.is_err(), "Expected FK violation error, got Ok");
    }
}

// ============================================================
// Blocked Features
// ============================================================

mod blocked_features {
    use super::*;

    /// Helper: create a project with two leaf features under a parent.
    async fn setup_two_features(db: &Database) -> (Project, Feature, Feature, Feature) {
        let project = create_test_project(db).await;
        let root = db
            .get_feature(project.root_feature_id.unwrap())
            .await
            .unwrap()
            .unwrap();
        let feature_a = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: Some(root.id),
                    title: "Feature A".to_string(),
                    details: Some("Spec for A".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .unwrap();
        let feature_b = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: Some(root.id),
                    title: "Feature B".to_string(),
                    details: Some("Spec for B".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .unwrap();
        (project, root, feature_a, feature_b)
    }

    #[tokio::test]
    async fn block_proposed_feature_with_blocker() {
        let db = setup().await;
        let (_project, _root, feature_a, feature_b) = setup_two_features(&db).await;

        // Block B by A
        let updated = db
            .update_feature(
                feature_b.id,
                UpdateFeatureInput {
                    parent_id: None,
                    title: None,
                    details: None,
                    desired_details: None,
                    details_summary: None,
                    state: Some(FeatureState::Blocked),
                    priority: None,
                    target_version_id: None,
                    blocked_by: Some(vec![feature_a.id]),
                },
            )
            .await
            .expect("Failed to block feature")
            .expect("Feature not found");

        assert_eq!(updated.state, FeatureState::Blocked);

        // Verify blockers stored
        let blockers = db.get_feature_blockers(feature_b.id).await.unwrap();
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].id, feature_a.id);
    }

    #[tokio::test]
    async fn reject_blocking_non_proposed_feature() {
        let db = setup().await;
        let (_project, _root, feature_a, feature_b) = setup_two_features(&db).await;

        // Set B to in_progress first
        db.update_feature(
            feature_b.id,
            UpdateFeatureInput {
                parent_id: None,
                title: None,
                details: None,
                desired_details: None,
                details_summary: None,
                state: Some(FeatureState::InProgress),
                priority: None,
                target_version_id: None,
                blocked_by: None,
            },
        )
        .await
        .unwrap();

        // Try to block an in_progress feature — should fail
        let result = db
            .update_feature(
                feature_b.id,
                UpdateFeatureInput {
                    parent_id: None,
                    title: None,
                    details: None,
                    desired_details: None,
                    details_summary: None,
                    state: Some(FeatureState::Blocked),
                    priority: None,
                    target_version_id: None,
                    blocked_by: Some(vec![feature_a.id]),
                },
            )
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("proposed"));
    }

    #[tokio::test]
    async fn reject_blocking_without_blocker_ids() {
        let db = setup().await;
        let (_project, _root, _feature_a, feature_b) = setup_two_features(&db).await;

        let result = db
            .update_feature(
                feature_b.id,
                UpdateFeatureInput {
                    parent_id: None,
                    title: None,
                    details: None,
                    desired_details: None,
                    details_summary: None,
                    state: Some(FeatureState::Blocked),
                    priority: None,
                    target_version_id: None,
                    blocked_by: None,
                },
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn reject_blockers_from_different_project() {
        let db = setup().await;
        let (_project1, _root1, feature_a, _feature_b) = setup_two_features(&db).await;

        // Create a second project with a feature
        let project2 = db
            .create_project(CreateProjectInput {
                slug: None,
                name: "Other Project".to_string(),
                description: None,
                instructions: None,
                key_prefix: None,
            })
            .await
            .unwrap();
        let root2 = db
            .get_feature(project2.root_feature_id.unwrap())
            .await
            .unwrap()
            .unwrap();
        let other_feature = db
            .create_feature(
                project2.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: Some(root2.id),
                    title: "Other Feature".to_string(),
                    details: Some("Spec".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .unwrap();

        // Try to block a feature using a blocker from another project
        let result = db
            .update_feature(
                feature_a.id,
                UpdateFeatureInput {
                    parent_id: None,
                    title: None,
                    details: None,
                    desired_details: None,
                    details_summary: None,
                    state: Some(FeatureState::Blocked),
                    priority: None,
                    target_version_id: None,
                    blocked_by: Some(vec![other_feature.id]),
                },
            )
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("same project"));
    }

    #[tokio::test]
    async fn unblock_clears_blockers() {
        let db = setup().await;
        let (_project, _root, feature_a, feature_b) = setup_two_features(&db).await;

        // Block B by A
        db.update_feature(
            feature_b.id,
            UpdateFeatureInput {
                parent_id: None,
                title: None,
                details: None,
                desired_details: None,
                details_summary: None,
                state: Some(FeatureState::Blocked),
                priority: None,
                target_version_id: None,
                blocked_by: Some(vec![feature_a.id]),
            },
        )
        .await
        .unwrap();

        // Unblock: blocked -> proposed
        let updated = db
            .update_feature(
                feature_b.id,
                UpdateFeatureInput {
                    parent_id: None,
                    title: None,
                    details: None,
                    desired_details: None,
                    details_summary: None,
                    state: Some(FeatureState::Proposed),
                    priority: None,
                    target_version_id: None,
                    blocked_by: None,
                },
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(updated.state, FeatureState::Proposed);

        // Blockers should be cleared
        let blockers = db.get_feature_blockers(feature_b.id).await.unwrap();
        assert!(blockers.is_empty());
    }

    #[tokio::test]
    async fn auto_resolve_when_all_blockers_implemented() {
        let db = setup().await;
        let (_project, _root, feature_a, feature_b) = setup_two_features(&db).await;

        // Block B by A
        db.update_feature(
            feature_b.id,
            UpdateFeatureInput {
                parent_id: None,
                title: None,
                details: None,
                desired_details: None,
                details_summary: None,
                state: Some(FeatureState::Blocked),
                priority: None,
                target_version_id: None,
                blocked_by: Some(vec![feature_a.id]),
            },
        )
        .await
        .unwrap();

        // Implement A — should auto-resolve B
        db.update_feature(
            feature_a.id,
            UpdateFeatureInput {
                parent_id: None,
                title: None,
                details: None,
                desired_details: None,
                details_summary: None,
                state: Some(FeatureState::Implemented),
                priority: None,
                target_version_id: None,
                blocked_by: None,
            },
        )
        .await
        .unwrap();

        // B should now be proposed
        let b = db.get_feature(feature_b.id).await.unwrap().unwrap();
        assert_eq!(b.state, FeatureState::Proposed);
    }

    #[tokio::test]
    async fn no_auto_resolve_when_some_blockers_remain() {
        let db = setup().await;
        let (project, root, feature_a, feature_b) = setup_two_features(&db).await;

        // Create a third feature
        let feature_c = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: Some(root.id),
                    title: "Feature C".to_string(),
                    details: Some("Spec for C".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .unwrap();

        // Block C by both A and B
        db.update_feature(
            feature_c.id,
            UpdateFeatureInput {
                parent_id: None,
                title: None,
                details: None,
                desired_details: None,
                details_summary: None,
                state: Some(FeatureState::Blocked),
                priority: None,
                target_version_id: None,
                blocked_by: Some(vec![feature_a.id, feature_b.id]),
            },
        )
        .await
        .unwrap();

        // Implement only A — C should remain blocked (B is still proposed)
        db.update_feature(
            feature_a.id,
            UpdateFeatureInput {
                parent_id: None,
                title: None,
                details: None,
                desired_details: None,
                details_summary: None,
                state: Some(FeatureState::Implemented),
                priority: None,
                target_version_id: None,
                blocked_by: None,
            },
        )
        .await
        .unwrap();

        let c = db.get_feature(feature_c.id).await.unwrap().unwrap();
        assert_eq!(c.state, FeatureState::Blocked);
    }

    #[tokio::test]
    async fn block_feature_set() {
        let db = setup().await;
        let (project, root, feature_a, _feature_b) = setup_two_features(&db).await;

        // Create a feature set (parent with children)
        let group = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: Some(root.id),
                    title: "Auth Group".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .unwrap();
        db.create_feature(
            project.id,
            CreateFeatureInput {
                id: None,
                parent_id: Some(group.id),
                title: "Login".to_string(),
                details: Some("Login spec".to_string()),
                priority: None,
                target_version_id: None,
                state: None,
            },
        )
        .await
        .unwrap();

        // Block the feature set by feature_a
        let updated = db
            .update_feature(
                group.id,
                UpdateFeatureInput {
                    parent_id: None,
                    title: None,
                    details: None,
                    desired_details: None,
                    details_summary: None,
                    state: Some(FeatureState::Blocked),
                    priority: None,
                    target_version_id: None,
                    blocked_by: Some(vec![feature_a.id]),
                },
            )
            .await
            .expect("Should allow blocking feature sets")
            .expect("Feature not found");

        assert_eq!(updated.state, FeatureState::Blocked);
    }

    #[tokio::test]
    async fn find_blocked_ancestor() {
        let db = setup().await;
        let (project, root, feature_a, _feature_b) = setup_two_features(&db).await;

        // Create a group and a child
        let group = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: Some(root.id),
                    title: "Blocked Group".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .unwrap();
        let child = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: Some(group.id),
                    title: "Child Feature".to_string(),
                    details: Some("Spec".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                },
            )
            .await
            .unwrap();

        // Block the group
        db.update_feature(
            group.id,
            UpdateFeatureInput {
                parent_id: None,
                title: None,
                details: None,
                desired_details: None,
                details_summary: None,
                state: Some(FeatureState::Blocked),
                priority: None,
                target_version_id: None,
                blocked_by: Some(vec![feature_a.id]),
            },
        )
        .await
        .unwrap();

        // find_blocked_ancestor from child should find the group
        let ancestor = db.find_blocked_ancestor(child.id).await.unwrap();
        assert!(ancestor.is_some());
        let (ancestor_id, ancestor_title) = ancestor.unwrap();
        assert_eq!(ancestor_id, group.id);
        assert_eq!(ancestor_title, "Blocked Group");

        // find_blocked_ancestor from feature_a (not blocked) should return None
        let no_ancestor = db.find_blocked_ancestor(feature_a.id).await.unwrap();
        assert!(no_ancestor.is_none());
    }
}

// ============================================================
// Feature Claims
// ============================================================

mod feature_claims {
    use super::*;

    #[tokio::test]
    async fn set_claim_writes_agent_and_timestamp() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let feature = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Claimable Feature".to_string(),
                    details: Some("Some details".to_string()),
                    state: Some(FeatureState::InProgress),
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        // Initially no claim
        assert!(feature.claimed_by.is_none());
        assert!(feature.claimed_at.is_none());
        assert!(feature.claim_metadata.is_none());

        // Set claim
        db.set_feature_claim(feature.id, "claude", Some(r#"{"branch":"feature/test"}"#))
            .await
            .unwrap();

        // Re-fetch and verify
        let updated = db.get_feature(feature.id).await.unwrap().unwrap();
        assert_eq!(updated.claimed_by.as_deref(), Some("claude"));
        assert!(updated.claimed_at.is_some());
        assert_eq!(
            updated.claim_metadata.as_deref(),
            Some(r#"{"branch":"feature/test"}"#)
        );
    }

    #[tokio::test]
    async fn clear_claim_nulls_all_fields() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let feature = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Clearable Feature".to_string(),
                    details: Some("Some details".to_string()),
                    state: Some(FeatureState::InProgress),
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        // Set then clear claim
        db.set_feature_claim(feature.id, "gemini", None)
            .await
            .unwrap();
        db.clear_feature_claim(feature.id).await.unwrap();

        // Re-fetch and verify all cleared
        let updated = db.get_feature(feature.id).await.unwrap().unwrap();
        assert!(updated.claimed_by.is_none());
        assert!(updated.claimed_at.is_none());
        assert!(updated.claim_metadata.is_none());
    }

    #[tokio::test]
    async fn complete_feature_clears_claim_and_sets_implemented() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let feature = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Completable Feature".to_string(),
                    details: Some("Some details".to_string()),
                    state: Some(FeatureState::InProgress),
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        // Set claim then complete
        db.set_feature_claim(feature.id, "claude", Some(r#"{"branch":"feature/test"}"#))
            .await
            .unwrap();

        let (completed, history) = db
            .complete_feature(feature.id, "Implemented the feature", &[])
            .await
            .unwrap();

        // Feature state should be implemented
        assert_eq!(completed.state, FeatureState::Implemented);
        // Claims should be cleared
        assert!(completed.claimed_by.is_none());
        assert!(completed.claimed_at.is_none());
        assert!(completed.claim_metadata.is_none());
        // History should be recorded
        assert_eq!(history.details.summary, "Implemented the feature");
    }

    #[tokio::test]
    async fn complete_feature_emits_completed_event() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let feature = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Event Feature".to_string(),
                    details: Some("Some details".to_string()),
                    state: Some(FeatureState::InProgress),
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        db.set_feature_claim(feature.id, "codex", None)
            .await
            .unwrap();

        // Subscribe before completing
        let mut rx = db.subscribe();

        let (_completed, _history) = db.complete_feature(feature.id, "Done", &[]).await.unwrap();

        // Drain events — the last one should be Completed
        // (there may be Updated events before it from update_feature + clear_claim)
        let mut found_completed = false;
        while let Ok(event) = rx.try_recv() {
            if let manifest::db::FeatureEvent::Completed {
                feature_title,
                project_name,
                agent_type,
                ..
            } = event
            {
                assert_eq!(feature_title, "Event Feature");
                assert_eq!(project_name, "Test Project");
                assert_eq!(agent_type, Some("codex".to_string()));
                found_completed = true;
            }
        }
        assert!(found_completed, "Expected a Completed event");
    }

    #[tokio::test]
    async fn force_overrides_existing_claim() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let feature = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Contested Feature".to_string(),
                    details: Some("Some details".to_string()),
                    state: Some(FeatureState::InProgress),
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        // First claim
        db.set_feature_claim(feature.id, "claude", Some(r#"{"branch":"feature/a"}"#))
            .await
            .unwrap();

        // Override with second claim (force)
        db.set_feature_claim(feature.id, "gemini", Some(r#"{"branch":"feature/b"}"#))
            .await
            .unwrap();

        // Should show the new claim
        let updated = db.get_feature(feature.id).await.unwrap().unwrap();
        assert_eq!(updated.claimed_by.as_deref(), Some("gemini"));
        assert_eq!(
            updated.claim_metadata.as_deref(),
            Some(r#"{"branch":"feature/b"}"#)
        );
    }

    #[tokio::test]
    async fn atomic_claim_succeeds_on_proposed_feature() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let feature = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Atomic Claim Target".to_string(),
                    details: Some("Some details".to_string()),
                    state: Some(FeatureState::Proposed),
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        // Atomic claim should succeed and transition to in_progress
        let claimed = db
            .claim_feature_atomic(
                feature.id,
                "claude",
                Some(r#"{"branch":"feature/x"}"#),
                false,
            )
            .await
            .unwrap();

        assert_eq!(claimed.state, FeatureState::InProgress);
        assert_eq!(claimed.claimed_by.as_deref(), Some("claude"));
        assert!(claimed.claimed_at.is_some());
        assert_eq!(
            claimed.claim_metadata.as_deref(),
            Some(r#"{"branch":"feature/x"}"#)
        );
    }

    #[tokio::test]
    async fn atomic_claim_rejects_when_already_claimed() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let feature = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Already Claimed Feature".to_string(),
                    details: Some("Some details".to_string()),
                    state: Some(FeatureState::Proposed),
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        // First claim succeeds
        db.claim_feature_atomic(feature.id, "claude", None, false)
            .await
            .unwrap();

        // Second claim should fail with ClaimConflict
        let result = db
            .claim_feature_atomic(feature.id, "gemini", None, false)
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        let manifest_err = err.downcast_ref::<manifest::db::ManifestError>().unwrap();
        match manifest_err {
            manifest::db::ManifestError::ClaimConflict(info) => {
                assert_eq!(info.agent_type, "claude");
                assert_eq!(info.feature_id, feature.id.to_string());
                assert!(!info.claimed_at.is_empty());
            }
            other => panic!("Expected ClaimConflict, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn atomic_claim_force_overrides_existing_claim() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let feature = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Force Override Target".to_string(),
                    details: Some("Some details".to_string()),
                    state: Some(FeatureState::Proposed),
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        // First claim
        db.claim_feature_atomic(feature.id, "claude", Some(r#"{"branch":"a"}"#), false)
            .await
            .unwrap();

        // Second claim with force=true should succeed
        let overridden = db
            .claim_feature_atomic(feature.id, "gemini", Some(r#"{"branch":"b"}"#), true)
            .await
            .unwrap();

        assert_eq!(overridden.claimed_by.as_deref(), Some("gemini"));
        assert_eq!(
            overridden.claim_metadata.as_deref(),
            Some(r#"{"branch":"b"}"#)
        );
    }

    #[tokio::test]
    async fn atomic_claim_returns_not_found_for_missing_feature() {
        let db = setup().await;
        let fake_id = manifest::models::FeatureId::from(uuid::Uuid::new_v4());

        let result = db
            .claim_feature_atomic(fake_id, "claude", None, false)
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        let manifest_err = err.downcast_ref::<manifest::db::ManifestError>().unwrap();
        assert!(
            matches!(manifest_err, manifest::db::ManifestError::NotFound(_)),
            "Expected NotFound, got {:?}",
            manifest_err
        );
    }

    #[tokio::test]
    async fn atomic_claim_on_implemented_feature_transitions_to_in_progress() {
        let db = setup().await;
        let project = create_test_project(&db).await;
        let feature = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Re-startable Feature".to_string(),
                    details: Some("Some details".to_string()),
                    state: Some(FeatureState::Implemented),
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        let claimed = db
            .claim_feature_atomic(feature.id, "claude", None, false)
            .await
            .unwrap();

        assert_eq!(claimed.state, FeatureState::InProgress);
        assert_eq!(claimed.claimed_by.as_deref(), Some("claude"));
    }
}
