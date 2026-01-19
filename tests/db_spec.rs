use manifest::db::Database;
use manifest::models::*;
use speculate2::speculate;
use uuid::Uuid;

fn create_test_project(db: &Database) -> Project {
    db.create_project(CreateProjectInput {
        name: "Test Project".to_string(),
        description: None,
        instructions: None,
    })
    .expect("Failed to create project")
}

speculate! {
    before {
        let db = Database::open_memory().expect("Failed to create in-memory database");
        db.migrate().expect("Failed to run migrations");
    }

    describe "projects" {
        describe "create_project" {
            it "creates a project with required fields" {
                let project = db.create_project(CreateProjectInput {
                    name: "My Project".to_string(),
                    description: None,
                    instructions: None,
                }).expect("Failed to create project");

                assert_eq!(project.name, "My Project");
                assert!(project.description.is_none());
            }

            it "creates a project with all fields" {
                let project = db.create_project(CreateProjectInput {
                    name: "Full Project".to_string(),
                    description: Some("A complete project".to_string()),
                    instructions: Some("Use cargo test to run tests".to_string()),
                }).expect("Failed to create project");

                assert_eq!(project.name, "Full Project");
                assert_eq!(project.description, Some("A complete project".to_string()));
                assert_eq!(project.instructions, Some("Use cargo test to run tests".to_string()));
            }
        }

        describe "get_project" {
            it "returns None for non-existent project" {
                let result = db.get_project(Uuid::new_v4()).expect("Query failed");
                assert!(result.is_none());
            }

            it "returns the project by id" {
                let created = db.create_project(CreateProjectInput {
                    name: "Test".to_string(),
                    description: None,
                    instructions: None,
                }).expect("Failed to create");

                let found = db.get_project(created.id).expect("Query failed");
                assert!(found.is_some());
                assert_eq!(found.unwrap().name, "Test");
            }
        }

        describe "get_all_projects" {
            it "returns empty list when no projects exist" {
                let projects = db.get_all_projects().expect("Query failed");
                assert!(projects.is_empty());
            }

            it "returns all projects ordered by name" {
                db.create_project(CreateProjectInput {
                    name: "Zebra".to_string(),
                    description: None,
                    instructions: None,
                }).expect("Failed to create");

                db.create_project(CreateProjectInput {
                    name: "Alpha".to_string(),
                    description: None,
                    instructions: None,
                }).expect("Failed to create");

                let projects = db.get_all_projects().expect("Query failed");
                assert_eq!(projects.len(), 2);
                assert_eq!(projects[0].name, "Alpha");
                assert_eq!(projects[1].name, "Zebra");
            }
        }

        describe "delete_project" {
            it "deletes the project and cascades to features" {
                let project = create_test_project(&db);

                db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Feature".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create feature");

                db.delete_project(project.id).expect("Failed to delete");

                let features = db.get_features_by_project(project.id).expect("Query failed");
                assert!(features.is_empty());
            }
        }
    }

    describe "project_directories" {
        describe "add_project_directory" {
            it "adds a directory to a project" {
                let project = create_test_project(&db);

                let dir = db.add_project_directory(project.id, AddDirectoryInput {
                    path: "/home/user/project".to_string(),
                    git_remote: Some("git@github.com:user/project.git".to_string()),
                    is_primary: true,
                    instructions: Some("Run npm test".to_string()),
                }).expect("Failed to add directory");

                assert_eq!(dir.project_id, project.id);
                assert_eq!(dir.path, "/home/user/project");
                assert!(dir.is_primary);
                assert_eq!(dir.instructions, Some("Run npm test".to_string()));
            }
        }

        describe "get_project_directories" {
            it "returns directories ordered by is_primary then path" {
                let project = create_test_project(&db);

                db.add_project_directory(project.id, AddDirectoryInput {
                    path: "/b/path".to_string(),
                    git_remote: None,
                    is_primary: false,
                    instructions: None,
                }).expect("Failed");

                db.add_project_directory(project.id, AddDirectoryInput {
                    path: "/a/path".to_string(),
                    git_remote: None,
                    is_primary: true,
                    instructions: None,
                }).expect("Failed");

                let dirs = db.get_project_directories(project.id).expect("Query failed");
                assert_eq!(dirs.len(), 2);
                assert!(dirs[0].is_primary); // Primary first
                assert_eq!(dirs[1].path, "/b/path");
            }
        }
    }

    describe "features" {
        describe "create_feature" {
            it "creates a feature with required fields" {
                let project = create_test_project(&db);
                let input = CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "User Login".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                };

                let feature = db.create_feature(project.id, input).expect("Failed to create feature");

                assert_eq!(feature.title, "User Login");
                assert_eq!(feature.project_id, project.id);
                assert_eq!(feature.state, FeatureState::Proposed);
            }

            it "creates a feature with all fields" {
                let project = create_test_project(&db);
                let input = CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "OAuth Integration".to_string(),
                    details: Some("As a user, I want to log in with OAuth.\n\n## Technical Notes\n\nUse PKCE flow".to_string()),
                    state: Some(FeatureState::Implemented),
                    priority: None,
                    target_version_id: None,
                };

                let feature = db.create_feature(project.id, input).expect("Failed to create feature");

                assert_eq!(feature.title, "OAuth Integration");
                assert_eq!(feature.state, FeatureState::Implemented);
                assert!(feature.details.as_ref().unwrap().contains("As a user"));
                assert!(feature.details.as_ref().unwrap().contains("PKCE"));
            }
        }

        describe "get_feature" {
            it "returns None for non-existent feature" {
                let result = db.get_feature(Uuid::new_v4()).expect("Query failed");
                assert!(result.is_none());
            }

            it "returns the feature by id" {
                let project = create_test_project(&db);
                let input = CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Rate Limiting".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                };
                let created = db.create_feature(project.id, input).expect("Failed to create");

                let found = db.get_feature(created.id).expect("Query failed");

                assert!(found.is_some());
                assert_eq!(found.unwrap().title, "Rate Limiting");
            }
        }

        describe "get_all_features" {
            it "returns empty list when no features exist" {
                let features = db.get_all_features().expect("Query failed");
                assert!(features.is_empty());
            }

            it "returns all features ordered by title" {
                let project = create_test_project(&db);

                db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Zebra Feature".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Alpha Feature".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let features = db.get_all_features().expect("Query failed");

                // With the root feature model, each project has a root feature
                // So we have: project root + 2 created = 3 features
                assert_eq!(features.len(), 3);
                assert_eq!(features[0].title, "Alpha Feature");
                assert_eq!(features[1].title, "Test Project"); // Root feature has project name
                assert_eq!(features[2].title, "Zebra Feature");
            }
        }

        describe "update_feature" {
            it "returns None for non-existent feature" {
                let input = UpdateFeatureInput {
                    parent_id: None,
                    title: Some("New Title".to_string()),
                    details: None,
                    desired_details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                };

                let result = db.update_feature(Uuid::new_v4(), input).expect("Query failed");
                assert!(result.is_none());
            }

            it "updates only provided fields" {
                let project = create_test_project(&db);
                let created = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Original Title".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: Some(FeatureState::Proposed),
                }).expect("Failed to create");

                let updated = db.update_feature(created.id, UpdateFeatureInput {
                    parent_id: None,
                    title: Some("Updated Title".to_string()),
                    details: None,
                    desired_details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Query failed").expect("Feature not found");

                assert_eq!(updated.title, "Updated Title");
                assert_eq!(updated.state, FeatureState::Proposed);
            }

            it "transitions feature state" {
                let project = create_test_project(&db);
                let created = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Feature".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: Some(FeatureState::Proposed),
                }).expect("Failed to create");

                let updated = db.update_feature(created.id, UpdateFeatureInput {
                    parent_id: None,
                    title: None,
                    details: None,
                    desired_details: None,
                    priority: None,
                    target_version_id: None,
                    state: Some(FeatureState::Implemented),
                }).expect("Query failed").expect("Feature not found");

                assert_eq!(updated.state, FeatureState::Implemented);
            }
        }

        describe "delete_feature" {
            it "returns false for non-existent feature" {
                let result = db.delete_feature(Uuid::new_v4()).expect("Query failed");
                assert!(!result);
            }

            it "deletes the feature and returns true" {
                let project = create_test_project(&db);
                let created = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "To Delete".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let deleted = db.delete_feature(created.id).expect("Query failed");
                assert!(deleted);

                let found = db.get_feature(created.id).expect("Query failed");
                assert!(found.is_none());
            }
        }

        describe "get_feature_diff" {
            it "returns has_changes false when no desired_details" {
                let project = create_test_project(&db);
                let feature = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Feature".to_string(),
                    details: Some("Current details".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let diff = db.get_feature_diff(feature.id).expect("Query failed").unwrap();
                assert!(!diff.has_changes);
                assert_eq!(diff.current, Some("Current details".to_string()));
                assert!(diff.desired.is_none());
            }

            it "returns has_changes true when desired_details differs" {
                let project = create_test_project(&db);
                let feature = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Feature".to_string(),
                    details: Some("Current".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                db.update_feature(feature.id, UpdateFeatureInput {
                    parent_id: None,
                    title: None,
                    details: None,
                    desired_details: Some("Desired".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to update");

                let diff = db.get_feature_diff(feature.id).expect("Query failed").unwrap();
                assert!(diff.has_changes);
                assert_eq!(diff.current, Some("Current".to_string()));
                assert_eq!(diff.desired, Some("Desired".to_string()));
            }

            it "returns None for non-existent feature" {
                let result = db.get_feature_diff(Uuid::new_v4()).expect("Query failed");
                assert!(result.is_none());
            }
        }

        describe "desired_details" {
            it "stores and retrieves desired_details" {
                let project = create_test_project(&db);
                let feature = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Feature".to_string(),
                    details: Some("Current".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let updated = db.update_feature(feature.id, UpdateFeatureInput {
                    parent_id: None,
                    title: None,
                    details: None,
                    desired_details: Some("Desired".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to update").unwrap();

                assert_eq!(updated.details, Some("Current".to_string()));
                assert_eq!(updated.desired_details, Some("Desired".to_string()));
            }

        }

        describe "search_features" {
            it "returns empty list when no matches" {
                let project = create_test_project(&db);
                db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "User Login".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let results = db.search_features("nonexistent", None, None).expect("Query failed");
                assert!(results.is_empty());
            }

            it "matches title case-insensitively" {
                let project = create_test_project(&db);
                db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "User Authentication".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let results = db.search_features("user", None, None).expect("Query failed");
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].title, "User Authentication");

                let results = db.search_features("USER", None, None).expect("Query failed");
                assert_eq!(results.len(), 1);
            }

            it "matches details content" {
                let project = create_test_project(&db);
                db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "OAuth Integration".to_string(),
                    details: Some("Implement Google OAuth using PKCE flow".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let results = db.search_features("PKCE", None, None).expect("Query failed");
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].title, "OAuth Integration");
            }

            it "ranks title matches before details matches" {
                let project = create_test_project(&db);
                db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "User Login".to_string(),
                    details: Some("Some login details".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "OAuth Flow".to_string(),
                    details: Some("User must click login button".to_string()),
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let results = db.search_features("login", None, None).expect("Query failed");
                assert_eq!(results.len(), 2);
                // "User Login" should be first (title match)
                assert_eq!(results[0].title, "User Login");
                // "OAuth Flow" should be second (details match)
                assert_eq!(results[1].title, "OAuth Flow");
            }

            it "filters by project_id" {
                let project1 = db.create_project(CreateProjectInput {
                    name: "Project 1".to_string(),
                    description: None,
                    instructions: None,
                }).expect("Failed to create project");

                let project2 = db.create_project(CreateProjectInput {
                    name: "Project 2".to_string(),
                    description: None,
                    instructions: None,
                }).expect("Failed to create project");

                db.create_feature(project1.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Auth Feature".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                db.create_feature(project2.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Auth in Project 2".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let results = db.search_features("Auth", Some(project1.id), None).expect("Query failed");
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].title, "Auth Feature");
            }

            it "respects limit parameter" {
                let project = create_test_project(&db);
                for i in 1..=5 {
                    db.create_feature(project.id, CreateFeatureInput { id: None,
                        parent_id: None,
                        title: format!("Feature {}", i),
                        details: None,
                        priority: None,
                        state: None,
                        target_version_id: None,
                    }).expect("Failed to create");
                }

                let results = db.search_features("Feature", None, Some(2)).expect("Query failed");
                assert_eq!(results.len(), 2);
            }

            it "defaults limit to 10" {
                let project = create_test_project(&db);
                for i in 1..=15 {
                    db.create_feature(project.id, CreateFeatureInput { id: None,
                        parent_id: None,
                        title: format!("Feature {}", i),
                        details: None,
                        priority: None,
                        state: None,
                        target_version_id: None,
                    }).expect("Failed to create");
                }

                let results = db.search_features("Feature", None, None).expect("Query failed");
                assert_eq!(results.len(), 10);
            }

            it "returns FeatureSummary not full Feature" {
                let project = create_test_project(&db);
                let feature = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Unique Feature Title".to_string(),
                    details: Some("Detailed description".to_string()),
                    priority: Some(5),
                    target_version_id: None,
                    state: Some(FeatureState::Implemented),
                }).expect("Failed to create");

                // Search for something specific to avoid matching root feature ("Test Project")
                let results = db.search_features("Unique Feature", None, None).expect("Query failed");
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

    describe "feature_hierarchy" {
        describe "nested features" {
            it "creates a child feature under a parent" {
                let project = create_test_project(&db);
                let parent = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Authentication".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create parent");

                let child = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: Some(parent.id),
                    title: "Login".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create child");

                assert_eq!(child.parent_id, Some(parent.id));
            }

            it "creates deeply nested features" {
                let project = create_test_project(&db);
                let root = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Authentication".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let level1 = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: Some(root.id),
                    title: "OAuth".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let level2 = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: Some(level1.id),
                    title: "Google".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                assert_eq!(level2.parent_id, Some(level1.id));

                let found = db.get_feature(level2.id).expect("Query failed").unwrap();
                assert_eq!(found.parent_id, Some(level1.id));
            }
        }

        describe "get_root_features" {
            it "returns only features without parents" {
                let project = create_test_project(&db);
                let root1 = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Root 1".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let _root2 = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Root 2".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: Some(root1.id),
                    title: "Child".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let roots = db.get_root_features(project.id).expect("Query failed");

                assert_eq!(roots.len(), 2);
                // With the root feature model, "root" features are children of the project's
                // root_feature, so they have parent_id = root_feature_id (not None)
                assert!(roots.iter().all(|f| f.parent_id == project.root_feature_id));
            }
        }

        describe "get_children" {
            it "returns empty list when feature has no children" {
                let project = create_test_project(&db);
                let leaf = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Leaf".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let children = db.get_children(leaf.id).expect("Query failed");
                assert!(children.is_empty());
            }

            it "returns direct children ordered by title" {
                let project = create_test_project(&db);
                let parent = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Parent".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: Some(parent.id),
                    title: "Zebra Child".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: Some(parent.id),
                    title: "Alpha Child".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let children = db.get_children(parent.id).expect("Query failed");

                assert_eq!(children.len(), 2);
                assert_eq!(children[0].title, "Alpha Child");
                assert_eq!(children[1].title, "Zebra Child");
            }

            it "does not return grandchildren" {
                let project = create_test_project(&db);
                let parent = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Parent".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let child = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: Some(parent.id),
                    title: "Child".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: Some(child.id),
                    title: "Grandchild".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let children = db.get_children(parent.id).expect("Query failed");
                assert_eq!(children.len(), 1);
                assert_eq!(children[0].title, "Child");
            }
        }

        describe "is_leaf" {
            it "returns true for feature with no children" {
                let project = create_test_project(&db);
                let leaf = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Leaf".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                assert!(db.is_leaf(leaf.id).expect("Query failed"));
            }

            it "returns false for feature with children" {
                let project = create_test_project(&db);
                let parent = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Parent".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: Some(parent.id),
                    title: "Child".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                assert!(!db.is_leaf(parent.id).expect("Query failed"));
            }
        }

        describe "cascade delete" {
            it "deletes children when parent is deleted" {
                let project = create_test_project(&db);
                let parent = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Parent".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let child = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: Some(parent.id),
                    title: "Child".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                db.delete_feature(parent.id).expect("Failed to delete");

                let found = db.get_feature(child.id).expect("Query failed");
                assert!(found.is_none());
            }
        }
    }

    describe "feature_history" {
        describe "create_history_entry" {
            it "creates a history entry with all fields" {
                let project = create_test_project(&db);
                let feature = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Test Feature".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create feature");

                // Create a version to link the history entry to
                let version = db.create_version(project.id, CreateVersionInput {
                    name: "v1.0.0".to_string(),
                    description: None,
                }).expect("Failed to create version");

                let entry = db.create_history_entry(CreateHistoryInput {
                    feature_id: feature.id,
                    version_id: Some(version.id),
                    details: HistoryDetails {
                        summary: "Implemented login flow".to_string(),
                        commits: vec![],
                    },
                }).expect("Failed to create history entry");

                assert_eq!(entry.feature_id, feature.id);
                assert_eq!(entry.version_id, Some(version.id));
                assert_eq!(entry.details.summary, "Implemented login flow");
            }

            it "creates entry without version_id" {
                let project = create_test_project(&db);
                let feature = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Manual Feature".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create feature");

                let entry = db.create_history_entry(CreateHistoryInput {
                    feature_id: feature.id,
                    version_id: None,
                    details: HistoryDetails {
                        summary: "Manual update".to_string(),
                        commits: vec![],
                    },
                }).expect("Failed to create history entry");

                assert!(entry.version_id.is_none());
            }
        }

        describe "get_feature_history" {
            it "returns empty list when no history exists" {
                let project = create_test_project(&db);
                let feature = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "New Feature".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create feature");

                let history = db.get_feature_history(feature.id).expect("Query failed");
                assert!(history.is_empty());
            }

            it "returns history entries in reverse chronological order" {
                let project = create_test_project(&db);
                let feature = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Feature".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create feature");

                db.create_history_entry(CreateHistoryInput {
                    feature_id: feature.id,
                    version_id: None,
                    details: HistoryDetails {
                        summary: "First change".to_string(),
                        commits: vec![],
                    },
                }).expect("Failed to create");

                db.create_history_entry(CreateHistoryInput {
                    feature_id: feature.id,
                    version_id: None,
                    details: HistoryDetails {
                        summary: "Second change".to_string(),
                        commits: vec![],
                    },
                }).expect("Failed to create");

                let history = db.get_feature_history(feature.id).expect("Query failed");

                assert_eq!(history.len(), 2);
                assert_eq!(history[0].details.summary, "Second change");
                assert_eq!(history[1].details.summary, "First change");
            }

            it "only returns history for specified feature" {
                let project = create_test_project(&db);
                let feature1 = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Feature 1".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let feature2 = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Feature 2".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                db.create_history_entry(CreateHistoryInput {
                    feature_id: feature1.id,
                    version_id: None,
                    details: HistoryDetails {
                        summary: "Change to feature 1".to_string(),
                        commits: vec![],
                    },
                }).expect("Failed to create");

                db.create_history_entry(CreateHistoryInput {
                    feature_id: feature2.id,
                    version_id: None,
                    details: HistoryDetails {
                        summary: "Change to feature 2".to_string(),
                        commits: vec![],
                    },
                }).expect("Failed to create");

                let history = db.get_feature_history(feature1.id).expect("Query failed");

                assert_eq!(history.len(), 1);
                assert_eq!(history[0].details.summary, "Change to feature 1");
            }
        }

        describe "cascade delete" {
            it "deletes history when feature is deleted" {
                let project = create_test_project(&db);
                let feature = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Feature".to_string(),

                    details: None,
                priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                db.create_history_entry(CreateHistoryInput {
                    feature_id: feature.id,
                    version_id: None,
                    details: HistoryDetails {
                        summary: "Some work".to_string(),
                        commits: vec![],
                    },
                }).expect("Failed to create");

                db.delete_feature(feature.id).expect("Failed to delete");

                // History should be gone (cascade delete)
                let history = db.get_feature_history(feature.id).expect("Query failed");
                assert!(history.is_empty());
            }
        }

        describe "get_project_history" {
            it "returns empty list when no history exists" {
                let project = create_test_project(&db);
                let history = db.get_project_history(project.id, None, None, None, None).expect("Query failed");
                assert!(history.is_empty());
            }

            it "returns history entries across all project features" {
                let project = create_test_project(&db);
                let feature1 = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Feature 1".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let feature2 = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Feature 2".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                db.create_history_entry(CreateHistoryInput {
                    feature_id: feature1.id,
                    version_id: None,
                    details: HistoryDetails {
                        summary: "Work on feature 1".to_string(),
                        commits: vec![],
                    },
                }).expect("Failed to create");

                db.create_history_entry(CreateHistoryInput {
                    feature_id: feature2.id,
                    version_id: None,
                    details: HistoryDetails {
                        summary: "Work on feature 2".to_string(),
                        commits: vec![],
                    },
                }).expect("Failed to create");

                let history = db.get_project_history(project.id, None, None, None, None).expect("Query failed");
                assert_eq!(history.len(), 2);
            }

            it "includes feature title and state in entries" {
                let project = create_test_project(&db);
                let feature = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "User Authentication".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: Some(FeatureState::Implemented),
                }).expect("Failed to create");

                db.create_history_entry(CreateHistoryInput {
                    feature_id: feature.id,
                    version_id: None,
                    details: HistoryDetails {
                        summary: "Implemented OAuth flow".to_string(),
                        commits: vec![],
                    },
                }).expect("Failed to create");

                let history = db.get_project_history(project.id, None, None, None, None).expect("Query failed");
                assert_eq!(history.len(), 1);
                assert_eq!(history[0].feature_title, "User Authentication");
                assert_eq!(history[0].feature_state, FeatureState::Implemented);
                assert_eq!(history[0].summary, "Implemented OAuth flow");
            }

            it "returns entries in reverse chronological order" {
                let project = create_test_project(&db);
                let feature = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Feature".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                db.create_history_entry(CreateHistoryInput {
                    feature_id: feature.id,
                    version_id: None,
                    details: HistoryDetails {
                        summary: "First change".to_string(),
                        commits: vec![],
                    },
                }).expect("Failed to create");

                db.create_history_entry(CreateHistoryInput {
                    feature_id: feature.id,
                    version_id: None,
                    details: HistoryDetails {
                        summary: "Second change".to_string(),
                        commits: vec![],
                    },
                }).expect("Failed to create");

                let history = db.get_project_history(project.id, None, None, None, None).expect("Query failed");
                assert_eq!(history.len(), 2);
                assert_eq!(history[0].summary, "Second change");
                assert_eq!(history[1].summary, "First change");
            }

            it "respects limit parameter" {
                let project = create_test_project(&db);
                let feature = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Feature".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                for i in 1..=5 {
                    db.create_history_entry(CreateHistoryInput {
                        feature_id: feature.id,
                        version_id: None,
                        details: HistoryDetails {
                            summary: format!("Change {}", i),
                            commits: vec![],
                        },
                    }).expect("Failed to create");
                }

                let history = db.get_project_history(project.id, None, Some(2), None, None).expect("Query failed");
                assert_eq!(history.len(), 2);
            }

            it "respects offset parameter" {
                let project = create_test_project(&db);
                let feature = db.create_feature(project.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Feature".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                for i in 1..=5 {
                    db.create_history_entry(CreateHistoryInput {
                        feature_id: feature.id,
                        version_id: None,
                        details: HistoryDetails {
                            summary: format!("Change {}", i),
                            commits: vec![],
                        },
                    }).expect("Failed to create");
                }

                let history = db.get_project_history(project.id, None, Some(2), Some(2), None).expect("Query failed");
                assert_eq!(history.len(), 2);
                // Should skip the 2 most recent entries (5 and 4) and return 3 and 2
                assert_eq!(history[0].summary, "Change 3");
                assert_eq!(history[1].summary, "Change 2");
            }

            it "excludes history from other projects" {
                let project1 = db.create_project(CreateProjectInput {
                    name: "Project 1".to_string(),
                    description: None,
                    instructions: None,
                }).expect("Failed to create");

                let project2 = db.create_project(CreateProjectInput {
                    name: "Project 2".to_string(),
                    description: None,
                    instructions: None,
                }).expect("Failed to create");

                let feature1 = db.create_feature(project1.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Feature in P1".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                let feature2 = db.create_feature(project2.id, CreateFeatureInput { id: None,
                    parent_id: None,
                    title: "Feature in P2".to_string(),
                    details: None,
                    priority: None,
                    target_version_id: None,
                    state: None,
                }).expect("Failed to create");

                db.create_history_entry(CreateHistoryInput {
                    feature_id: feature1.id,
                    version_id: None,
                    details: HistoryDetails {
                        summary: "Work in project 1".to_string(),
                        commits: vec![],
                    },
                }).expect("Failed to create");

                db.create_history_entry(CreateHistoryInput {
                    feature_id: feature2.id,
                    version_id: None,
                    details: HistoryDetails {
                        summary: "Work in project 2".to_string(),
                        commits: vec![],
                    },
                }).expect("Failed to create");

                let history1 = db.get_project_history(project1.id, None, None, None, None).expect("Query failed");
                assert_eq!(history1.len(), 1);
                assert_eq!(history1[0].summary, "Work in project 1");

                let history2 = db.get_project_history(project2.id, None, None, None, None).expect("Query failed");
                assert_eq!(history2.len(), 1);
                assert_eq!(history2[0].summary, "Work in project 2");
            }
        }
    }

}
