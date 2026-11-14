// Java debugging for java-wis.
//
// Adapted from zed-extensions/java (https://github.com/zed-extensions/java),
// Copyright Java Extension Contributors, licensed under the Apache License 2.0.
// Changes: reduced to launch/attach through jdtls, pinned downloads, java-wis locator.

use std::{collections::HashMap, fs, path::PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha1::{Digest, Sha1};
use zed_extension_api::{
    self as zed, Architecture, DownloadedFileType, Os, TcpArgumentsTemplate, Worktree,
    http_client::{HttpMethod, HttpRequest},
    serde_json::{self, Map, Value, json},
};

/// `java-lsp-proxy` from zed-extensions/java sits between WiSCode and jdtls and
/// exposes an HTTP port, because extensions cannot send LSP requests themselves.
const PROXY_RELEASE: &str = "v6.8.30";
const PROXY_REPOSITORY: &str = "zed-extensions/java";
const PROXY_BINARY: &str = "java-lsp-proxy";
const PROXY_INSTALL_DIR: &str = "lsp-proxy";
/// The proxy writes `<work dir>/proxy/<sha1(project root)>` containing its HTTP port.
const PROXY_PORT_DIR: &str = "proxy";

const DEBUG_PLUGIN_VERSION: &str = "0.53.2";
const DEBUG_PLUGIN_URL: &str = "https://github.com/zed-industries/java-debug/releases/download/0.53.2/com.microsoft.java.debug.plugin-0.53.2.jar";
const DEBUG_PLUGIN_DIR: &str = "debugger";

pub const DEBUG_ADAPTER_NAME: &str = "Java";
/// Env var set by java-wis tasks (`languages/java/tasks.json`) that the locator
/// turns into a launch configuration; its value is the main class.
pub const MAIN_CLASS_ENV: &str = "JAVA_WIS_MAIN_CLASS";

const TEST_SCOPE: &str = "$Test";
const AUTO_SCOPE: &str = "$Auto";
const RUNTIME_SCOPE: &str = "$Runtime";
const SCOPES: [&str; 3] = [TEST_SCOPE, AUTO_SCOPE, RUNTIME_SCOPE];

fn proxy_executable_name() -> String {
    match zed::current_platform().0 {
        Os::Windows => format!("{PROXY_BINARY}.exe"),
        _ => PROXY_BINARY.to_string(),
    }
}

pub fn ensure_proxy() -> zed::Result<PathBuf> {
    let version_dir = PathBuf::from(PROXY_INSTALL_DIR).join(PROXY_RELEASE);
    let binary = version_dir.join(proxy_executable_name());
    if binary.is_file() {
        return Ok(binary);
    }
    let (os, architecture) = zed::current_platform();
    let (os_name, file_type, archive_extension) = match os {
        Os::Mac => ("darwin", DownloadedFileType::GzipTar, "tar.gz"),
        Os::Linux => ("linux", DownloadedFileType::GzipTar, "tar.gz"),
        Os::Windows => ("windows", DownloadedFileType::Zip, "zip"),
    };
    let architecture_name = match architecture {
        Architecture::Aarch64 => "aarch64",
        Architecture::X8664 => "x86_64",
        Architecture::X86 => return Err("java-lsp-proxy is not available for 32-bit x86".into()),
    };
    let url = format!(
        "https://github.com/{PROXY_REPOSITORY}/releases/download/{PROXY_RELEASE}/{PROXY_BINARY}-{os_name}-{architecture_name}.{archive_extension}"
    );
    zed::download_file(&url, &version_dir.to_string_lossy(), file_type)
        .map_err(|error| format!("failed to download {url}: {error}"))?;
    if !binary.is_file() {
        return Err(format!("{url} did not contain {}", binary.display()));
    }
    zed::make_file_executable(&binary.to_string_lossy())?;
    Ok(binary)
}

pub fn ensure_debug_plugin() -> zed::Result<PathBuf> {
    let jar = PathBuf::from(DEBUG_PLUGIN_DIR).join(format!(
        "com.microsoft.java.debug.plugin-{DEBUG_PLUGIN_VERSION}.jar"
    ));
    if jar.is_file() {
        return Ok(jar);
    }
    fs::create_dir_all(DEBUG_PLUGIN_DIR)
        .map_err(|error| format!("failed to create {DEBUG_PLUGIN_DIR}: {error}"))?;
    zed::download_file(
        DEBUG_PLUGIN_URL,
        &jar.to_string_lossy(),
        DownloadedFileType::Uncompressed,
    )
    .map_err(|error| format!("failed to download {DEBUG_PLUGIN_URL}: {error}"))?;
    Ok(jar)
}

/// Adds the java-debug plugin to jdtls' `bundles` so jdtls can start debug sessions.
pub fn add_debug_bundle(options: &mut Value, plugin: String) {
    let Some(object) = options.as_object_mut() else {
        return;
    };
    let bundles = object
        .entry("bundles")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Some(bundles) = bundles.as_array_mut() {
        let plugin = Value::String(plugin);
        if !bundles.contains(&plugin) {
            bundles.push(plugin);
        }
    }
}

fn lsp_request<T: DeserializeOwned>(root: &str, method: &str, params: Value) -> zed::Result<T> {
    let port_file = PathBuf::from(PROXY_PORT_DIR).join(hex::encode(Sha1::digest(
        root.trim_end_matches('/').as_bytes(),
    )));
    let port = fs::read_to_string(&port_file)
        .map_err(|_| {
            "jdtls is not ready yet: open a Java file and wait for the project to load".to_string()
        })?
        .trim()
        .parse::<u16>()
        .map_err(|error| format!("invalid port in {}: {error}", port_file.display()))?;

    let mut body = Map::new();
    body.insert("method".to_string(), Value::String(method.to_string()));
    body.insert("params".to_string(), params);
    let response = HttpRequest::builder()
        .method(HttpMethod::Post)
        .url(format!("http://localhost:{port}"))
        .body(Value::Object(body).to_string())
        .build()?
        .fetch()
        .map_err(|error| format!("failed to reach jdtls through java-lsp-proxy: {error}"))?;

    match serde_json::from_slice::<LspResponse<T>>(&response.body)
        .map_err(|error| format!("invalid response from java-lsp-proxy: {error}"))?
    {
        LspResponse::Success { result } => Ok(result),
        LspResponse::Error { error } => Err(format!("{} {}", error.code, error.message)),
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum LspResponse<T> {
    Success { result: T },
    Error { error: LspError },
}

#[derive(Deserialize)]
struct LspError {
    code: i64,
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MainClassEntry {
    main_class: String,
    project_name: String,
}

pub fn start_session(root: &str) -> zed::Result<TcpArgumentsTemplate> {
    let port = lsp_request::<u16>(
        root,
        "workspace/executeCommand",
        json!({ "command": "vscode.java.startDebugSession" }),
    )?;
    Ok(TcpArgumentsTemplate {
        host: None,
        port: Some(port),
        timeout: None,
    })
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum StringOrList {
    String(String),
    List(Vec<String>),
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct LaunchConfig {
    request: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    main_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<StringOrList>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vm_args: Option<StringOrList>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encoding: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    class_paths: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    module_paths: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    env: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_on_entry: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    no_debug: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    console: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    java_exec: Option<String>,
}

/// Fills in what jdtls knows and the user should not have to type: the project of
/// the main class and its full classpath (Maven/Gradle dependencies included).
pub fn resolve_launch_config(worktree: &Worktree, config: String) -> zed::Result<String> {
    let value: Value =
        serde_json::from_str(&config).map_err(|error| format!("invalid debug config: {error}"))?;
    if value.get("request").and_then(Value::as_str) != Some("launch") {
        return Ok(config);
    }
    let mut launch = serde_json::from_value::<LaunchConfig>(value)
        .map_err(|error| format!("invalid Java launch config: {error}"))?;
    let root = worktree.root_path();

    let filters = [launch.main_class.clone(), launch.project_name.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let entries = lsp_request::<Vec<MainClassEntry>>(
        &root,
        "workspace/executeCommand",
        json!({ "command": "vscode.java.resolveMainClass", "arguments": filters }),
    )?
    .into_iter()
    .filter(|entry| {
        launch
            .main_class
            .as_ref()
            .is_none_or(|main_class| &entry.main_class == main_class)
            && launch
                .project_name
                .as_ref()
                .is_none_or(|project| &entry.project_name == project)
    })
    .collect::<Vec<_>>();
    if entries.len() > 1 && launch.project_name.is_none() {
        return Err(
            "several main classes match; set \"mainClass\" and \"projectName\" in the debug configuration"
                .to_string(),
        );
    }
    if let Some(entry) = entries.into_iter().next() {
        launch.main_class = Some(entry.main_class);
        launch.project_name = Some(entry.project_name);
    }

    let mut class_paths = launch
        .class_paths
        .take()
        .unwrap_or_else(|| vec![AUTO_SCOPE.to_string()]);
    if class_paths
        .iter()
        .any(|class_path| SCOPES.contains(&class_path.as_str()))
    {
        let scope = if class_paths.iter().any(|class_path| class_path == TEST_SCOPE) {
            Some("test")
        } else if class_paths.iter().any(|class_path| class_path == RUNTIME_SCOPE)
            && !class_paths.iter().any(|class_path| class_path == AUTO_SCOPE)
        {
            Some("runtime")
        } else {
            None
        };
        let resolved = lsp_request::<Vec<Vec<String>>>(
            &root,
            "workspace/executeCommand",
            json!({
                "command": "vscode.java.resolveClasspath",
                "arguments": [launch.main_class, launch.project_name, scope]
            }),
        )?;
        for paths in resolved {
            class_paths.extend(paths);
        }
    }
    class_paths.retain(|class_path| !SCOPES.contains(&class_path.as_str()));
    class_paths.dedup();
    launch.class_paths = Some(class_paths);
    launch.cwd = launch.cwd.or_else(|| Some(root.clone()));

    Ok(serde_json::to_string(&launch)
        .map_err(|error| format!("failed to encode Java launch config: {error}"))?
        .replace("${workspaceFolder}", &root))
}

/// The launch configuration for a java-wis task, before task variables are substituted.
pub fn launch_config_for_main_class(main_class: &str) -> String {
    json!({ "request": "launch", "mainClass": main_class }).to_string()
}

pub fn attach_config(port: u16) -> String {
    json!({ "request": "attach", "hostName": "localhost", "port": port }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_debug_bundle_keeps_existing_bundles_once() {
        let mut options = json!({ "bundles": ["a.jar"], "settings": {} });
        add_debug_bundle(&mut options, "debug.jar".to_string());
        add_debug_bundle(&mut options, "debug.jar".to_string());
        assert_eq!(options["bundles"], json!(["a.jar", "debug.jar"]));
    }

    #[test]
    fn test_launch_config_for_main_class_keeps_task_variables() {
        let config = launch_config_for_main_class("${ZED_CUSTOM_java_package_name}.Main");
        let value: Value = serde_json::from_str(&config).expect("valid json");
        assert_eq!(value["mainClass"], "${ZED_CUSTOM_java_package_name}.Main");
        assert_eq!(value["request"], "launch");
    }
}
