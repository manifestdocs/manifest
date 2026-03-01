mod common;

use common::*;
use manifest::db::Database;
use manifest::models::*;

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

        let result = db
            .complete_feature(feature.id, "Implemented the feature", &[])
            .await
            .unwrap();
        let completed = result.feature;
        let history = result.history;

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

        let _result = db.complete_feature(feature.id, "Done", &[]).await.unwrap();

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

// ============================================================
// Proof Gate on complete_feature
// ============================================================

mod proof_gate {
    use super::*;

    /// Helper: create a project with a given testing policy and a leaf feature under root.
    async fn setup_with_policy(db: &Database, policy: TestingPolicy) -> (Project, Feature) {
        let project = create_test_project(db).await;
        // Update project to the desired testing policy
        db.update_project(
            project.id,
            UpdateProjectInput {
                name: None,
                slug: None,
                description: None,
                instructions: None,
                current_version_id: None,
                default_feature_destination: None,
                detail_level: None,
                ac_level: None,
                ac_format: None,
                testing_policy: Some(policy),
                test_adapter: None,
                key_prefix: None,
            },
        )
        .await
        .unwrap();

        let root = db
            .get_feature(project.root_feature_id.unwrap())
            .await
            .unwrap()
            .unwrap();
        let feature = db
            .create_feature(
                project.id,
                CreateFeatureInput {
                    id: None,
                    parent_id: Some(root.id),
                    title: "Provable Feature".to_string(),
                    details: Some("Spec for provable feature".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: Some(FeatureState::InProgress),
                },
            )
            .await
            .unwrap();
        (project, feature)
    }

    #[tokio::test]
    async fn complete_feature_with_tdd_policy_requires_passing_proof() {
        let db = setup().await;
        let (_project, feature) = setup_with_policy(&db, TestingPolicy::Tdd).await;

        // Try to complete without any proof — should be rejected
        let result = db
            .complete_feature(feature.id, "Done without proof", &[])
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no proof recorded"), "Error: {err}");
    }

    #[tokio::test]
    async fn complete_feature_with_tdd_policy_rejects_failing_proof() {
        let db = setup().await;
        let (_project, feature) = setup_with_policy(&db, TestingPolicy::Tdd).await;

        // Create a failing proof (exit_code != 0)
        db.create_proof(CreateProofInput {
            feature_id: feature.id,
            history_id: None,
            command: "cargo test".to_string(),
            exit_code: 1,
            output: Some("test failed".to_string()),
            test_suites: None,
            evidence: vec![],
            commit_sha: None,
            agent_type: Some("claude".to_string()),
        })
        .await
        .unwrap();

        // Try to complete with failing proof — should be rejected
        let result = db
            .complete_feature(feature.id, "Done with failing tests", &[])
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("failing tests"), "Error: {err}");
    }

    #[tokio::test]
    async fn complete_feature_with_tdd_policy_succeeds_with_passing_proof() {
        let db = setup().await;
        let (_project, feature) = setup_with_policy(&db, TestingPolicy::Tdd).await;

        // Create a passing proof (exit_code == 0)
        db.create_proof(CreateProofInput {
            feature_id: feature.id,
            history_id: None,
            command: "cargo test".to_string(),
            exit_code: 0,
            output: Some("all tests passed".to_string()),
            test_suites: None,
            evidence: vec![],
            commit_sha: None,
            agent_type: Some("claude".to_string()),
        })
        .await
        .unwrap();

        // Complete should succeed
        let result = db
            .complete_feature(feature.id, "Done with passing tests", &[])
            .await
            .unwrap();

        assert_eq!(result.feature.state, FeatureState::Implemented);
        assert_eq!(result.history.details.summary, "Done with passing tests");
    }

    #[tokio::test]
    async fn complete_feature_with_advisory_policy_succeeds_without_proof() {
        let db = setup().await;
        let (_project, feature) = setup_with_policy(&db, TestingPolicy::Advisory).await;

        // Complete without proof — advisory should not block (but should warn)
        let result = db
            .complete_feature(feature.id, "Done without proof (advisory)", &[])
            .await
            .unwrap();

        assert_eq!(result.feature.state, FeatureState::Implemented);
        assert!(
            !result.warnings.is_empty(),
            "advisory should produce warnings when proof is missing"
        );
    }

    #[tokio::test]
    async fn complete_feature_with_none_policy_succeeds_without_proof() {
        let db = setup().await;
        let (_project, feature) = setup_with_policy(&db, TestingPolicy::None).await;

        // Complete without proof — none policy should not block or warn about proof
        let result = db
            .complete_feature(feature.id, "Done without proof (none)", &[])
            .await
            .unwrap();

        assert_eq!(result.feature.state, FeatureState::Implemented);
    }
}
