//! MCP server for AI-assisted feature development.
//!
//! Exposes tools optimized for CLI agents like Claude Code:
//! - Discovery: list_projects, find_features, get_feature, render_feature_tree, sync
//! - Setup: init_project, add_project_directory, plan, create_feature
//! - Work: start_feature, complete_feature, get_next_feature
//! - Versions: list_versions, create_version, set_feature_version, release_version

use super::tools;
use super::types::{
    AddProjectDirectoryRequest, CompleteFeatureRequest, CreateFeatureRequest, CreateVersionRequest,
    DeleteFeatureRequest, FindFeaturesRequest, GenerateFeatureTreeRequest, GetActiveFeatureRequest,
    GetFeatureRequest, GetNextFeatureRequest, GetProjectHistoryRequest,
    GetProjectInstructionsRequest, InitProjectRequest, ListProjectsRequest, ListVersionsRequest,
    PlanFeaturesRequest, ProveFeatureRequest, RecordVerificationRequest, ReleaseVersionRequest,
    RenderFeatureTreeRequest, SetFeatureVersionRequest, StartFeatureRequest, SyncRequest,
    UpdateFeatureRequest, VerifyFeatureRequest,
};
use super::ManifestClient;
use rmcp::{
    handler::server::{
        tool::{ToolCallContext, ToolRouter},
        wrapper::Parameters,
    },
    model::{CallToolResult, Content, ListToolsResult, ServerInfo},
    service::RequestContext,
    tool, tool_router, ErrorData as McpError, RoleServer, ServerHandler,
};
use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

// ============================================================
// Server Implementation
// ============================================================

pub struct McpServer {
    client: ManifestClient,
    tool_router: ToolRouter<Self>,
    /// Populated by a background task with an update notice if a newer
    /// version is available, or `None` if the server is up-to-date.
    update_notice: Arc<OnceLock<Option<String>>>,
}

impl McpServer {
    /// Create a new MCP server backed by the given [`ManifestClient`].
    ///
    /// Spawns a background task to check for newer Manifest releases;
    /// the result is appended to tool responses once available.
    pub fn new(client: ManifestClient) -> Self {
        let update_notice = Arc::new(OnceLock::new());

        // Spawn a background task to check for updates. The notice is
        // appended to tool responses once the check completes.
        let cell = Arc::clone(&update_notice);
        tokio::spawn(async move {
            let notice = super::version_check::check_for_update().await;
            let _ = cell.set(notice);
        });

        Self {
            client,
            tool_router: Self::tool_router(),
            update_notice,
        }
    }

    /// Create an MCP server using a [`ManifestClient`] configured from environment variables.
    pub fn from_env() -> Self {
        Self::new(ManifestClient::from_env())
    }
}

#[tool_router]
impl McpServer {
    // ============================================================
    // Discovery Tools
    // ============================================================

    #[tool(description = "List projects")]
    async fn list_projects(
        &self,
        params: Parameters<ListProjectsRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::projects::list_projects(&self.client, params.0).await
    }

    #[tool(description = "Get project instructions")]
    async fn get_project_instructions(
        &self,
        params: Parameters<GetProjectInstructionsRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::projects::get_project_instructions(&self.client, params.0).await
    }

    #[tool(description = "Get active feature")]
    async fn get_active_feature(
        &self,
        params: Parameters<GetActiveFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::context::get_active_feature(&self.client, params.0).await
    }

    #[tool(description = "Find features")]
    async fn find_features(
        &self,
        params: Parameters<FindFeaturesRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::find_features(&self.client, params.0).await
    }

    #[tool(description = "Get feature details")]
    async fn get_feature(
        &self,
        params: Parameters<GetFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::get_feature(&self.client, params.0).await
    }

    #[tool(description = "Render feature tree")]
    async fn render_feature_tree(
        &self,
        params: Parameters<RenderFeatureTreeRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::render_feature_tree(&self.client, params.0).await
    }

    #[tool(description = "Get project history")]
    async fn get_project_history(
        &self,
        params: Parameters<GetProjectHistoryRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::projects::get_project_history(&self.client, params.0).await
    }

    // ============================================================
    // Setup Tools
    // ============================================================

    #[tool(description = "Initialize project")]
    async fn init_project(
        &self,
        params: Parameters<InitProjectRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::projects::init_project(&self.client, params.0).await
    }

    #[tool(description = "Add project directory")]
    async fn add_project_directory(
        &self,
        params: Parameters<AddProjectDirectoryRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::projects::add_project_directory(&self.client, params.0).await
    }

    #[tool(description = "Generate feature tree")]
    async fn generate_feature_tree(
        &self,
        params: Parameters<GenerateFeatureTreeRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::generate::generate_feature_tree(params.0).await
    }

    #[tool(description = "Plan features")]
    async fn plan(
        &self,
        params: Parameters<PlanFeaturesRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::plan(&self.client, params.0).await
    }

    #[tool(description = "Sync features")]
    async fn sync(&self, params: Parameters<SyncRequest>) -> Result<CallToolResult, McpError> {
        tools::sync::sync(&self.client, params.0).await
    }

    #[tool(description = "Create feature")]
    async fn create_feature(
        &self,
        params: Parameters<CreateFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::create_feature(&self.client, params.0).await
    }

