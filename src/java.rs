use std::{
    fs,
    path::{Path, PathBuf},
};

mod debugger;

use zed_extension_api::{
    self as zed, DebugAdapterBinary, DebugConfig, DebugRequest, DebugScenario,
    DebugTaskDefinition, DownloadedFileType, LanguageServerId, LanguageServerInstallationStatus,
    Os, Result, StartDebuggingRequestArguments, StartDebuggingRequestArgumentsRequest,
    TaskTemplate,
    http_client::{HttpMethod, HttpRequest},
    serde_json::{self, json},
    settings::LspSettings,
};

const SERVER_ID: &str = "jdtls";
const SNAPSHOTS_URL: &str = "https://download.eclipse.org/jdtls/snapshots";
const INSTALL_DIR_PREFIX: &str = "jdtls-";
const MAVEN_CENTRAL: &str = "https://repo.maven.apache.org/maven2";
const MAVEN_DIR: &str = "maven";
const GRADLE_DIR: &str = "gradle";
const LOMBOK_DIR: &str = "lombok";
const ENV_DIR: &str = "env";
/// The JDWP port the Maven/Gradle debug run configurations open (see `run_configurations/java.json`).
const DEFAULT_DEBUG_PORT: u16 = 5005;

struct JavaExtension {
    cached_install_dir: Option<PathBuf>,
    build_tools_checked: bool,
}

fn fetch_text(url: &str) -> Result<String> {
    let request = HttpRequest::builder()
        .method(HttpMethod::Get)
        .url(url)
        .build()?;
    let response = request.fetch()?;
    String::from_utf8(response.body)
        .map_err(|error| format!("invalid response from {url}: {error}"))
}

/// Picks the newest stable version from a Maven Central `maven-metadata.xml`.
/// `<release>` is not used because it may point at a release candidate.
fn latest_release(group_path: &str, artifact: &str) -> Result<String> {
    let metadata = fetch_text(&format!(
        "{MAVEN_CENTRAL}/{group_path}/{artifact}/maven-metadata.xml"
    ))?;
    latest_stable_version(&metadata)
        .ok_or_else(|| format!("no stable release found for {artifact}"))
}

fn latest_stable_version(metadata: &str) -> Option<String> {
    metadata
        .split("<version>")
        .skip(1)
        .filter_map(|chunk| chunk.split("</version>").next())
        .map(str::trim)
        .filter(|version| {
            !version.is_empty()
                && version
                    .chars()
                    .all(|character| character.is_ascii_digit() || character == '.')
        })
        .max_by_key(|version| {
            version
                .split('.')
                .map(|part| part.parse::<u64>().unwrap_or(0))
                .collect::<Vec<_>>()
        })
        .map(str::to_string)
}

fn first_entry(dir: &str) -> Option<PathBuf> {
    let mut entries = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();
    entries.pop()
}

/// Installs Apache Maven into the extension work directory, where WiSCode's run
/// configurations look for it when neither `mvnw` nor `mvn` is available.
fn ensure_maven() -> Result<()> {
    if first_entry(MAVEN_DIR).is_some() {
        return Ok(());
    }
    let version = latest_release("org/apache/maven", "apache-maven")?;
    let target = format!("{MAVEN_DIR}/{version}");
    zed::download_file(
        &format!(
            "{MAVEN_CENTRAL}/org/apache/maven/apache-maven/{version}/apache-maven-{version}-bin.zip"
        ),
        &target,
        DownloadedFileType::Zip,
    )
    .map_err(|error| format!("failed to download Maven {version}: {error}"))?;
    if zed::current_platform().0 != Os::Windows {
        zed::make_file_executable(&format!("{target}/apache-maven-{version}/bin/mvn"))?;
    }
    Ok(())
}

fn ensure_gradle() -> Result<()> {
    if first_entry(GRADLE_DIR).is_some() {
        return Ok(());
    }
    let current: serde_json::Value =
        serde_json::from_str(&fetch_text("https://services.gradle.org/versions/current")?)
            .map_err(|error| format!("invalid Gradle version response: {error}"))?;
    let version = current["version"]
        .as_str()
        .ok_or_else(|| "missing Gradle version".to_string())?
        .to_string();
    let download_url = current["downloadUrl"]
        .as_str()
        .ok_or_else(|| "missing Gradle download URL".to_string())?;
    let target = format!("{GRADLE_DIR}/{version}");
    zed::download_file(download_url, &target, DownloadedFileType::Zip)
        .map_err(|error| format!("failed to download Gradle {version}: {error}"))?;
    if zed::current_platform().0 != Os::Windows {
        zed::make_file_executable(&format!("{target}/gradle-{version}/bin/gradle"))?;
    }
    Ok(())
}

