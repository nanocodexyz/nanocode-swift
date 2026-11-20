use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use nanocode::{
    ClaudeClient, ClaudeModel, HarnessFamily, Model, ModelSelection, OpenAi, ServiceTier, Thinking,
    Tools,
    agent::ExecutionEnvironment,
    antigravity::{AntigravityHarness, AuthMethod, Client, DEFAULT_MODEL, Endpoint, GoogleConfig},
    claude::ClaudeEnvironment,
    codex::{OpenAiAuth, transport::ResponsesTransport},
};
use serde::Deserialize;

use crate::{
    files::NativeFiles,
    payments::Service,
    shell::{Shell, Simulated},
};

pub const ANTHROPIC_API: &str = "https://api.anthropic.com/v1";
pub const OPENAI_API: &str = "https://api.openai.com/v1";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    #[default]
    Claude,
    Codex,
    Antigravity,
}

impl Provider {
    fn default_model(self) -> ModelSelection {
        match self {
            Self::Claude => ModelSelection::Claude(ClaudeModel::Haiku55),
            Self::Codex => ModelSelection::Codex(Model::Luna),
            Self::Antigravity => AntigravityHarness::selection(DEFAULT_MODEL),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
        }
    }
}

pub const GOOGLE_STATE_FILE: &str = "antigravity.json";
const NO_GOOGLE_PLAN: &str = "Google offers no plan sign-in for apps built on nanocode: use a Gemini API key, Vertex AI with Application Default Credentials or an access token, a Vertex AI express mode key or an LLM gateway";

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Auth {
    ApiKey(String),
    Headers(BTreeMap<String, String>),
    Plan,
    #[default]
    Tempo,
    Google(GoogleCredentials),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum GoogleCredentials {
    Vertex {
        #[serde(default)]
        project: Option<String>,
        #[serde(default)]
        location: Option<String>,
        #[serde(default)]
        access_token: Option<String>,
        #[serde(default)]
        credentials_file: Option<PathBuf>,
    },
    VertexExpress(String),
    Gateway {
        url: String,
        #[serde(default)]
        api_key: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceTools {
    #[default]
    All,
    Simulated,
    Files,
    None,
}

impl WorkspaceTools {
    pub fn resolved(self) -> Self {
        if cfg!(target_os = "ios") && self == Self::All {
            Self::Simulated
        } else {
            self
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub project_instructions: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HandMode {
    #[default]
    Alongside,
    Replace,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandLink {
    pub url: String,
    pub token: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub mode: HandMode,
}

impl HandLink {
    pub fn runner(&self) -> nanocode::Runner {
        let runner = nanocode::Runner::connect(self.url.clone()).token(self.token.clone());
        let runner = match &self.name {
            Some(name) => runner.name(name.clone()),
            None => runner,
        };
        match self.mode {
            HandMode::Alongside => runner.alongside(),
            HandMode::Replace => runner.replace(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServerConfig {
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub bearer_token: Option<String>,
    #[serde(default)]
    pub paid: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fork {
    pub session: String,
    #[serde(default)]
    pub checkpoint: Option<nanocode::SessionCheckpoint>,
    #[serde(default)]
    pub before: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    #[serde(default)]
    pub hands: Vec<HandLink>,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
    pub session_id: String,
    #[serde(default)]
    pub provider: Provider,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub thinking: Option<String>,
    #[serde(default)]
    pub service_tier: Option<ServiceTier>,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub auth: Auth,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub subagents: Option<bool>,
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    #[serde(default)]
    pub workspace_tools: WorkspaceTools,
    #[serde(default)]
    pub tools: Vec<crate::tools::HostTool>,
    #[serde(default = "enabled")]
    pub web_search: bool,
    #[serde(default)]
    pub paid_tools: bool,
    #[serde(default)]
    pub stream_tool_input: bool,
    #[serde(default)]
    pub live_deltas: bool,
    #[serde(default)]
    pub event_coalesce_ms: u64,
    #[serde(default)]
    pub environment: Option<Environment>,
    #[serde(default)]
    pub permission_mode: Option<crate::approvals::Mode>,
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub observe_edits: bool,
    #[serde(default)]
    pub fork: Option<Fork>,
}

const fn enabled() -> bool {
    true
}

impl Configuration {
    pub fn model(&self) -> Result<ModelSelection, String> {
        let Some(model) = self
            .model
            .as_deref()
            .filter(|model| !model.trim().is_empty())
        else {
            return Ok(self.provider.default_model());
        };
        let selection = match self.provider {
            Provider::Claude => model.parse::<ClaudeModel>().map_or_else(
                |_| ModelSelection::new(HarnessFamily::Claude, model),
                ModelSelection::Claude,
            ),
            Provider::Codex => model
                .parse::<Model>()
                .map(ModelSelection::Codex)
                .map_err(|error| format!("model {model}: {error}"))?,
            Provider::Antigravity => AntigravityHarness::selection(model),
        };
        selection
            .validate()
            .map_err(|error| format!("model {model}: {error}"))?;
        Ok(selection)
    }

    pub fn thinking(&self) -> Result<Option<Thinking>, String> {
        self.thinking.as_deref().map(str::parse).transpose()
    }

    pub fn claude_environment(&self) -> Option<ClaudeEnvironment> {
        let environment = self.environment.as_ref()?;
        let mut claude = ClaudeEnvironment::new();
        if let Some(cwd) = environment
            .cwd
            .as_deref()
            .or(self.workspace_text().as_deref())
        {
            claude = claude.working_directory(cwd);
        }
        if let Some(shell) = &environment.shell {
            claude = claude.shell(shell.as_str());
        }
        if let Some(text) = &environment.project_instructions {
            claude = claude.project_instructions(text.as_str());
        }
        Some(claude)
    }

    pub fn codex_environment(&self) -> Option<ExecutionEnvironment> {
        let environment = self.environment.as_ref()?;
        let timezone: Arc<str> = environment.timezone.as_deref().unwrap_or("Etc/UTC").into();
        let mut codex = match &environment.date {
            Some(date) => ExecutionEnvironment::new(date.as_str(), timezone),
            None => ExecutionEnvironment::dated(timezone, crate::files::today),
        };
        if let Some(cwd) = environment
            .cwd
            .as_deref()
            .or(self.workspace_text().as_deref())
        {
            codex = codex.working_directory(cwd);
        }
        if let Some(shell) = &environment.shell {
            codex = codex.shell(shell.as_str());
        }
        if let Some(text) = &environment.project_instructions {
            codex = codex.project_instructions(text.as_str());
        }
        Some(codex)
    }

    fn workspace_text(&self) -> Option<String> {
        self.workspace
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
    }

    pub fn workspace_tools(&self, family: Provider, shell: &Arc<Shell>) -> Result<Tools, String> {
        let workspace = self.workspace.clone();
        let files = Arc::new(NativeFiles);
        let simulated = || Arc::new(Simulated(Arc::clone(shell)));
        let tools = match (workspace, self.workspace_tools.resolved(), family) {
            (Some(root), WorkspaceTools::All, Provider::Claude) => {
                return nanocode::claude::runner_tools(root).map_err(|error| error.to_string());
            }
            (Some(_), WorkspaceTools::All, Provider::Codex) => Tools::builder()
                .web_search(false)
                .image_generation(false)
                .build(),
            (Some(root), WorkspaceTools::Simulated, Provider::Claude) => {
                nanocode::claude::host_tools(root, files, simulated())
            }
            (Some(root), WorkspaceTools::Simulated, Provider::Codex) => {
                nanocode::codex_tools::host_tools(root, files, simulated())
            }
            (Some(root), WorkspaceTools::Files, Provider::Claude) => {
                nanocode::claude::host_file_tools(root, files)
            }
            (Some(root), WorkspaceTools::Files, Provider::Codex) => {
                nanocode::codex_tools::host_file_tools(root, files)
            }
            _ => Tools::builder().without_defaults().build(),
        };
        tools.map_err(|error| error.to_string())
    }
}

fn not_google(provider: &str) -> String {
    format!("Google credentials work only with the antigravity provider, not {provider}")
}

pub fn no_google_plan() -> String {
    NO_GOOGLE_PLAN.to_owned()
}

pub async fn google_harness(
    host: &Arc<crate::bridge::Host>,
    config: &Configuration,
    model: &ModelSelection,
) -> Result<AntigravityHarness, String> {
    let mut values = BTreeMap::<&'static str, String>::new();
    let mut google = GoogleConfig {
        state: Some(host.data_dir.join(GOOGLE_STATE_FILE)),
        ..GoogleConfig::default()
    };
    let vertex_base = |values: &mut BTreeMap<&'static str, String>| {
        if let Some(base) = &config.endpoint {
            values.insert("GOOGLE_VERTEX_BASE_URL", base.clone());
        }
    };
    let method = match &config.auth {
        Auth::ApiKey(key) => {
            values.insert("GEMINI_API_KEY", key.clone());
            if let Some(base) = &config.endpoint {
                values.insert("GEMINI_BASE_URL", base.clone());
            }
            AuthMethod::ApiKey
        }
        Auth::Google(GoogleCredentials::Vertex {
            project,
            location,
            access_token,
            credentials_file,
        }) => {
            if let Some(token) = access_token {
                values.insert("GOOGLE_OAUTH_ACCESS_TOKEN", token.clone());
            }
            if let Some(file) = credentials_file {
                values.insert(
                    "GOOGLE_APPLICATION_CREDENTIALS",
                    file.to_string_lossy().into_owned(),
                );
            }
            vertex_base(&mut values);
            google.project.clone_from(project);
            google.location.clone_from(location);
            AuthMethod::Vertex
        }
        Auth::Google(GoogleCredentials::VertexExpress(key)) => {
            values.insert("GOOGLE_API_KEY", key.clone());
            vertex_base(&mut values);
            AuthMethod::VertexExpress
        }
        Auth::Google(GoogleCredentials::Gateway { url, api_key }) => {
            values.insert("AGY_LLM_GATEWAY_URL", url.clone());
            if let Some(key) = api_key {
                values.insert("AGY_LLM_GATEWAY_API_KEY", key.clone());
            }
            AuthMethod::Gateway
        }
        Auth::Plan => return Err(NO_GOOGLE_PLAN.to_owned()),
        Auth::Tempo | Auth::Headers(_) => {
            return Err("the antigravity provider takes a Gemini API key or Google credentials (Vertex AI, Vertex AI express mode or an LLM gateway)".to_owned());
        }
    };
    google.method = Some(method);
    let lookup = move |name: &str| {
        values
            .get(name)
            .cloned()
            .or_else(|| std::env::var(name).ok())
    };
    let sender = Client::native_sender().map_err(|error| error.to_string())?;
    let endpoint = Endpoint::discover(&google, &lookup, sender, model.as_str()).await?;
    Ok(AntigravityHarness::from(google)
        .endpoint(endpoint)
        .app_data(host.data_dir.join("antigravity")))
}

pub fn claude_client(
    host: &Arc<crate::bridge::Host>,
    config: &Configuration,
    payments: &crate::payments::Payments,
) -> Result<ClaudeClient, String> {
    let base = config.endpoint.as_deref();
    Ok(match &config.auth {
        Auth::ApiKey(key) => ClaudeClient::messages_api(
            base.unwrap_or(ANTHROPIC_API),
            key.as_str(),
            CONNECT_TIMEOUT,
            IDLE_TIMEOUT,
        )
        .map_err(|error| error.to_string())?,
        Auth::Headers(headers) => ClaudeClient::with_auth_headers(
            Default::default(),
            messages_url(base.unwrap_or(ANTHROPIC_API)),
            header_map(headers)?,
        ),
        Auth::Plan => ClaudeClient::with_auth_provider(
            Default::default(),
            base.map_or_else(
                || nanocode::claude::ANTHROPIC_SUBSCRIPTION_MESSAGES_URL.to_owned(),
                messages_url,
            ),
            crate::plans::claude_provider(host)?,
        )
        .subscription_compatibility(),
        Auth::Tempo => {
            let url = base.map_or_else(|| Service::Anthropic.url().to_owned(), messages_url);
            ClaudeClient::with_auth_headers(Default::default(), url.clone(), Default::default())
                .with_http_layer(payments.layer(Service::Anthropic, &url)?)
        }
        Auth::Google(_) => return Err(not_google("claude")),
    })
}

pub async fn codex_client(
    host: &Arc<crate::bridge::Host>,
    config: &Configuration,
    model: Model,
    payments: &crate::payments::Payments,
) -> Result<OpenAi, String> {
    let builder = match &config.auth {
        Auth::ApiKey(key) => {
            let client = nanocode_http::Client::builder()
                .connect_timeout(CONNECT_TIMEOUT)
                .read_timeout(IDLE_TIMEOUT)
                .build()
                .map_err(|error| error.to_string())?;
            let builder = OpenAi::builder(OpenAiAuth::api_key(key.as_str())).http_client(client);
            match config.endpoint.as_deref() {
                Some(base) => builder.api_base_url(base),
                None => builder,
            }
        }
        Auth::Plan => {
            let builder = OpenAi::builder(crate::plans::chatgpt_auth(host).await?);
            match config.endpoint.as_deref() {
                Some(base) => builder
                    .api_base_url(base)
                    .transport(ResponsesTransport::Https),
                None => builder,
            }
        }
        Auth::Headers(headers) => OpenAi::builder(OpenAiAuth::gateway_managed())
            .api_base_url(config.endpoint.as_deref().unwrap_or(OPENAI_API))
            .transport(ResponsesTransport::Https)
            .http_layer(crate::payments::Headers(header_map(headers)?)),
        Auth::Tempo => {
            let base = config
                .endpoint
                .clone()
                .unwrap_or_else(|| Service::OpenAi.url().to_owned());
            OpenAi::builder(OpenAiAuth::gateway_managed())
                .api_base_url(base.clone())
                .transport(ResponsesTransport::Https)
                .http_layer(payments.layer(Service::OpenAi, &base)?)
        }
        Auth::Google(_) => return Err(not_google("codex")),
    };
    builder
        .model(model)
        .raw_api_events(false)
        .tool_input_deltas(config.stream_tool_input)
        .build()
        .map_err(|error| error.to_string())
}

pub fn codex_web_search(
    config: &Configuration,
    payments: &crate::payments::Payments,
) -> Result<Option<Arc<dyn nanocode::Tool>>, String> {
    let base = config
        .endpoint
        .clone()
        .unwrap_or_else(|| match config.auth {
            Auth::Tempo => Service::OpenAi.url().to_owned(),
            _ => OPENAI_API.to_owned(),
        });
    let transport: Arc<dyn nanocode::codex::transport::host::HostTransport> = match &config.auth {
        Auth::Plan | Auth::Google(_) => return Ok(None),
        Auth::Tempo => Arc::new(crate::payments::Transport::new(
            payments.layer(Service::OpenAi, &base)?,
        )),
        Auth::Headers(headers) => Arc::new(crate::payments::Transport::new(
            crate::payments::Headers(header_map(headers)?),
        )),
        Auth::ApiKey(key) => {
            let mut headers = nanocode_http::header::HeaderMap::new();
            let mut value = nanocode_http::header::HeaderValue::from_str(&format!("Bearer {key}"))
                .map_err(|error| error.to_string())?;
            value.set_sensitive(true);
            headers.insert(nanocode_http::header::AUTHORIZATION, value);
            Arc::new(crate::payments::Transport::new(crate::payments::Headers(
                headers,
            )))
        }
    };
    Ok(Some(Arc::new(nanocode::codex_tools::web_search_tool(
        &base, None, transport,
    ))))
}

fn messages_url(base: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.ends_with("/messages") {
        base.to_owned()
    } else {
        format!("{base}/messages")
    }
}

fn header_map(
    headers: &BTreeMap<String, String>,
) -> Result<nanocode_http::header::HeaderMap, String> {
    let mut map = nanocode_http::header::HeaderMap::new();
    for (name, value) in headers {
        let name = nanocode_http::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| error.to_string())?;
        let mut value = nanocode_http::header::HeaderValue::from_str(value)
            .map_err(|error| error.to_string())?;
        value.set_sensitive(true);
        map.insert(name, value);
    }
    Ok(map)
}