    // ============================================================
    // Work Tools
    // ============================================================

    #[tool(description = "Start feature")]
    async fn start_feature(
        &self,
        params: Parameters<StartFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::start_feature(&self.client, params.0).await
    }

    #[tool(description = "Complete feature")]
    async fn complete_feature(
        &self,
        params: Parameters<CompleteFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::complete_feature(&self.client, params.0).await
    }

    #[tool(description = "Prove feature")]
    async fn prove_feature(
        &self,
        params: Parameters<ProveFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::prove_feature(&self.client, params.0).await
    }

    #[tool(description = "Update feature")]
    async fn update_feature(
        &self,
        params: Parameters<UpdateFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::update_feature(&self.client, params.0).await
    }

    #[tool(description = "Delete feature")]
    async fn delete_feature(
        &self,
        params: Parameters<DeleteFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::delete_feature(&self.client, params.0).await
    }

    #[tool(description = "Get next feature")]
    async fn get_next_feature(
        &self,
        params: Parameters<GetNextFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::get_next_feature(&self.client, params.0).await
    }

    // ============================================================
    // Verification Tools
    // ============================================================

    #[tool(description = "Verify feature")]
    async fn verify_feature(
        &self,
        params: Parameters<VerifyFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::verify_feature(&self.client, params.0).await
    }

    #[tool(description = "Record verification")]
    async fn record_verification(
        &self,
        params: Parameters<RecordVerificationRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::record_verification(&self.client, params.0).await
    }

    // ============================================================
    // Version Tools
    // ============================================================

    #[tool(description = "List versions")]
    async fn list_versions(
        &self,
        params: Parameters<ListVersionsRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::versions::list_versions(&self.client, params.0).await
    }

    #[tool(description = "Create version")]
    async fn create_version(
        &self,
        params: Parameters<CreateVersionRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::versions::create_version(&self.client, params.0).await
    }

    #[tool(description = "Set feature version")]
    async fn set_feature_version(
        &self,
        params: Parameters<SetFeatureVersionRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::versions::set_feature_version(&self.client, params.0).await
    }

    #[tool(description = "Release version")]
    async fn release_version(
        &self,
        params: Parameters<ReleaseVersionRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::versions::release_version(&self.client, params.0).await
    }
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            server_info: rmcp::model::Implementation {
                name: "mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                title: None,
                icons: None,
                website_url: None,
            },
            capabilities: rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .build(),
            instructions: Some(INSTRUCTIONS.into()),
            ..Default::default()
        }
    }

    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let tcc = ToolCallContext::new(self, request, context);
        let mut result = self.tool_router.call(tcc).await?;

        // Append update notice once the background check has completed
        if let Some(Some(notice)) = self.update_notice.get() {
            result.content.push(Content::text(notice));
        }

        Ok(result)
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let mut tools = self.tool_router.list_all();
        for tool in &mut tools {
            if let Some(desc) = tool_description(&tool.name) {
                tool.description = Some(Cow::Borrowed(desc));
            }
        }
        Ok(ListToolsResult {
            tools,
            meta: None,
            next_cursor: None,
        })
    }
}

const INSTRUCTIONS: &str = include_str!("instructions/server.xml");

fn tool_description(name: &str) -> Option<&'static str> {
    Some(match name {
        "list_projects" => include_str!("instructions/tools/list_projects.txt"),
        "get_project_instructions" => {
            include_str!("instructions/tools/get_project_instructions.txt")
        }
        "get_active_feature" => include_str!("instructions/tools/get_active_feature.txt"),
        "find_features" => include_str!("instructions/tools/find_features.txt"),
        "get_feature" => include_str!("instructions/tools/get_feature.txt"),
        "render_feature_tree" => include_str!("instructions/tools/render_feature_tree.txt"),
        "get_project_history" => include_str!("instructions/tools/get_project_history.txt"),
        "init_project" => include_str!("instructions/tools/init_project.txt"),
        "add_project_directory" => include_str!("instructions/tools/add_project_directory.txt"),
        "generate_feature_tree" => include_str!("instructions/tools/generate_feature_tree.txt"),
        "plan" => include_str!("instructions/tools/plan.txt"),
        "sync" => include_str!("instructions/tools/sync.txt"),
        "create_feature" => include_str!("instructions/tools/create_feature.txt"),
        "start_feature" => include_str!("instructions/tools/start_feature.txt"),
        "complete_feature" => include_str!("instructions/tools/complete_feature.txt"),
        "prove_feature" => include_str!("instructions/tools/prove_feature.txt"),
        "update_feature" => include_str!("instructions/tools/update_feature.txt"),
        "delete_feature" => include_str!("instructions/tools/delete_feature.txt"),
        "get_next_feature" => include_str!("instructions/tools/get_next_feature.txt"),
        "verify_feature" => include_str!("instructions/tools/verify_feature.txt"),
        "record_verification" => include_str!("instructions/tools/record_verification.txt"),
        "list_versions" => include_str!("instructions/tools/list_versions.txt"),
        "create_version" => include_str!("instructions/tools/create_version.txt"),
        "set_feature_version" => include_str!("instructions/tools/set_feature_version.txt"),
        "release_version" => include_str!("instructions/tools/release_version.txt"),
        _ => return None,
    })
}