/// jdtls needs Lombok as a Java agent to understand generated members.
fn ensure_lombok() -> Result<PathBuf> {
    if let Some(existing) = first_entry(LOMBOK_DIR) {
        return Ok(existing);
    }
    let version = latest_release("org/projectlombok", "lombok")?;
    fs::create_dir_all(LOMBOK_DIR)
        .map_err(|error| format!("failed to create {LOMBOK_DIR}: {error}"))?;
    let target = format!("{LOMBOK_DIR}/lombok-{version}.jar");
    zed::download_file(
        &format!("{MAVEN_CENTRAL}/org/projectlombok/lombok/{version}/lombok-{version}.jar"),
        &target,
        DownloadedFileType::Uncompressed,
    )
    .map_err(|error| format!("failed to download Lombok {version}: {error}"))?;
    Ok(PathBuf::from(target))
}

fn has_file(worktree: &zed::Worktree, path: &str) -> bool {
    worktree.read_text_file(path).is_ok()
}

impl JavaExtension {
    fn ensure_build_tools(&mut self, worktree: &zed::Worktree) {
        if self.build_tools_checked {
            return;
        }
        self.build_tools_checked = true;
        let wrapper_exists = |name: &str| has_file(worktree, name);
        if has_file(worktree, "pom.xml")
            && !wrapper_exists("mvnw")
            && worktree.which("mvn").is_none()
            && let Err(error) = ensure_maven()
        {
            eprintln!("java-wis: {error}");
        }
        if (has_file(worktree, "build.gradle") || has_file(worktree, "build.gradle.kts"))
            && !wrapper_exists("gradlew")
            && worktree.which("gradle").is_none()
            && let Err(error) = ensure_gradle()
        {
            eprintln!("java-wis: {error}");
        }
    }

    fn latest_snapshot_name() -> Result<String> {
        let request = HttpRequest::builder()
            .method(HttpMethod::Get)
            .url(format!("{SNAPSHOTS_URL}/latest.txt"))
            .build()?;
        let response = request.fetch()?;
        let name = String::from_utf8(response.body)
            .map_err(|error| format!("invalid latest.txt: {error}"))?
            .trim()
            .to_string();
        if !name.ends_with(".tar.gz") {
            return Err(format!("unexpected jdtls snapshot name: {name:?}"));
        }
        Ok(name)
    }

    fn installed_dirs() -> Vec<PathBuf> {
        fs::read_dir(".")
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| {
                        entry
                            .file_name()
                            .to_str()
                            .is_some_and(|name| name.starts_with(INSTALL_DIR_PREFIX))
                    })
                    .map(|entry| entry.path())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn config_dir(install_dir: &Path) -> PathBuf {
        install_dir.join(match zed::current_platform().0 {
            Os::Mac => "config_mac",
            Os::Linux => "config_linux",
            Os::Windows => "config_win",
        })
    }

    /// jdtls cannot start without the `config.ini` of its configuration directory, and that
    /// file has been seen missing from an existing install, so its presence decides whether
    /// the install is usable rather than only the existence of `plugins`.
    fn is_complete_install(install_dir: &Path) -> bool {
        install_dir.join("plugins").exists()
            && Self::config_dir(install_dir).join("config.ini").exists()
    }

    /// Downloads the latest jdtls snapshot, falling back to any previously installed
    /// copy when offline so that Java keeps working without network access.
    fn ensure_installed(&mut self, language_server_id: &LanguageServerId) -> Result<PathBuf> {
        if let Some(dir) = &self.cached_install_dir
            && Self::is_complete_install(dir)
        {
            return Ok(dir.clone());
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &LanguageServerInstallationStatus::CheckingForUpdate,
        );
        let snapshot_name = match Self::latest_snapshot_name() {
            Ok(name) => name,
            Err(error) => {
                if let Some(existing) = Self::installed_dirs().into_iter().next() {
                    self.cached_install_dir = Some(existing.clone());
                    return Ok(existing);
                }
                return Err(error);
            }
        };
        let version = snapshot_name.trim_end_matches(".tar.gz");
        let install_dir = PathBuf::from(format!("{INSTALL_DIR_PREFIX}{version}"));

        if !Self::is_complete_install(&install_dir) {
            if install_dir.exists() {
                fs::remove_dir_all(&install_dir)
                    .map_err(|error| format!("failed to remove incomplete jdtls install: {error}"))?;
            }
            zed::set_language_server_installation_status(
                language_server_id,
                &LanguageServerInstallationStatus::Downloading,
            );
            zed::download_file(
                &format!("{SNAPSHOTS_URL}/{snapshot_name}"),
                &install_dir.to_string_lossy(),
                DownloadedFileType::GzipTar,
            )
            .map_err(|error| format!("failed to download jdtls: {error}"))?;

            for old_dir in Self::installed_dirs() {
                if old_dir.file_name() != install_dir.file_name() {
                    fs::remove_dir_all(&old_dir).ok();
                }
            }
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &LanguageServerInstallationStatus::None,
        );
        self.cached_install_dir = Some(install_dir.clone());
        Ok(install_dir)
    }

