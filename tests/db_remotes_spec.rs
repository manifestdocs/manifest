mod common;

use common::*;
use manifest::models::*;

// ── Remote CRUD ────────────────────────────────────────────────────

mod remote_crud {
    use super::*;

    #[tokio::test]
    async fn creates_a_remote() {
        let db = setup().await;
        let remote = db
            .create_remote(&CreateRemoteInput {
                name: "work".to_string(),
                provider: None,
                url: "libsql://mydb.turso.io".to_string(),
                token: "secret-token".to_string(),
            })
            .await
            .expect("create remote");

        assert_eq!(remote.name, "work");
        assert_eq!(remote.provider, "turso");
        assert_eq!(remote.url, "libsql://mydb.turso.io");
        assert!(remote.sync_enabled);
    }

    #[tokio::test]
    async fn rejects_duplicate_name() {
        let db = setup().await;
        db.create_remote(&CreateRemoteInput {
            name: "work".to_string(),
            provider: None,
            url: "libsql://one.turso.io".to_string(),
            token: "token1".to_string(),
        })
        .await
        .expect("create first");

        let result = db
            .create_remote(&CreateRemoteInput {
                name: "work".to_string(),
                provider: None,
                url: "libsql://two.turso.io".to_string(),
                token: "token2".to_string(),
            })
            .await;

        assert!(result.is_err(), "duplicate name should fail");
    }

    #[tokio::test]
    async fn lists_remotes_alphabetically() {
        let db = setup().await;
        db.create_remote(&CreateRemoteInput {
            name: "zebra".to_string(),
            provider: None,
            url: "libsql://z.turso.io".to_string(),
            token: "t1".to_string(),
        })
        .await
        .unwrap();
        db.create_remote(&CreateRemoteInput {
            name: "alpha".to_string(),
            provider: None,
            url: "libsql://a.turso.io".to_string(),
            token: "t2".to_string(),
        })
        .await
        .unwrap();

        let remotes = db.list_remotes().await.unwrap();
        assert_eq!(remotes.len(), 2);
        assert_eq!(remotes[0].name, "alpha");
        assert_eq!(remotes[1].name, "zebra");
    }

    #[tokio::test]
    async fn gets_remote_by_name() {
        let db = setup().await;
        let created = db
            .create_remote(&CreateRemoteInput {
                name: "personal".to_string(),
                provider: Some("turso".to_string()),
                url: "libsql://personal.turso.io".to_string(),
                token: "tok".to_string(),
            })
            .await
            .unwrap();

        let found = db
            .get_remote_by_name("personal")
            .await
            .unwrap()
            .expect("should find by name");
        assert_eq!(found.id, created.id);
    }

    #[tokio::test]
    async fn updates_remote_url() {
        let db = setup().await;
        let remote = db
            .create_remote(&CreateRemoteInput {
                name: "work".to_string(),
                provider: None,
                url: "libsql://old.turso.io".to_string(),
                token: "tok".to_string(),
            })
            .await
            .unwrap();

        let updated = db
            .update_remote(
                remote.id,
                &UpdateRemoteInput {
                    url: Some("libsql://new.turso.io".to_string()),
                    token: None,
                    sync_enabled: None,
                },
            )
            .await
            .unwrap()
            .expect("update should return Some");

        assert_eq!(updated.url, "libsql://new.turso.io");
        assert_eq!(updated.name, "work"); // unchanged
    }

    #[tokio::test]
    async fn updates_remote_token() {
        let db = setup().await;
        let remote = db
            .create_remote(&CreateRemoteInput {
                name: "work".to_string(),
                provider: None,
                url: "libsql://db.turso.io".to_string(),
                token: "old-token".to_string(),
            })
            .await
            .unwrap();

        db.update_remote(
            remote.id,
            &UpdateRemoteInput {
                url: None,
                token: Some("new-token".to_string()),
                sync_enabled: None,
            },
        )
        .await
        .unwrap()
        .expect("update should return Some");

        // Verify token was updated by checking DB directly
        let mut rows = db
            .conn()
            .query(
                "SELECT auth_token FROM remotes WHERE id = ?1",
                libsql::params![remote.id.to_string()],
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let token: String = row.get(0).unwrap();
        assert_eq!(token, "new-token");
    }

    #[tokio::test]
    async fn deletes_remote() {
        let db = setup().await;
        let remote = db
            .create_remote(&CreateRemoteInput {
                name: "temp".to_string(),
                provider: None,
                url: "libsql://temp.turso.io".to_string(),
                token: "tok".to_string(),
            })
            .await
            .unwrap();

        let deleted = db.delete_remote(remote.id).await.unwrap();
        assert!(deleted);

        let found = db.get_remote(remote.id).await.unwrap();
        assert!(found.is_none());
    }
}

// ── Project-Remote Linking ────────────────────────────────────────

mod project_remote_linking {
    use super::*;

