//! Instances on the VM: created, reused and recreated as their mounts
//! change, a harness installed into the tools volume through the gateway,
//! and pi's `models.json` written into the home volume.
//!
//! Ignored by default; needs a running machine, both images and internet
//! access: `cargo test -p harness-core --test instance_vm -- --ignored --nocapture`

use std::path::PathBuf;

use harness_core::config::{FileMode, HarnessConfig, NetworkPreset, Project};
use harness_core::gateway;
use harness_core::image::{BASE_IMAGE, GATEWAY_IMAGE, ImageStatus};
use harness_core::instance::{self, InstanceState, TOOLS_DIR};
use harness_core::pi::{self, Existing};
use harness_core::podman::{DEFAULT_MACHINE, PodmanProvider};
use harness_core::project::ProjectName;
use harness_core::provider::{ExecOutput, ExecSpec, Provider, Resources};
use harness_core::server::{DeviceCredential, ModelInfo};
use serde_json::{Map, Value, json};

fn object(value: &Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

struct Cleanup<'a> {
    provider: &'a PodmanProvider,
    project: &'a Project,
    harness: &'a HarnessConfig,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let p = self.provider;
        let name = &self.project.name;
        // The tools volume is shared with real projects: only the test's own
        // program goes, through the installer, the only writer of the volume.
        let uninstall = HarnessConfig {
            install: format!("rm -f {TOOLS_DIR}/bin/{}", self.harness.name),
            ..self.harness.clone()
        };
        let _ = instance::install_harness(p, self.project, &uninstall);
        let _ = p.remove_container(&name.instance());
        let _ = p.remove_container(&name.gateway());
        let _ = p.remove_container(&name.installer());
        for volume in [
            name.workspace_volume(),
            name.home_volume(),
            self.harness.config_volume(),
        ] {
            let _ = p.remove_volume(&volume);
        }
        let _ = p.remove_network(&name.network());
        if let Ok(exchange) = p.exchange()
            && let Ok(dir) = exchange.create_dir(name.exchange_dir())
        {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[test]
#[ignore = "needs the running Podman machine, both images and internet access"]
fn instances_install_harnesses_and_keep_their_volumes() {
    let machine = std::env::var("HARNESS_MACHINE").unwrap_or_else(|_| DEFAULT_MACHINE.into());
    let provider = PodmanProvider::for_machine(&machine).unwrap();
    for image in [BASE_IMAGE, GATEWAY_IMAGE] {
        assert_eq!(
            image.status(&provider).unwrap(),
            ImageStatus::Current,
            "run `harness image build` first"
        );
    }
    let name = ProjectName::new(&format!("insttest-{}", std::process::id())).unwrap();
    // Preset off: the installer only reaches npm through the temporary grant.
    let project = Project {
        name: name.clone(),
        file: PathBuf::from("unused.toml"),
        path: PathBuf::from("/unused"),
        harness: "pi".into(),
        mode: FileMode::Isolated,
        network: NetworkPreset::Off,
        ask: false,
        allow: vec![],
        deny: vec![],
        llm_apis: vec![],
        resources: Resources::default(),
    };
    let program = format!("itest-{}", std::process::id());
    let harness = HarnessConfig {
        name: program.clone(),
        description: None,
        install: format!(
            "set -e\n\
             curl -sS --max-time 20 -o /dev/null https://registry.npmjs.org/\n\
             mkdir -p {TOOLS_DIR}/bin\n\
             printf '#!/bin/sh\\necho installed\\n' > {TOOLS_DIR}/bin/{program}\n\
             chmod +x {TOOLS_DIR}/bin/{program}"
        ),
        command: vec![program.clone()],
        persist: vec!["~/.itest".into()],
        llm: None,
        adapter: None,
    };
    let _cleanup = Cleanup {
        provider: &provider,
        project: &project,
        harness: &harness,
    };

    gateway::ensure_running(&provider, &project, None).unwrap();
    assert!(instance::ensure_running(&provider, &project, &[]).unwrap());
    assert!(!instance::ensure_running(&provider, &project, &[]).unwrap());
    assert_eq!(
        instance::state(&provider, &name).unwrap(),
        InstanceState::Running
    );

    let run = |script: &str| -> ExecOutput {
        provider
            .exec_output(&ExecSpec {
                container: name.instance(),
                command: vec!["bash".into(), "-lc".into(), script.into()],
                ..ExecSpec::default()
            })
            .unwrap()
    };
    // The agent can neither reach npm (preset off) nor write the tools.
    assert!(!instance::harness_installed(&provider, &name, &harness).unwrap());
    let npm = run("curl -sS --max-time 10 -o /dev/null https://registry.npmjs.org/");
    assert_ne!(npm.code, 0, "npm reachable under preset off");
    let write = run(&format!("touch {TOOLS_DIR}/agent-was-here"));
    assert_ne!(write.code, 0, "tools volume writable from the instance");

    instance::install_harness(&provider, &project, &harness).unwrap();
    assert!(instance::harness_installed(&provider, &name, &harness).unwrap());
    // On the PATH of a login shell, as `harness run` starts it.
    assert_eq!(run(&program).stdout.trim(), "installed");

    // A further harness adds its volume, which recreates the container; the
    // workspace and home volumes keep their contents.
    let marker = run("echo kept > /workspace/marker && echo kept > ~/marker");
    assert_eq!(marker.code, 0, "{}", marker.stderr);
    assert!(instance::ensure_running(&provider, &project, &[&harness]).unwrap());
    let kept = run("cat /workspace/marker ~/marker && touch ~/.itest/state && echo ok");
    assert_eq!(kept.stdout, "kept\nkept\nok\n", "{}", kept.stderr);

    let mut credential = DeviceCredential {
        id: "c1".into(),
        kind: "ollama".into(),
        label: "Ollama".into(),
        base_url: Some("http://localhost:11434/v1".into()),
        models: None,
    };
    credential.models = Some(vec![ModelInfo {
        id: "qwen".into(),
        name: "Qwen".into(),
    }]);
    let models = pi::models_json(&[credential]);
    let instance_name = name.instance();
    let read = || pi::read_models(&provider, &instance_name).unwrap();
    assert_eq!(read(), Existing::Missing);
    assert!(
        pi::wire(&provider, &instance_name, &models)
            .unwrap()
            .is_empty()
    );
    assert_eq!(read(), Existing::Json(object(&models.json)));
    // The user's own provider survives logging out; harness's entries go.
    let own = json!({ "providers": { "local": { "baseUrl": "http://localhost:1234/v1" } } });
    let (both, _) = pi::merge(&Existing::Json(object(&own)), &models);
    pi::write_models(&provider, &instance_name, &both, false).unwrap();
    assert!(pi::unwire(&provider, &instance_name).unwrap());
    assert_eq!(read(), Existing::Json(object(&own)));
    assert!(!pi::unwire(&provider, &instance_name).unwrap());
    // A broken file is kept as a backup when wiring replaces it.
    let broken = run(&format!("printf '{{oops' > {}", pi::MODELS_FILE));
    assert_eq!(broken.code, 0, "{}", broken.stderr);
    assert_eq!(read(), Existing::Invalid);
    pi::wire(&provider, &instance_name, &models).unwrap();
    assert_eq!(read(), Existing::Json(object(&models.json)));
    assert_eq!(run(&format!("cat {}.bak", pi::MODELS_FILE)).stdout, "{oops");
    assert!(pi::unwire(&provider, &instance_name).unwrap());
    assert_eq!(read(), Existing::Missing);

    // Stop keeps the container; the next start reuses it.
    instance::stop(&provider, &name).unwrap();
    assert_eq!(
        instance::state(&provider, &name).unwrap(),
        InstanceState::Stopped
    );
    assert!(!instance::ensure_running(&provider, &project, &[&harness]).unwrap());
    assert_eq!(run("cat ~/.itest/state && echo ok").stdout, "ok\n");
}