    fn find_launcher_jar(install_dir: &Path) -> Result<PathBuf> {
        let plugins = install_dir.join("plugins");
        fs::read_dir(&plugins)
            .map_err(|error| format!("failed to read {}: {error}", plugins.display()))?
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with("org.eclipse.equinox.launcher_") && name.ends_with(".jar")
                    })
            })
            .ok_or_else(|| "jdtls launcher jar not found".to_string())
    }

    fn find_java(worktree: &zed::Worktree) -> Result<String> {
        let env = worktree.shell_env();
        if let Some((_, java_home)) = env.iter().find(|(key, _)| key == "JAVA_HOME") {
            let executable = if zed::current_platform().0 == Os::Windows {
                "java.exe"
            } else {
                "java"
            };
            let candidate = PathBuf::from(java_home).join("bin").join(executable);
            if candidate.exists() {
                return Ok(candidate.to_string_lossy().into_owned());
            }
        }
        worktree.which("java").ok_or_else(|| {
            "Java (JDK 21+) was not found. Install a JDK and set JAVA_HOME or add `java` to PATH."
                .to_string()
        })
    }

    /// Each project gets its own jdtls workspace so that index data from
    /// unrelated projects never mixes.
    fn data_dir(worktree: &zed::Worktree) -> PathBuf {
        let root = worktree.root_path();
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in root.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        let name = root
            .rsplit(['/', '\\'])
            .find(|segment| !segment.is_empty())
            .unwrap_or("workspace");
        PathBuf::from("jdtls-workspaces").join(format!("{name}-{hash:016x}"))
    }
}

impl JavaExtension {
    fn jdtls_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary_settings = LspSettings::for_worktree(SERVER_ID, worktree)
            .ok()
            .and_then(|settings| settings.binary);
        if let Some(path) = binary_settings
            .as_ref()
            .and_then(|binary| binary.path.clone())
        {
            return Ok(zed::Command {
                command: path,
                args: binary_settings
                    .and_then(|binary| binary.arguments)
                    .unwrap_or_default(),
                env: worktree.shell_env(),
            });
        }
        if let Some(path) = worktree.which("jdtls") {
            return Ok(zed::Command {
                command: path,
                args: vec!["-data".to_string(), absolute(&Self::data_dir(worktree))?],
                env: worktree.shell_env(),
            });
        }

        let java = Self::find_java(worktree)?;
        let install_dir = self.ensure_installed(language_server_id)?;
        let launcher = Self::find_launcher_jar(&install_dir)?;
        let config_dir = Self::config_dir(&install_dir);
        let data_dir = Self::data_dir(worktree);
        fs::create_dir_all(&data_dir).ok();

        let lombok = match ensure_lombok() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("java-wis: {error}");
                install_dir.join("lombok.jar")
            }
        };
        let mut args = vec![
            "-Declipse.application=org.eclipse.jdt.ls.core.id1".to_string(),
            "-Dosgi.bundles.defaultStartLevel=4".to_string(),
            "-Declipse.product=org.eclipse.jdt.ls.core.product".to_string(),
            "-Dlog.level=WARNING".to_string(),
            "-Xms256m".to_string(),
            "-XX:+UseParallelGC".to_string(),
            "--add-modules=ALL-SYSTEM".to_string(),
            "--add-opens".to_string(),
            "java.base/java.util=ALL-UNNAMED".to_string(),
            "--add-opens".to_string(),
            "java.base/java.lang=ALL-UNNAMED".to_string(),
        ];
        if lombok.exists() {
            args.push(format!("-javaagent:{}", absolute(&lombok)?));
        }
        args.extend([
            "-jar".to_string(),
            absolute(&launcher)?,
            "-configuration".to_string(),
            absolute(&config_dir)?,
            "-data".to_string(),
            absolute(&data_dir)?,
        ]);

        Ok(zed::Command {
            command: java,
            args,
            env: worktree.shell_env(),
        })
    }
}