    async fn setup_with_remote(db: &manifest::db::Database) -> (Project, Remote) {
        let project = create_test_project(db).await;
        let remote = db
            .create_remote(&CreateRemoteInput {
                name: "test-remote".to_string(),
                provider: None,
                url: "libsql://test.turso.io".to_string(),
                token: "tok".to_string(),
            })
            .await
            .unwrap();
        (project, remote)
    }

    #[tokio::test]
    async fn links_project_to_remote() {
        let db = setup().await;
        let (project, remote) = setup_with_remote(&db).await;

        let link = db
            .link_project_remote(project.id, remote.id)
            .await
            .expect("link project");

        assert_eq!(link.project_id, project.id);
        assert_eq!(link.remote_id, remote.id);
        assert_eq!(link.sync_state, SyncState::Active);
        assert!(link.last_synced_at.is_none());
    }

    #[tokio::test]
    async fn rejects_duplicate_link() {
        let db = setup().await;
        let (project, remote) = setup_with_remote(&db).await;

        db.link_project_remote(project.id, remote.id).await.unwrap();
        let result = db.link_project_remote(project.id, remote.id).await;
        assert!(result.is_err(), "duplicate link should fail");
    }

    #[tokio::test]
    async fn unlinks_project_from_remote() {
        let db = setup().await;
        let (project, remote) = setup_with_remote(&db).await;

        db.link_project_remote(project.id, remote.id).await.unwrap();
        let removed = db
            .unlink_project_remote(project.id, remote.id)
            .await
            .unwrap();
        assert!(removed);

        let link = db.get_project_remote(project.id, remote.id).await.unwrap();
        assert!(link.is_none());
    }

    #[tokio::test]
    async fn delete_remote_orphans_linked_projects() {
        let db = setup().await;
        let (project, remote) = setup_with_remote(&db).await;

        db.link_project_remote(project.id, remote.id).await.unwrap();

        // Delete the remote — should orphan the link first, then cascade delete
        db.delete_remote(remote.id).await.unwrap();

        // The project_remote should be deleted (CASCADE)
        let link = db.get_project_remote(project.id, remote.id).await.unwrap();
        assert!(link.is_none(), "link should be deleted by cascade");
    }

    #[tokio::test]
    async fn re_links_orphaned_project() {
        let db = setup().await;
        let project = create_test_project(&db).await;

        let remote1 = db
            .create_remote(&CreateRemoteInput {
                name: "r1".to_string(),
                provider: None,
                url: "libsql://r1.turso.io".to_string(),
                token: "t1".to_string(),
            })
            .await
            .unwrap();

        // Link then orphan manually
        db.link_project_remote(project.id, remote1.id)
            .await
            .unwrap();

        db.conn()
            .execute(
                "UPDATE project_remotes SET sync_state = 'orphaned' WHERE project_id = ?1 AND remote_id = ?2",
                libsql::params![project.id.to_string(), remote1.id.to_string()],
            )
            .await
            .unwrap();

        // Re-link should re-activate
        let link = db
            .link_project_remote(project.id, remote1.id)
            .await
            .unwrap();
        assert_eq!(link.sync_state, SyncState::Active);
    }

    #[tokio::test]
    async fn lists_remotes_for_project() {
        let db = setup().await;
        let project = create_test_project(&db).await;

        let r1 = db
            .create_remote(&CreateRemoteInput {
                name: "r1".to_string(),
                provider: None,
                url: "libsql://r1.turso.io".to_string(),
                token: "t1".to_string(),
            })
            .await
            .unwrap();
        let r2 = db
            .create_remote(&CreateRemoteInput {
                name: "r2".to_string(),
                provider: None,
                url: "libsql://r2.turso.io".to_string(),
                token: "t2".to_string(),
            })
            .await
            .unwrap();

        db.link_project_remote(project.id, r1.id).await.unwrap();
        db.link_project_remote(project.id, r2.id).await.unwrap();

        let links = db.get_project_remotes(project.id).await.unwrap();
        assert_eq!(links.len(), 2);
    }

    #[tokio::test]
    async fn lists_projects_for_remote() {
        let db = setup().await;
        let remote = db
            .create_remote(&CreateRemoteInput {
                name: "shared".to_string(),
                provider: None,
                url: "libsql://shared.turso.io".to_string(),
                token: "tok".to_string(),
            })
            .await
            .unwrap();

        let p1 = create_test_project(&db).await;
        let p2 = db
            .create_project(CreateProjectInput {
                id: None,
                slug: Some("project-two".to_string()),
                name: "Project Two".to_string(),
                description: None,
                instructions: None,
                key_prefix: None,
                skip_default_versions: true,
            })
            .await
            .unwrap();

        db.link_project_remote(p1.id, remote.id).await.unwrap();
        db.link_project_remote(p2.id, remote.id).await.unwrap();

        let links = db.get_remote_projects(remote.id).await.unwrap();
        assert_eq!(links.len(), 2);
    }
}
