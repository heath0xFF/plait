//! MCP settings use injected metadata; these scenes never contact a host or server.
use super::{Harness, fixture_repository, isolated};
use crate::ui::settings_agent::AgentView;
use plait_core::host::agent::{AgentResources, McpServer, McpTransport};
use std::{path::Path, sync::Arc};

fn resources(checkout: &Path) -> AgentResources {
    let server = |name: &str, transport, enabled, needs_sign_in, signed_in| McpServer {
        name: name.into(),
        transport: Some(transport),
        source: "/opt/sample-agent/mcp.json".into(),
        override_source: None,
        program: (transport == McpTransport::Stdio).then(|| "uvx".into()),
        host: (transport == McpTransport::Http).then(|| "docs.example".into()),
        enabled,
        can_enable: true,
        needs_sign_in,
        signed_in,
        disabled_reason: (!enabled).then(|| "Off until enabled for this sandboxed project.".into()),
        problem: None,
    };
    let mut invalid = server("invalid-config", McpTransport::Http, false, None, false);
    invalid.transport = None;
    invalid.host = None;
    invalid.can_enable = false;
    invalid.problem = Some("url must be an http or https URL".into());
    invalid.disabled_reason = invalid.problem.clone();
    AgentResources {
        checkout: checkout.into(),
        trusted: false,
        instructions: None,
        skills: vec![],
        templates: vec![],
        problems: vec![],
        mcp_servers: vec![
            server("local-docs", McpTransport::Stdio, true, Some(false), false),
            server(
                "project-search-with-a-long-name-that-still-leaves-room-for-the-switch",
                McpTransport::Http,
                false,
                None,
                false,
            ),
            server("design", McpTransport::Http, true, Some(true), false),
            server("issues", McpTransport::Http, true, Some(true), true),
            invalid,
        ],
    }
}

pub(super) fn mcp_walk(harness: &mut Harness, repository: &Path, suffix: &str) {
    harness.connect(repository, false);
    harness.update(|plait, window, cx| {
        plait.show_project_settings(window, cx);
        plait.policy.key = Some((plait.host.epoch, plait.host.checkout.clone()));
        plait.policy.value = Some(plait_core::host::policy::CommandPolicy::Sandboxed);
        plait.agent.key = Some((plait.host.epoch, plait.host.checkout.clone()));
        plait.agent.view = Some(Arc::new(AgentView::new(resources(&plait.host.checkout))));
        plait
            .settings_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(-180.)));
    });
    harness.capture(&format!("project-settings-mcp-{suffix}"));
    harness.update(|plait, _, _| {
        let mut reply = resources(&plait.host.checkout);
        reply.mcp_servers.clear();
        plait.agent.view = Some(Arc::new(AgentView::new(reply)));
        plait
            .settings_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
    });
    harness.capture(&format!("project-settings-no-mcp-{suffix}"));
    harness.update(|plait, _, _| plait.settings_open = false);
}

#[test]
fn mcp_rows_explain_configuration_and_sign_in() {
    let view = AgentView::new(resources(Path::new("/repo")));
    let rows = &view.mcp_servers;
    assert_eq!(rows[0].state, "Listed");
    assert!(rows[0].enabled && rows[0].hover.contains("uvx"));
    assert_eq!(rows[1].state, "Off");
    assert!(rows[1].hover.contains("sandboxed project"));
    assert_eq!(rows[2].state, "Sign in with Pi");
    assert_eq!(rows[3].state, "Sign-in saved");
    assert!(rows[3].hover.contains("validity has not been checked"));
    assert_eq!(rows[4].state, "Invalid");
    let mut old = resources(Path::new("/repo"));
    old.mcp_servers.clear();
    assert!(AgentView::new(old).mcp_servers.is_empty());
}

#[test]
fn mcp_replies_are_guarded_by_checkout_connection_and_refresh() {
    if !isolated("mcp::mcp_replies_are_guarded_by_checkout_connection_and_refresh") {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("plait-mcp-view-{}", std::process::id()));
    let repository = fixture_repository(&scratch);
    let mut harness = Harness::new(&repository, None, 900., 600.);
    harness.update(|plait, _, _| {
        let key = (plait.host.epoch, plait.host.checkout.clone());
        plait.agent.key = Some(key.clone());
        let revision = plait.agent.revision;
        let reply = || Ok(AgentView::new(resources(&key.1)));
        assert!(plait.accept_agent(&key, revision, reply()));
        assert_eq!(plait.agent.view.as_ref().unwrap().mcp_servers.len(), 5);
        assert!(!plait.accept_agent(&(key.0 + 1, key.1.clone()), revision, reply()));
        assert!(!plait.accept_agent(&(key.0, "/another".into()), revision, reply()));
        plait.forget_agent();
        assert!(!plait.accept_agent(&key, revision, reply()));
        // A policy refresh can reopen the same checkout on the same host connection.
        plait.agent.key = Some(key.clone());
        assert!(!plait.accept_agent(&key, revision, reply()));
        assert!(plait.accept_agent(&key, plait.agent.revision, reply()));
        assert_eq!(
            plait.agent.view.as_ref().unwrap().mcp_servers[0]
                .name
                .as_ref(),
            "local-docs"
        );
    });
    drop(harness);
    std::fs::remove_dir_all(scratch).unwrap();
}