impl zed::Extension for JavaExtension {
    fn new() -> Self {
        Self {
            cached_install_dir: None,
            build_tools_checked: false,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        if language_server_id.as_ref() != SERVER_ID {
            return Err(format!("unknown language server {language_server_id}"));
        }
        self.ensure_build_tools(worktree);
        write_run_environment(worktree, &merged_settings(worktree));

        let command = self.jdtls_command(language_server_id, worktree)?;
        // Without the proxy jdtls still works for editing; only Run/Debug through the
        // debugger becomes unavailable.
        let proxy = match debugger::ensure_proxy() {
            Ok(proxy) => proxy,
            Err(error) => {
                eprintln!("java-wis: {error}");
                return Ok(command);
            }
        };
        let mut args = vec![absolute(&PathBuf::from("."))?, command.command];
        args.extend(command.args);
        Ok(zed::Command {
            command: absolute(&proxy)?,
            args,
            env: command.env,
        })
    }

    fn language_server_initialization_options(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<serde_json::Value>> {
        let user_options = LspSettings::for_worktree(SERVER_ID, worktree)
            .ok()
            .and_then(|settings| settings.initialization_options);
        let mut options = user_options.unwrap_or_else(|| {
            json!({
                "extendedClientCapabilities": {
                    "classFileContentsSupport": true
                },
                "settings": merged_settings(worktree)
            })
        });
        match debugger::ensure_debug_plugin().and_then(|plugin| absolute(&plugin)) {
            Ok(plugin) => debugger::add_debug_bundle(&mut options, plugin),
            Err(error) => eprintln!("java-wis: {error}"),
        }
        Ok(Some(options))
    }

    fn get_dap_binary(
        &mut self,
        adapter_name: String,
        config: DebugTaskDefinition,
        _user_provided_debug_adapter_path: Option<String>,
        worktree: &zed::Worktree,
    ) -> Result<DebugAdapterBinary> {
        if adapter_name != debugger::DEBUG_ADAPTER_NAME {
            return Err(format!("java-wis cannot start debug adapter {adapter_name}"));
        }
        let request_value = serde_json::from_str::<serde_json::Value>(&config.config)
            .map_err(|error| format!("invalid debug config: {error}"))?;
        let request = self.dap_request_kind(adapter_name, request_value)?;
        let root = worktree.root_path();
        Ok(DebugAdapterBinary {
            command: None,
            arguments: Vec::new(),
            envs: Vec::new(),
            cwd: Some(root.clone()),
            connection: Some(zed::resolve_tcp_template(debugger::start_session(&root)?)?),
            request_args: StartDebuggingRequestArguments {
                configuration: debugger::resolve_launch_config(worktree, config.config)?,
                request,
            },
        })
    }

    fn dap_request_kind(
        &mut self,
        adapter_name: String,
        config: serde_json::Value,
    ) -> Result<StartDebuggingRequestArgumentsRequest> {
        if adapter_name != debugger::DEBUG_ADAPTER_NAME {
            return Err(format!("java-wis does not handle debug adapter {adapter_name}"));
        }
        match config.get("request").and_then(|request| request.as_str()) {
            Some("launch") => Ok(StartDebuggingRequestArgumentsRequest::Launch),
            Some("attach") => Ok(StartDebuggingRequestArgumentsRequest::Attach),
            Some(other) => Err(format!("unknown Java debug request {other:?}")),
            None => Err("the Java debug config needs \"request\": \"launch\" or \"attach\"".into()),
        }
    }

    fn dap_config_to_scenario(&mut self, config: DebugConfig) -> Result<DebugScenario> {
        let (label, debug_config) = match config.request {
            DebugRequest::Launch(launch) => (
                config.label,
                debugger::launch_config_for_main_class(&launch.program),
            ),
            DebugRequest::Attach(_) => (config.label, debugger::attach_config(DEFAULT_DEBUG_PORT)),
        };
        Ok(DebugScenario {
            label,
            adapter: config.adapter,
            build: None,
            config: debug_config,
            tcp_connection: None,
        })
    }

    fn dap_locator_create_scenario(
        &mut self,
        _locator_name: String,
        build_task: TaskTemplate,
        resolved_label: String,
        debug_adapter_name: String,
    ) -> Option<DebugScenario> {
        if debug_adapter_name != debugger::DEBUG_ADAPTER_NAME {
            return None;
        }
        let (_, main_class) = build_task
            .env
            .iter()
            .find(|(key, _)| key == debugger::MAIN_CLASS_ENV)?;
        Some(DebugScenario {
            label: resolved_label,
            adapter: debug_adapter_name,
            build: None,
            config: debugger::launch_config_for_main_class(main_class),
            tcp_connection: None,
        })
    }

    fn language_server_workspace_configuration(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<serde_json::Value>> {
        let settings = merged_settings(worktree);
        write_run_environment(worktree, &settings);
        Ok(Some(settings))
    }
}

/// User settings are layered over the defaults instead of replacing them, so setting
/// one key (for example `java.configuration.runtimes`) keeps the others.
fn merged_settings(worktree: &zed::Worktree) -> serde_json::Value {
    let mut settings = default_settings();
    if let Some(user_settings) = LspSettings::for_worktree(SERVER_ID, worktree)
        .ok()
        .and_then(|settings| settings.settings)
    {
        merge_json(&mut settings, user_settings);
    }
    settings
}

fn merge_json(base: &mut serde_json::Value, overlay: serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base), serde_json::Value::Object(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(&key) {
                    Some(existing) => merge_json(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

/// The JDK marked `"default": true` in jdtls' standard `java.configuration.runtimes`.
fn default_runtime_path(settings: &serde_json::Value) -> Option<String> {
    settings
        .pointer("/java/configuration/runtimes")?
        .as_array()?
        .iter()
        .find(|runtime| runtime.get("default").and_then(|value| value.as_bool()) == Some(true))?
        .get("path")?
        .as_str()
        .map(str::to_string)
}

/// Must produce the same value as WiSCode's `run_configurations::root_key` for the
/// `$ROOT_KEY` placeholder used by `env_file` in `run_configurations/java.json`.
fn root_key(root: &str) -> String {
    root.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

/// Run configurations read this file when they start, so Maven and Gradle use the
/// same JDK that jdtls compiles the project against.
fn write_run_environment(worktree: &zed::Worktree, settings: &serde_json::Value) {
    let env_file = PathBuf::from(ENV_DIR).join(format!("{}.env", root_key(&worktree.root_path())));
    let result = match default_runtime_path(settings) {
        Some(java_home) => fs::create_dir_all(ENV_DIR)
            .and_then(|()| fs::write(&env_file, format!("JAVA_HOME={java_home}\n"))),
        None if env_file.exists() => fs::remove_file(&env_file),
        None => Ok(()),
    };
    if let Err(error) = result {
        eprintln!("java-wis: failed to update {}: {error}", env_file.display());
    }
}

fn default_settings() -> serde_json::Value {
    json!({
        "java": {
            "configuration": { "updateBuildConfiguration": "automatic" },
            "import": {
                "maven": { "enabled": true },
                "gradle": { "enabled": true }
            },
            "implementationsCodeLens": { "enabled": false },
            "referencesCodeLens": { "enabled": false },
            "signatureHelp": { "enabled": true },
            "inlayHints": { "parameterNames": { "enabled": "literals" } },
            "completion": {
                "favoriteStaticMembers": [
                    "org.junit.jupiter.api.Assertions.*",
                    "org.mockito.Mockito.*",
                    "io.restassured.RestAssured.*"
                ]
            }
        }
    })
}

fn absolute(path: &PathBuf) -> Result<String> {
    let current_dir =
        std::env::current_dir().map_err(|error| format!("no working directory: {error}"))?;
    Ok(current_dir.join(path).to_string_lossy().into_owned())
}

zed::register_extension!(JavaExtension);

#[cfg(test)]
mod tests {
    use super::{default_runtime_path, latest_stable_version, merge_json, root_key};
    use zed_extension_api::serde_json::json;

    #[test]
    fn test_user_runtimes_keep_default_settings_and_pick_default_jdk() {
        let mut settings = json!({
            "java": { "configuration": { "updateBuildConfiguration": "automatic" } }
        });
        merge_json(
            &mut settings,
            json!({ "java": { "configuration": { "runtimes": [
                { "name": "JavaSE-21", "path": "C:/jdk-21" },
                { "name": "JavaSE-17", "path": "C:/jdk-17", "default": true }
            ] } } }),
        );
        assert_eq!(
            settings.pointer("/java/configuration/updateBuildConfiguration"),
            Some(&json!("automatic"))
        );
        assert_eq!(default_runtime_path(&settings).as_deref(), Some("C:/jdk-17"));
    }

    #[test]
    fn test_root_key_matches_wiscode() {
        assert_eq!(root_key(r"D:\Code\Core-Parent"), "dcodecoreparent");
    }

    #[test]
    fn test_latest_stable_version_skips_release_candidates() {
        let metadata = "<versions><version>3.9.9</version><version>3.9.11</version><version>4.0.0-rc-7</version></versions><release>4.0.0-rc-7</release>";
        assert_eq!(latest_stable_version(metadata).as_deref(), Some("3.9.11"));
    }
}
