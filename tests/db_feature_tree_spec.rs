mod common;

use common::*;
use manifest::models::*;
use uuid::Uuid;

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
